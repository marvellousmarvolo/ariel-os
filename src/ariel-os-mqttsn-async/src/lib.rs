#![no_std]
pub mod serialization;
pub mod udp_nal;

use ariel_os::net;
use ariel_os::reexports::embassy_time::WithTimeout; // TODO: when rebased, change to ariel_os::time::with_timeout
use ariel_os::time::Duration;
use ariel_os::{debug::log::*, reexports::embassy_time::TimeoutError};
use core::{default::Default, net::SocketAddr};
use embassy_executor::Spawner;
use embassy_futures::select::{Either3, select3};
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{Channel, Receiver, Sender},
    mutex::Mutex,
    once_lock::OnceLock,
    pubsub::PubSubChannel,
};
use embedded_nal_async::UnconnectedUdp;
use heapless::{LinearMap, String};
use serialization::{
    flags::{Flags, QoS, TopicIdType},
    header::{Header, HeaderLong, HeaderShort, MsgType},
    message_variable_part::{ConnAck, Publish, ReturnCode},
    packet::Packet,
};

pub const MAX_PAYLOAD_SIZE: usize = 1024; // !usize_from_env_or()
pub const MAX_TOPIC_LENGTH: usize = 64; // !usize_from_env_or()
pub const MAX_SUBSCRIPTIONS: usize = 16;
pub const MAX_CLIENTS: usize = 4;

type Payload = [u8; MAX_PAYLOAD_SIZE];
type ActionReply = Result<(), Error>;

pub type ActionRequestChannel<'ch> = Channel<CriticalSectionRawMutex, ActionRequest<'ch>, 1>;
pub type ActionReplyChannel = Channel<CriticalSectionRawMutex, ActionReply, 1>;
pub type ActionReplySender<'a> = Sender<'a, CriticalSectionRawMutex, ActionReply, 1>;
pub type MessageChannel = Channel<CriticalSectionRawMutex, Message, 1>;
pub type MessageReceiver = Receiver<'static, CriticalSectionRawMutex, Message, 1>;
pub type MessageSender = Sender<'static, CriticalSectionRawMutex, Message, 1>;

type TopicMap = LinearMap<u16, u16, MAX_SUBSCRIPTIONS>;
type SubscriptionList = [Option<MessageSender>; MAX_SUBSCRIPTIONS];
type MsgIdMap = LinearMap<u16, Option<MessageSender>, MAX_SUBSCRIPTIONS>;

pub static ACTION_REQUEST_CHANNEL: ActionRequestChannel<'_> = Channel::new();

#[derive(Clone)]
pub enum Message {
    Publish { topic: u16, payload: Payload },
}

#[derive(Clone)]
pub struct ActionRequest<'ch> {
    pub action: Action,
    pub reply_tx: ActionReplySender<'ch>,
}

#[derive(Clone)]
pub enum Action {
    Subscribe {
        topic: Topic,
        action_tx: MessageSender,
    },
    Publish {
        topic: Topic,
        payload: Payload,
    },
}

#[derive(PartialEq, Default, Debug, defmt::Format)]
enum State {
    #[default]
    Disconnected,
    Active,
}

#[derive(Debug, Copy, Clone, PartialEq, defmt::Format)]
pub enum Error {
    InvalidState,
    Timeout,
    TransmissionFailed,
    ConversionFailed,
    Rejected,
    InvalidIdType,
}

#[derive(Debug, defmt::Format, Clone, PartialEq)]
pub enum Topic {
    Id(u16),
    ShortName([u8; 2]),
    LongName(String<MAX_TOPIC_LENGTH>),
}

impl Topic {
    pub fn from_short(short_name: [u8; 2]) -> Topic {
        Self::ShortName(short_name)
    }

    pub fn from_id(id: u16) -> Topic {
        Self::Id(id)
    }

    pub fn from_long(long_name: &str) -> Topic {
        // TODO: make fallible
        Self::LongName(String::try_from(long_name).unwrap())
    }

    pub fn len(&self) -> usize {
        match self {
            Self::Id(_id) => 2,
            Self::ShortName(_short_name) => 2,
            Self::LongName(long_name) => long_name.len(),
        }
    }

    fn to_buf(&self, buf: &mut [u8]) {
        match self {
            Self::Id(id) => buf[..2].copy_from_slice(&id.to_be_bytes()),
            Self::ShortName(short_name) => buf[..2].copy_from_slice(short_name),
            Self::LongName(long_name) => {
                (&mut buf[..long_name.len()]).copy_from_slice(long_name.as_bytes())
            }
        }
    }
}

pub struct MqttClient {
    action_reply_channel: ActionReplyChannel,
    message_channel: MessageChannel,
}

impl MqttClient {
    pub const fn new() -> Self {
        Self {
            action_reply_channel: ActionReplyChannel::new(),
            message_channel: MessageChannel::new(),
        }
    }

    pub async fn subscribe(&'static self, topic: Topic) -> Result<(), Error> {
        ACTION_REQUEST_CHANNEL
            .send(ActionRequest {
                action: Action::Subscribe {
                    topic,
                    action_tx: self.message_channel.sender(),
                },
                reply_tx: self.action_reply_channel.sender(), // obsolete? Can be represented by message_channel
            })
            .await;
        self.action_reply_channel.receive().await
    }
}

trait MqttPacketReceive {
    async fn receive_packet<'b>(&mut self, buf: &'b mut [u8]) -> Result<Packet<'b>, Error>;
}

impl MqttPacketReceive for udp_nal::UnconnectedUdp<'_> {
    async fn receive_packet<'b>(&mut self, buf: &'b mut [u8]) -> Result<Packet<'b>, Error> {
        match self.receive_into(buf).await {
            Ok((n, _, _)) => {
                debug!("Bytes: {:?}", &buf[..n]);
                match Packet::try_from(&buf[..n]) {
                    Ok(packet) => Ok(packet),
                    Err(_) => Err(Error::ConversionFailed),
                }
            }
            Err(_) => Err(Error::TransmissionFailed),
        }
    }
}

pub struct MqttsnConnection<'a, 'ch> {
    stack: net::NetworkStack,
    state: State,
    local: SocketAddr,
    remote: SocketAddr,
    socket: udp_nal::UnconnectedUdp<'a>,
    action_rx: Receiver<'a, CriticalSectionRawMutex, ActionRequest<'ch>, 1>,
    msg_id: u16,
    topic_map: TopicMap,
    consumers: SubscriptionList,
    msg_id_map: MsgIdMap
}

trait RegisterSubscriber {
    fn register_subscriber(&mut self, topic: u16, index: u16) -> Result<(), Error>;
}

impl RegisterSubscriber for TopicMap {
    fn register_subscriber(&mut self, topic: u16, index: u16) -> Result<(), Error> {
        let bit = 1 << index;
        if let Some(entry) = self.get_mut(&topic) {
            *entry |= bit;
            Ok(())
        } else {
            self.insert(topic, bit);
            // TODO: catch map full error
            Ok(())
        }
    }
}

impl<'a, 'ch> MqttsnConnection<'a, 'ch> {
    async fn handle_action_request(
        &mut self,
        action_request: ActionRequest<'_>,
    ) -> Result<(), Error> {
        let ActionRequest { action, reply_tx } = action_request;
        let reply = self.handle_action(action).await;

        reply_tx.send(reply).await; // ?? move to SubAck receive

        Ok(())
    }

    async fn handle_action(&mut self, action: Action) -> Result<(), Error> {
        match action {
            Action::Subscribe{topic, action_tx: channel} => {
                info!("subscribe");

                // identify by message ID, save globally
                if self.msg_id_map.capacity() > self.msg_id_map.len() {
                    self.subscribe(topic, false, QoS::Zero).await?;
                    self.msg_id_map.insert(self.msg_id, Some(channel));
                } else {
                    info!("no free consumer slot");
                }
            }
            Action::Publish { topic, payload } => {
                info!("publish");
                self.publish(topic, &payload).await?;
            }
        }
        Ok(())
    }

    async fn handle_packet(&mut self, packet: Packet<'_>) -> Result<(), Error> {
        match packet {
            Packet::RegAck { header, reg_ack } => info!("RegAck {:?}", reg_ack.get_return_code()),
            Packet::Publish {
                header,
                publish,
                data,
            } => self.handle_publish(&publish, data).await?,
            Packet::SubAck { header, sub_ack } => {
                let return_code = sub_ack.get_return_code();
                info!("SubAck {:?}", return_code);
                match return_code {
                    ReturnCode::Accepted => {
                        let topic_id = sub_ack.get_topic_id();
                        info!("received Topic Id {:?}", topic_id);

                        if let Some(Some(channel)) = self.msg_id_map.remove(&sub_ack.get_msg_id()){
                            if let Some((i, entry)) = self
                                .consumers
                                .iter_mut()
                                .enumerate()
                                .find(|(_i, c)| c.is_none())
                            {
                                *entry = Some(channel);
                                self.topic_map.register_subscriber(topic_id, i as u16);
                            } else {
                                info!("no free consumer slot");
                            }
                        } else {
                            info!("No return channel for received msg_id. Ignore SubAck");
                        };
                        
                    }
                    ReturnCode::RejectedCongestion => todo!(),
                    ReturnCode::RejectedInvalidTopicId => todo!(),
                    ReturnCode::RejectedNotSupported => todo!(),
                }
            }
            Packet::PingReq { header, client_id } => info!("TODO: PingReq {:?}", client_id),
            _ => panic!("Unexpected message type received"),
        }
        Ok(())
    }

    async fn run(&mut self) {
        // TODO: handle errors
        loop {
            while self.state == State::Disconnected {
                if let Err(e) = self.connect(60000, b"ariel", false, false).await {
                    match e {
                        Error::Timeout => info!("TIMEOUT ERROR"),
                        Error::TransmissionFailed => info!("TRANSMISSION ERROR"),
                        _ => info!("OTHER ERROR"),
                    }
                }
            }

            // TODO: "32" is arbitrary, check MAX_PAYLOAD_SIZE and add appropriate header length
            let mut rx_buf = [0; MAX_PAYLOAD_SIZE + 32];
            loop {
                match select3(
                    self.socket.receive_packet(&mut rx_buf),
                    self.action_rx.receive(),
                    self.stack.wait_config_down(),
                )
                .await
                {
                    Either3::First(Ok(packet)) => {
                        info!("got packet");
                        self.handle_packet(packet).await.unwrap();
                    }
                    Either3::First(Err(_)) => todo!(),
                    Either3::Second(action_request) => {
                        info!("got action");
                        self.handle_action_request(action_request).await.unwrap();
                    }
                    Either3::Third(_) => {
                        info!("got config down");
                        self.state = State::Disconnected;
                        break;
                    }
                }
            }
        }
    }

    fn check_state(&self, state: State) -> Result<(), Error> {
        if self.state != state {
            return Err(Error::InvalidState);
        }
        Ok(())
    }

    pub async fn connect(
        &mut self,
        duration_millis: u16,
        client_id: &[u8],
        will: bool,
        clean_session: bool,
    ) -> Result<(), Error> {
        info!("connect");
        self.check_state(State::Disconnected)?;
        let mut buf: [u8; MAX_PAYLOAD_SIZE + 32] = [0; MAX_PAYLOAD_SIZE + 32];

        self.send_connect(&mut buf, clean_session, will, client_id)
            .await?;

        info!("connect: recv");
        // wait for connack
        match self
            .socket
            .receive_packet(&mut buf)
            .with_timeout(Duration::from_millis(duration_millis as u64))
            .await
            .map_err(|_| Error::Timeout)??
        {
            Packet::ConnAck { header, conn_ack } => match conn_ack.get_return_code() {
                ReturnCode::Accepted => {
                    self.state = State::Active;
                    info!("connected");
                    Ok(())
                }
                ReturnCode::RejectedCongestion => todo!(),
                ReturnCode::RejectedInvalidTopicId => todo!(),
                ReturnCode::RejectedNotSupported => todo!(),
            },
            _ => {
                info!("Transmission failed!");
                Err(Error::TransmissionFailed)
            }
        }
    }

    pub async fn subscribe(&mut self, topic: Topic, dup: bool, qos: QoS) -> Result<(), Error> {
        info!("send subscribe. state: {:?}", self.state);
        self.check_state(State::Active)?;

        let mut buf: [u8; MAX_PAYLOAD_SIZE + 32] = [0; MAX_PAYLOAD_SIZE + 32];

        let packet = Packet::subscribe(&topic, dup, qos, self.msg_id);

        {
            let packet_slice = packet.write_to_buf(&mut buf);
            self.send_packet(packet_slice).await?;
        }

        self.msg_id += 1;
        Ok(())
    }

    pub async fn publish(&mut self, topic: Topic, payload: &[u8]) -> Result<(), Error> {
        self.check_state(State::Active)?;

        let mut buf: [u8; MAX_PAYLOAD_SIZE + 32] = [0; MAX_PAYLOAD_SIZE + 32];

        let packet = Packet::publish(&topic, QoS::Zero, payload);

        {
            let packet_slice = packet.write_to_buf(&mut buf);
            self.send_packet(packet_slice).await?;
        }

        Ok(())
    }

    async fn send_packet(&mut self, buf: &[u8]) -> Result<(), Error> {
        match self.socket.send(self.local, self.remote, &buf).await {
            Ok(_) => Ok(()),
            Err(_) => Err(Error::TransmissionFailed),
        }
    }

    async fn send_connect(
        &mut self,
        buf: &mut [u8],
        clean_session: bool,
        will: bool,
        client_id: &[u8],
    ) -> Result<(), Error> {
        let packet = Packet::connect(clean_session, will, client_id);
        info!("connect: send");
        let packet_slice = packet.write_to_buf(buf);
        self.send_packet(packet_slice).await
    }

    async fn handle_publish(&mut self, publish: &Publish, data: Payload) -> Result<(), Error> {
        let topic_id = publish.get_topic_id();
        info!("publish topic={}", topic_id);

        for (topic, channel_bitmap) in self.topic_map.iter().filter(|(key, _)| **key == topic_id) {
            let channel_bitmap = *channel_bitmap;
            while channel_bitmap.leading_zeros() < 16 {
                let channel_id = channel_bitmap.trailing_zeros() as usize;
                let channel = self.consumers[channel_id].as_ref().unwrap();
                let _ = channel
                    .send(Message::Publish {
                        topic: *topic,
                        payload: data,
                    })
                    .await;
                let bit = 1 << channel_id;
                let channel_bitmap = channel_bitmap & !bit;
            }
        }

        Ok(())
    }
}

#[embassy_executor::task]
pub async fn client() {
    let stack = net::network_stack().await.unwrap();
    stack.wait_config_up().await;
    let local = "0.0.0.0:1234".parse().unwrap();
    let remote = "192.168.1.236:1884".parse().unwrap();
    //let remote = SocketAddr::new(stack.config_v4().unwrap().gateway.unwrap().into(), 1884);

    let mut rx_meta = [PacketMetadata::EMPTY; 16];
    let mut rx_buffer = [0; 4096];
    let mut tx_meta = [PacketMetadata::EMPTY; 16];
    let mut tx_buffer = [0; 4096];

    let socket = UdpSocket::new(
        stack,
        &mut rx_meta,
        &mut rx_buffer,
        &mut tx_meta,
        &mut tx_buffer,
    );

    let mut socket = udp_nal::UnconnectedUdp::bind_multiple(socket, local)
        .await
        .unwrap();

    let action_rx = ACTION_REQUEST_CHANNEL.receiver();
    let mut connection = MqttsnConnection {
        msg_id: 0,
        stack,
        socket,
        local,
        remote,
        action_rx,
        state: State::Disconnected,
        topic_map: TopicMap::new(),
        consumers: SubscriptionList::default(),
        msg_id_map: MsgIdMap::new()
    };

    connection.run().await;
}

// async fn ping() -> Result<(), Error> {
//     const MSG_LEN: u16 = 16;
//     let packet = Packet::PingResp {
//         header: Header::new(MsgType::PingResp, MSG_LEN),
//     };
//     let mut send_buf = [0u8; MSG_LEN];
//     packet.write_to_buf(send_buf);
//     send(send_buf, MSG_LEN).await?;
//     Ok(())
// }
