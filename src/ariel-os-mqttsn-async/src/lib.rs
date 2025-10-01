#![no_std]
pub mod flags;
mod header;
mod message_variable_part;
mod packet;
pub mod udp_nal;

use crate::flags::{QoS, TopicIdType};
use crate::message_variable_part::ReturnCode;
use ariel_os::net;
use ariel_os::reexports::embassy_time::WithTimeout; // TODO: when rebased, change to ariel_os::time::with_timeout
use ariel_os::time::Duration;
use ariel_os::{debug::log::*, reexports::embassy_time::TimeoutError};
use core::{default::Default, net::SocketAddr};
use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{Channel, Receiver},
    mutex::Mutex,
    once_lock::OnceLock,
    pubsub::PubSubChannel,
};
use embedded_nal_async::UnconnectedUdp;
use header::{Header, HeaderLong, HeaderShort, MsgType};
use heapless::String;
use packet::Packet;

use message_variable_part as mvp;

pub const MAX_PAYLOAD_SIZE: usize = 1024; // !usize_from_env_or()

pub const MAX_TOPIC_LENGTH: usize = 64; // !usize_from_env_or()
// pub const MAX_PUBLISHERS: usize = 16;

// type MessageChannel = PubSubChannel<CriticalSectionRawMutex, Message, 1, 1, MAX_PUBLISHERS>;

// static SOCKET: OnceLock<MqttsnSocket> = OnceLock::new();
// static MESSAGE_CHANNEL: MessageChannel = Channel<CriticalSectionRawMutex, Message, 1> = Channel::new();
pub static ACTION_CHANNEL: Channel<CriticalSectionRawMutex, Action, 1> = Channel::new();

// static HANDLERS: Mutex<CriticalSectionRawMutex, [MessageChannel; MAX_PUBLISHERS]> = Mutex::new([]);
// static SUBSCRIPTIONS: Mutex<CriticalSectionRawMutex, [(u16, u16); MAX_PUBLISHERS]> = Mutex::new([]);

type Payload = [u8; MAX_PAYLOAD_SIZE];

#[derive(Clone)]
pub enum Message {
    Publish { topic: u16, payload: Payload },
}

#[derive(Clone)]
pub enum Action {
    Subscribe(Topic),
    Publish { topic: Topic, payload: Payload },
}

#[derive(PartialEq, Default, Debug, defmt::Format)]
enum State {
    #[default]
    Disconnected,
    Active,
}

#[derive(Debug)]
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

pub struct MqttsnConnection<'a> {
    stack: net::NetworkStack,
    state: State,
    local: SocketAddr,
    remote: SocketAddr,
    socket: udp_nal::UnconnectedUdp<'a>,
    action_rx: Receiver<'a, CriticalSectionRawMutex, Action, 1>,
    msg_id: u16,
}

trait MqttPacketReceive {
    async fn receive_packet<'b>(&mut self, buf: &'b mut [u8]) -> Result<Packet<'b>, Error>;
}

impl MqttPacketReceive for udp_nal::UnconnectedUdp<'_> {
    async fn receive_packet<'b>(&mut self, buf: &'b mut [u8]) -> Result<Packet<'b>, Error> {
        match self.receive_into(buf).await {
            Ok((n, _, _)) => {
                info!("Bytes: {:?}", &buf[..n]);
                match Packet::try_from(&buf[..n]) {
                    Ok(packet) => Ok(packet),
                    Err(_) => Err(Error::ConversionFailed),
                }
            }
            Err(_) => Err(Error::TransmissionFailed),
        }
    }
}

impl<'a> MqttsnConnection<'a> {
    async fn handle_action(&mut self, action: Action) -> Result<(), Error> {
        match action {
            Action::Subscribe(topic) => {
                info!("subscribe");
                self.subscribe(topic, false, QoS::Zero).await?;
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
            Packet::ConnAck { header, conn_ack } => {
                // remove here?
                info!("ConnAck {:?}", conn_ack.get_return_code())
            }
            Packet::RegAck { header, reg_ack } => info!("RegAck {:?}", reg_ack.get_return_code()),
            Packet::Publish {
                header,
                publish,
                data,
            } => info!("Publish  {:?}", data),
            Packet::SubAck { header, sub_ack } => {
                let return_code = sub_ack.get_return_code();
                info!("SubAck {:?}", return_code);
                match return_code {
                    ReturnCode::Accepted => {
                        let topic = Topic::from_id(sub_ack.get_topic_id());
                        info!("received Topic Id {:?}", topic);
                    }
                    ReturnCode::RejectedCongestion => todo!(),
                    ReturnCode::RejectedInvalidTopicId => todo!(),
                    ReturnCode::RejectedNotSupported => todo!(),
                }
            }
            Packet::PingReq { header, client_id } => info!("PingReq {:?}", client_id),
            _ => panic!("Unexpected message type received"),
        }
        Ok(())
    }

    async fn run(&mut self) {
        self.stack.wait_config_up().await;
        // TODO: loop, handle errors
        match self.connect(60000, b"ariel", false, false).await {
            Ok(_) => (),
            Err(e) => match e {
                Error::Timeout => info!("TIMEOUT ERROR"),
                Error::TransmissionFailed => info!("TRANSMISSION ERROR"),
                _ => info!("OTHER ERROR"),
            },
        }

        // TODO: "32" is arbitrary, check MAX_PAYLOAD_SIZE and add appropriate header length
        let mut rx_buf = [0; MAX_PAYLOAD_SIZE + 32];
        loop {
            match select(
                self.socket.receive_packet(&mut rx_buf),
                self.action_rx.receive(),
            )
            .await
            {
                Either::First(Ok(packet)) => {
                    info!("got packet");
                    self.handle_packet(packet).await.unwrap();
                }
                Either::First(Err(_)) => todo!(),
                Either::Second(action) => {
                    info!("got action");
                    self.handle_action(action).await.unwrap();
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
        use flags::{Flags, QoS, TopicIdType};
        info!("connect");
        self.check_state(State::Disconnected)?;
        let mut buf: [u8; MAX_PAYLOAD_SIZE + 32] = [0; MAX_PAYLOAD_SIZE + 32];

        let packet = Packet::connect(clean_session, will, client_id);

        {
            info!("connect: send");
            let packet_slice = packet.write_to_buf(&mut buf);
            self.send(packet_slice).await?;
        }

        // Ok(())

        // TODO: move into central handler -> run()
        info!("connect: recv");
        // wait for connack
        match self
            .receive(&mut buf)
            .with_timeout(Duration::from_millis(duration_millis as u64))
            .await
            .map_err(|_| Error::Timeout)?
            .map(|res| res.get_msg_type())?
        {
            MsgType::ConnAck => {
                self.state = State::Active;
                Ok(())
            }
            _ => {
                info!("Transmission failed!");
                Err(Error::TransmissionFailed)
            }
        }
    }

    pub async fn subscribe(&mut self, topic: Topic, dup: bool, qos: QoS) -> Result<(), Error> {
        info!("send subscribe");
        info!("{:?}", self.state);
        self.check_state(State::Active)?;

        let mut buf: [u8; MAX_PAYLOAD_SIZE + 32] = [0; MAX_PAYLOAD_SIZE + 32];

        let packet = Packet::subscribe(&topic, dup, qos, self.msg_id);

        {
            let packet_slice = packet.write_to_buf(&mut buf);
            self.send(packet_slice).await?;
        }

        self.msg_id += 1;
        Ok(())

        // match self
        //     .receive(&mut buf)
        //     .with_timeout(Duration::from_millis(60000))
        //     .await
        // {
        //     Ok(res) => match res? {
        //         Packet::SubAck { header: _, sub_ack } => {
        //             let topic_id = sub_ack.get_topic_id();
        //             info!("received Topic Id {:?} for Topic {:?}", topic_id, topic);
        //             Ok(Topic::from_id(topic_id))
        //         }
        //         _ => Err(Error::TransmissionFailed),
        //     },
        //     Err(_) => Err(Error::Timeout),
        // }
    }

    pub async fn publish(&mut self, topic: Topic, payload: &[u8]) -> Result<(), Error> {
        self.check_state(State::Active)?;

        let mut buf: [u8; MAX_PAYLOAD_SIZE + 32] = [0; MAX_PAYLOAD_SIZE + 32];

        let packet = Packet::publish(&topic, QoS::Zero, payload);

        {
            let packet_slice = packet.write_to_buf(&mut buf);
            self.send(packet_slice).await?;
        }

        Ok(())
    }

    async fn send(&mut self, buf: &[u8]) -> Result<(), Error> {
        match self.socket.send(self.local, self.remote, &buf).await {
            Ok(_) => Ok(()),
            Err(_) => Err(Error::TransmissionFailed),
        }
    }
    async fn receive<'b>(&mut self, buf: &'b mut [u8]) -> Result<Packet<'b>, Error> {
        match self.socket.receive_into(buf).await {
            Ok((n, _, _)) => {
                info!("Bytes: {:?}", &buf[..n]);
                match Packet::try_from(&buf[..n]) {
                    Ok(packet) => Ok(packet),
                    Err(_) => Err(Error::ConversionFailed),
                }
            }
            Err(_) => Err(Error::TransmissionFailed),
        }
    }
}

#[embassy_executor::task]
pub async fn client() {
    let stack = net::network_stack().await.unwrap();
    let local = "0.0.0.0:1234".parse().unwrap();
    let remote: SocketAddr = "10.42.0.1:1884".parse().unwrap();

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

    let action_rx = ACTION_CHANNEL.receiver();
    let mut connection = MqttsnConnection {
        msg_id: 0,
        stack,
        socket,
        local,
        remote,
        action_rx,
        state: State::Disconnected,
    };

    connection.run().await;
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

// async fn send(buf: &[u8], length: u16) -> Result<(), Error> {
//     let socket = SOCKET.get().await;

//     match socket
//         .socket
//         .send(socket.local, socket.remote, &buf[..length as usize])
//         .await
//     {
//         Ok(_) => Ok(()),
//         Err(_) => Err(Error::TransmissionFailed),
//     };
// }

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

// pub async fn init(
//     max_buffer_size: u16,
//     client_id: &[u8],
//     socket: MqttsnSocket<'_>,
// ) -> Result<(), Error> {
//     // Initialize Buffer Size, Global Lists, Socket
//     // MQTT-SN Connect
//     // Start Client Task
// }

// #[embassy_executor::task]
// async fn client() {
//     loop {
//         let mut buf = [0u8; MAX_PAYLOAD_SIZE + 32];

//         match receive(&mut buf).await {
//             Ok(packet) => match packet {
//                 Packet::Publish {
//                     header: _,
//                     publish,
//                     data,
//                 } => {
//                     let topic_id = publish.get_topic_id();

//                     SUBSCRIPTIONS.lock().await.map(|subs| async {
//                         if subs.0 == topic_id {
//                             for i in 0..16u16 {
//                                 // check if bit is set in handler bitmap
//                                 if subs.1 & (1 << i) != 0 {
//                                     let handlers = HANDLERS.lock().await;
//                                     let handler: MessageChannel = handlers[i as usize];
//                                     handler.publisher().unwrap().publish_immediate(
//                                         Message::Publish {
//                                             topic: topic_id,
//                                             payload: data,
//                                         },
//                                     );
//                                 }
//                             }
//                         }
//                     });
//                     continue;
//                 }
//                 Packet::PingReq { .. } => match ping().await {
//                     Ok(_) => continue,
//                     Err(_) => (), // handle Error
//                 },
//                 _ => {
//                     info!("Conversion error");
//                     continue;
//                 }
//             },
//             Err(_) => {
//                 info!("Packet not recognized");
//                 continue;
//             }
//         }
//     }
// }

// fn calculate_message_length(payload_len: usize, mvp_byte_len: usize) -> u16 {
//     use bilge::Bitsized;
//     let mvp_len = payload_len + mvp_byte_len;

//     if (mvp_len + HeaderShort::BITS / 8) > 256 {
//         (mvp_len + HeaderLong::BITS / 8) as u16
//     } else {
//         (mvp_len + HeaderShort::BITS / 8) as u16
//     }
// }
