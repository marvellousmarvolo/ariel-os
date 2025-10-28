#![no_std]
pub mod client;
pub mod error;
pub mod serialization;
pub mod udp_nal;

use crate::error::Error;
use crate::{client::*, serialization::message_variable_part::SubAck};

use ariel_os::debug::log::*;
use ariel_os::net;
use ariel_os::reexports::embassy_time::WithTimeout; // TODO: when rebased, change to ariel_os::time::with_timeout
use ariel_os::time::Duration;
use core::{default::Default, net::SocketAddr};
use embassy_futures::select::{Either3, select3};
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Receiver};
use embedded_nal_async::UnconnectedUdp;
use heapless::{LinearMap, Vec};
use serialization::{
    flags::QoS,
    message_variable_part::{Publish, ReturnCode},
    packet::Packet,
};

pub const MAX_SUBSCRIPTIONS: usize = 16;
pub const MAX_CLIENTS: usize = 4;

// pub type MessageReceiver = Receiver<'static, CriticalSectionRawMutex, Message, 1>;

// type MsgIdSubscriptionMap = LinearMap<u16, u16, MAX_SUBSCRIPTIONS>;
type TopicMap = LinearMap<u16, u16, MAX_SUBSCRIPTIONS>;
type SubscriptionList = [Option<MessageSender>; MAX_SUBSCRIPTIONS];
type MsgIdMap = LinearMap<u16, Option<MessageSender>, MAX_SUBSCRIPTIONS>;

#[derive(PartialEq, Default, Debug, defmt::Format)]
enum State {
    #[default]
    Disconnected,
    Active,
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
            match self.insert(topic, bit) {
                Ok(_) => Ok(()),
                // TODO: is there a better error?
                Err(_) => Err(Error::NoFreeSubscriberSlot),
            }
        }
    }
}

#[embassy_executor::task]
pub async fn start() {
    let stack = net::network_stack().await.unwrap();
    stack.wait_config_up().await;
    let local = "0.0.0.0:1234".parse().unwrap();
    let remote = "192.168.1.129:1884".parse().unwrap();// !usize_from_env_or()
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

    let socket = udp_nal::UnconnectedUdp::bind_multiple(socket, local)
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
        msg_id_map: MsgIdMap::new(),
    };

    connection.run().await;
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
    msg_id_map: MsgIdMap,
}

impl<'a, 'ch> MqttsnConnection<'a, 'ch> {
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
                // TODO: Cancel-Safety of select statement
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

    async fn handle_action_request(
        &mut self,
        action_request: ActionRequest<'_>,
    ) -> Result<(), Error> {
        let ActionRequest {
            action,
            response_tx,
        } = action_request;
        let action_response = self.handle_action(action).await;

        response_tx.send(action_response).await;

        Ok(())
    }

    async fn handle_action(&mut self, action: Action) -> Result<ActionResponse, Error> {
        match action {
            Action::Subscribe { topic, message_tx } => {
                info!("subscribe");

                // TODO: check here if there is actually a free subscriber slot?

                // identify by message ID, pre-reserve id_map slot
                if self.msg_id_map.capacity() > self.msg_id_map.len() {
                    let msgid = self.subscribe(topic, false, QoS::Zero).await?;
                    // always succeeds, space checked above. can ignore error.
                    let _ = self.msg_id_map.insert(msgid, Some(message_tx));
                    Ok(ActionResponse::Subscription { msgid })
                } else {
                    Err(Error::NoFreeSubscriberSlot)
                }
            }
            Action::Publish { topic, payload } => {
                info!("publish");
                self.publish(topic, &payload).await?;
                Ok(ActionResponse::Ok)
            }
        }
    }

    async fn handle_packet(&mut self, packet: Packet<'_>) -> Result<(), Error> {
        match packet {
            Packet::RegAck { header: _, reg_ack } => {
                info!("RegAck {:?}", reg_ack.get_return_code())
            }
            Packet::Publish {
                header: _,
                publish,
                data,
            } => {
                // unwrap ok, data len checked earlier
                self.handle_publish(&publish, Vec::from_slice(data).unwrap())
                    .await?
            }
            Packet::SubAck { header: _, sub_ack } => self.handle_sub_ack(sub_ack).await?,
            Packet::PingReq {
                header: _,
                client_id,
            } => info!("TODO: PingReq {:?}", client_id),
            _ => panic!("Unexpected message type received"),
        }
        Ok(())
    }

    async fn handle_sub_ack(&mut self, sub_ack: SubAck) -> Result<(), Error> {
        let return_code = sub_ack.get_return_code();
        info!("SubAck {:?}", return_code);
        match return_code {
            ReturnCode::Accepted => {
                let topic_id = sub_ack.get_topic_id();
                info!("received Topic Id {:?}", topic_id);

                if let Some(Some(message_tx)) = self.msg_id_map.remove(&sub_ack.get_msg_id()) {
                    if let Some((i, consumer_entry)) = self
                        .consumers
                        .iter_mut()
                        .enumerate()
                        .find(|(_i, c)| c.is_none())
                    {
                        *consumer_entry = Some(message_tx);
                        // TODO: handle error
                        let _ = self.topic_map.register_subscriber(topic_id, i as u16);

                        message_tx
                            .send(Message::TopicInfo {
                                msgid: sub_ack.get_msg_id(),
                                topic_id,
                            })
                            .await;
                    } else {
                        info!("no free consumer slot");
                        // TODO: unsubscribe now or not
                    }
                } else {
                    // TODO: unsubscribe now or not
                    info!("No return channel for received msg_id. Ignore SubAck");
                };
            }
            ReturnCode::RejectedCongestion => todo!(),
            ReturnCode::RejectedInvalidTopicId => todo!(),
            ReturnCode::RejectedNotSupported => todo!(),
        }
        Ok(())
    }

    async fn handle_publish(&mut self, publish: &Publish, data: Payload) -> Result<(), Error> {
        let topic_id = publish.get_topic_id();
        info!("publish topic={}", topic_id);

        for (topic, channel_bitmap) in self.topic_map.iter().filter(|(key, _)| **key == topic_id) {
            // TODO: explain channel bitmap
            let mut channel_bitmap = *channel_bitmap;
            while channel_bitmap.leading_zeros() < 16 {
                let channel_id = channel_bitmap.trailing_zeros() as usize;
                let channel = self.consumers[channel_id].as_ref().unwrap();
                let _ = channel
                    .send(Message::Publish {
                        topic: *topic,
                        payload: data.clone(),
                    })
                    .await;
                let bit = 1 << channel_id;
                channel_bitmap = channel_bitmap & !bit;
            }
        }

        Ok(())
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
            Packet::ConnAck {
                header: _,
                conn_ack,
            } => match conn_ack.get_return_code() {
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

    pub async fn subscribe(&mut self, topic: Topic, dup: bool, qos: QoS) -> Result<u16, Error> {
        let msg_id = self.get_next_msg_id();

        info!("send subscribe. state: {:?}", self.state);
        self.check_state(State::Active)?;

        let mut buf: [u8; MAX_PAYLOAD_SIZE + 32] = [0; MAX_PAYLOAD_SIZE + 32];

        let packet = Packet::subscribe(&topic, dup, qos, msg_id);

        {
            let packet_slice = packet.write_to_buf(&mut buf);
            self.send_packet(packet_slice).await?;
        }

        Ok(msg_id)
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

    fn get_next_msg_id(&mut self) -> u16 {
        let msg_id = self.msg_id;
        self.msg_id += 1;
        msg_id
    }
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
