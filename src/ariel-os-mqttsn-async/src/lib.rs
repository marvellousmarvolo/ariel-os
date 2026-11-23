#![no_std]
pub mod client;
pub mod error;
pub mod serialization;
pub mod settings;
pub mod topic;
pub mod transport;

use crate::error::Error;
use crate::serialization::header::{HeaderLong, HeaderShort};
use crate::transport::{
    // TcpTransport,
    Transport,
    UdpTransport,
};
use crate::{
    client::*,
    serialization::message_variable_part::{RegAck, SubAck},
    topic::{TopicFilter, TopicId},
};
use ariel_os_debug_log::*;
use ariel_os_embassy::net;
use ariel_os_utils::ipv4_addr_from_env;
use bilge::Bitsized;
use core::{default::Default, net::Ipv4Addr};
use embassy_futures::select::{Either4, select4};
use embassy_net::{
    IpEndpoint,
    // tcp::TcpSocket,
    udp::{PacketMetadata, UdpSocket},
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Receiver};
use embassy_time::{Duration, Instant, Timer, WithTimeout};
use heapless::{LinearMap, String, Vec};
use serialization::{
    flags::QoS,
    message_variable_part::{PubAck, Publish, ReturnCode},
    packet::Packet,
};
use settings::Settings;

pub const MAX_SUBSCRIPTIONS: usize = 16;
pub const T_ADV: Duration = Duration::from_secs(15 * 60);
pub const N_ADV: usize = 3;
pub const T_SEARCHGW: Duration = Duration::from_secs(5);
pub const T_GWINFO: Duration = Duration::from_secs(5);
pub const T_WAIT: Duration = Duration::from_secs(5 * 60);
pub const T_RETRY: Duration = Duration::from_secs(15);
pub const N_RETRY: usize = 5;

/// Contains the mapping between the Topic ID of a subscription and a bitmap indicating which Clients shall receive the incoming Message
type TopicMap = LinearMap<TopicId, u16, MAX_SUBSCRIPTIONS>;
/// Contains Senders for permanently relaying incoming Messages to the subscribed Clients, identified by their index
type MessageTxList = [Option<MessageSender>; MAX_SUBSCRIPTIONS];
/// Helper structure for temporarily matching the returning Message ID with the request awaiting it
type MsgIdMap = LinearMap<u16, PendingRequest, MAX_SUBSCRIPTIONS>;

/// A request sent to the gateway, waiting for its acknowledgement.
struct PendingRequest {
    request: Request,
    response_tx: ActionReplySender,
    attempts: usize,
    deadline: Instant,
}

/// The requests the gateway answers.
enum Request {
    /// A SUBSCRIBE, remembered along with its filter, which gives meaning to the topic ID of the SUBACK, and is needed again should the client have to unsubscribe.
    Subscription {
        filter: TopicFilter,
        message_tx: MessageSender,
        quality_of_service: QoS,
    },
    /// A REGISTER, whose REGACK always carries a normal topic ID.
    Registration {
        topic_name: String<MAX_TOPIC_LENGTH>,
    },
    /// A PUBLISH, which is answered by a PUBACK.
    Publication {
        topic: TopicId,
        payload: Payload,
        quality_of_service: QoS,
    },
}

#[derive(PartialEq, Default, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
enum State {
    #[default]
    Disconnected,
    Active,
}

#[allow(async_fn_in_trait)]
pub trait MqttPacketReceive {
    async fn receive_packet<'b>(&mut self, buf: &'b mut [u8]) -> Result<Packet<'b>, Error>;
}

impl<T: Transport> MqttPacketReceive for T {
    async fn receive_packet<'b>(&mut self, buf: &'b mut [u8]) -> Result<Packet<'b>, Error> {
        match self.recv(buf).await {
            Ok(n) => {
                debug!("Received Packet Bytes: {:?}", &buf[..n]);
                match Packet::try_from(&buf[..n]) {
                    Ok(packet) => Ok(packet),
                    Err(_) => Err(Error::ConversionFailed),
                }
            }
            Err(e) => Err(e),
        }
    }
}

trait RegisterSubscriber {
    fn register_subscriber(&mut self, topic: TopicId, index: u16) -> Result<(), Error>;
}

impl RegisterSubscriber for TopicMap {
    fn register_subscriber(&mut self, topic: TopicId, index: u16) -> Result<(), Error> {
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
pub async fn start(settings: Settings<'static>) {
    let stack = net::network_stack().await.unwrap();
    stack.wait_config_up().await;

    let local_port = 1234;
    let local = IpEndpoint::new(Ipv4Addr::new(0, 0, 0, 0).into(), local_port);
    let remote_ip = ipv4_addr_from_env!("MQTT_BROKER_ADDR", "static IPv4 MQTT broker address");
    let remote = IpEndpoint::new(remote_ip.into(), 1884);

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
    let mut transport = UdpTransport::bind(socket, local, remote).expect("Bind error");

    // let socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
    // let mut transport= &mut TcpTransport::connect(socket, remote).await.expect("Connect Error");

    let mut connection = MqttsnConnection {
        client_id: settings.client_id(),
        msg_id: 1,
        stack,
        transport: &mut transport,
        action_rx: ACTION_REQUEST_CHANNEL.receiver(),
        state: State::Disconnected,
        topic_map: TopicMap::new(),
        message_tx_list: MessageTxList::default(),
        msg_id_map: MsgIdMap::new(),
        keepalive: settings.keepalive(),
        clean_session: true,
    };

    connection.run().await;
}

pub struct MqttsnConnection<'a, T: Transport + MqttPacketReceive> {
    client_id: &'a [u8],
    stack: net::NetworkStack,
    state: State,
    transport: &'a mut T,
    action_rx: Receiver<'a, CriticalSectionRawMutex, ActionRequest, 1>,
    msg_id: u16,
    topic_map: TopicMap,
    message_tx_list: MessageTxList,
    msg_id_map: MsgIdMap,
    keepalive: u16,
    clean_session: bool,
}

impl<'a, T: Transport + MqttPacketReceive> MqttsnConnection<'a, T> {
    async fn run(&mut self) {
        loop {
            while self.state == State::Disconnected {
                if let Err(e) = self
                    .connect(self.keepalive, self.client_id, false, self.clean_session)
                    .await
                {
                    match e {
                        Error::Timeout => {
                            error!("TIMEOUT ERROR");
                        }
                        Error::TransmissionFailed => {
                            error!("TRANSMISSION ERROR");
                        }
                        _ => {
                            error!("OTHER ERROR");
                        }
                    }
                }
            }

            const HEADER_SIZE: usize = match MAX_PAYLOAD_SIZE > 256 as usize {
                true => HeaderLong::BITS,
                false => HeaderShort::BITS,
            };
            let mut rx_buf = [0; MAX_PAYLOAD_SIZE + HEADER_SIZE];

            while self.state == State::Active {
                let keepalive_at = match self.keepalive > 0 {
                    true => Instant::now() + Duration::from_secs(self.keepalive.into()),
                    false => Instant::MAX,
                };
                // Wake up for the keepalive, or for the first request whose answer is overdue
                let wakeup_at = self
                    .msg_id_map
                    .values()
                    .map(|pending| pending.deadline)
                    .fold(keepalive_at, core::cmp::min);

                // TODO: Cancel-Safety of select statement
                match select4(
                    self.transport.receive_packet(&mut rx_buf),
                    self.action_rx.receive(), //receive with_timeout / Deadline!!
                    self.stack.wait_config_down(),
                    Timer::at(wakeup_at),
                )
                .await
                {
                    Either4::First(packet_result) => match packet_result {
                        Ok(packet) => {
                            debug!("Got packet. Start handling...");
                            self.handle_packet(packet).await.unwrap();
                        }
                        Err(e) => error!("Got receive_packet error: {:?}", e),
                    },
                    Either4::Second(action_request) => {
                        info!("Got action. Start handling...");
                        self.handle_action_request(action_request).await.unwrap();
                    }
                    Either4::Third(_) => {
                        warn!("Got config down. Attempting to reconnect...");
                        self.state = State::Disconnected;
                    }
                    Either4::Fourth(_) => {
                        if Instant::now() >= keepalive_at {
                            debug!("Got keepalive timeout. Sending Ping Request...");
                            let _ = self.ping_req().await;
                        }
                        self.retransmit_overdue().await;
                    }
                }
            }
        }
    }

    async fn handle_action_request(&mut self, action_request: ActionRequest) -> Result<(), Error> {
        let ActionRequest {
            action,
            response_tx,
        } = action_request;
        let action_response = self.handle_action(action, response_tx).await;

        response_tx.send(action_response).await;

        Ok(())
    }

    async fn handle_action(
        &mut self,
        action: Action,
        response_tx: ActionReplySender,
    ) -> Result<ActionResponse, Error> {
        match action {
            Action::Subscribe {
                topic,
                message_tx,
                quality_of_service,
            } => {
                debug!("subscribe");

                self.send_request(
                    Request::Subscription {
                        filter: topic,
                        message_tx,
                        quality_of_service,
                    },
                    response_tx,
                )
                .await
            }
            Action::Register { topic_name } => {
                debug!("register");

                self.send_request(Request::Registration { topic_name }, response_tx)
                    .await
            }
            Action::Publish {
                topic,
                payload,
                quality_of_service,
            } => {
                debug!("publish");

                // The specification reserves msg_id 0x0000 for QoS 0
                if quality_of_service == QoS::Zero {
                    self.publish(0x0000, topic, &payload, quality_of_service, false)
                        .await?;
                    return Ok(ActionResponse::Ok);
                }

                self.send_request(
                    Request::Publication {
                        topic,
                        payload,
                        quality_of_service,
                    },
                    response_tx,
                )
                .await
            }
            Action::Disconnect { duration } => {
                debug!("disconnect");

                self.disconnect(duration).await?;
                Ok(ActionResponse::Ok)
            }
        }
    }

    /// Sends a request to the gateway, and keeps it until the gateway answers.
    async fn send_request(
        &mut self,
        request: Request,
        response_tx: ActionReplySender,
    ) -> Result<ActionResponse, Error> {
        if self.msg_id_map.len() == self.msg_id_map.capacity() {
            return Err(Error::NoFreeSubscriberSlot);
        }

        let msg_id = self.get_next_msg_id();
        self.transmit(msg_id, &request, false).await?;

        // Always succeeds, since a free slot was checked for above
        let _ = self.msg_id_map.insert(
            msg_id,
            PendingRequest {
                request,
                response_tx,
                attempts: 1,
                deadline: Instant::now() + T_RETRY,
            },
        );
        Ok(ActionResponse::Pending { msg_id })
    }

    /// Sends every request whose answer is overdue once more.
    async fn retransmit_overdue(&mut self) {
        let now = Instant::now();
        let overdue: Vec<u16, MAX_SUBSCRIPTIONS> = self
            .msg_id_map
            .iter()
            .filter(|(_, pending)| pending.deadline <= now)
            .map(|(&msg_id, _)| msg_id)
            .collect();

        for msg_id in overdue {
            let Some(mut pending) = self.msg_id_map.remove(&msg_id) else {
                continue;
            };

            if pending.attempts == N_RETRY {
                warn!(
                    "No answer to message {} after {} attempts. Attempt reconnecting...",
                    msg_id, N_RETRY
                );
                pending
                    .response_tx
                    .send(Ok(ActionResponse::NoAnswer { msg_id }))
                    .await;
                self.clean_session = false;
                self.state = State::Disconnected;
                return;
            }

            pending.attempts += 1;
            warn!(
                "No answer to message {} yet. Sending it again ({} of {})",
                msg_id, pending.attempts, N_RETRY
            );
            // A failed transmission counts as an attempt
            if let Err(e) = self.transmit(msg_id, &pending.request, true).await {
                warn!("Retransmission of message {} failed: {:?}", msg_id, e);
            }
            pending.deadline = now + T_RETRY;

            // Always succeeds, since the slot was just freed
            let _ = self.msg_id_map.insert(msg_id, pending);
        }
    }

    /// Sends `request` under `msg_id`, flagged as a duplicate when `dup` is set.
    async fn transmit(&mut self, msg_id: u16, request: &Request, dup: bool) -> Result<(), Error> {
        match request {
            Request::Subscription {
                filter,
                quality_of_service,
                ..
            } => {
                self.subscribe(msg_id, filter, dup, *quality_of_service)
                    .await
            }
            // A REGISTER has no DUP flag
            Request::Registration { topic_name } => self.register(msg_id, topic_name).await,
            Request::Publication {
                topic,
                payload,
                quality_of_service,
            } => {
                self.publish(msg_id, *topic, payload, *quality_of_service, dup)
                    .await
            }
        }
    }

    async fn handle_packet(&mut self, packet: Packet<'_>) -> Result<(), Error> {
        match packet {
            Packet::Register {
                header: _,
                register: _,
                topic_name: _,
            } => {
                warn!("Received REGISTER");
            }
            Packet::RegAck { header: _, reg_ack } => {
                self.handle_reg_ack(reg_ack).await?;
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
            Packet::PubAck { header: _, pub_ack } => {
                debug!(
                    "Received PubAck for message {} of topic {}",
                    pub_ack.get_msg_id(),
                    pub_ack.get_topic_id()
                );
                self.handle_pub_ack(&pub_ack).await?;
                // match topic, send PubAck to client
            }
            Packet::SubAck { header: _, sub_ack } => {
                self.handle_sub_ack(sub_ack).await?; //todo: handle error
            }
            Packet::PingReq {
                header: _,
                client_id,
            } => {
                debug!("PingReq {:?}", client_id);
                self.ping_resp().await?;
            }
            Packet::PingResp { header: _ } => {
                debug!("Received PingResp from gateway")
            }
            Packet::Disconnect {
                header: _,
                duration: _,
            } => {
                self.state = State::Disconnected;
            }
            _ => panic!("Can not handle received packet type."),
        }
        Ok(())
    }

    async fn handle_sub_ack(&mut self, sub_ack: SubAck) -> Result<(), Error> {
        let return_code = sub_ack.get_return_code();

        debug!("SubAck {:?}", return_code);

        match return_code {
            ReturnCode::Accepted => {
                info!("providing Quality of Service {:?}", sub_ack.get_qos());

                if let Some(PendingRequest {
                    request:
                        Request::Subscription {
                            filter, message_tx, ..
                        },
                    response_tx,
                    ..
                }) = self.msg_id_map.remove(&sub_ack.get_msg_id())
                {
                    let topic_id = filter.topic_id(sub_ack.get_topic_id());
                    info!(
                        "Topic subscription successful! Received Topic Id {:?}",
                        topic_id
                    );

                    if let Some((i, message_tx_list_entry)) = self
                        .message_tx_list
                        .iter_mut()
                        .enumerate()
                        .find(|(_i, c)| c.is_none())
                    {
                        *message_tx_list_entry = Some(message_tx);
                        // TODO: handle error
                        let _ = self.topic_map.register_subscriber(topic_id, i as u16);

                        response_tx
                            .send(Ok(ActionResponse::TopicInfo {
                                msg_id: sub_ack.get_msg_id(),
                                topic: topic_id,
                            }))
                            .await;
                    } else {
                        error!("no free client subscriber slot");
                        // A subscription by name can only be undone by name, never by its ID
                        let _ = self.unsubscribe(&filter).await;
                        //TODO: send info message to client
                        return Err(Error::NoFreeSubscriberSlot);
                    }
                } else {
                    warn!("No return channel for received msg_id. Ignore SubAck");
                }
            }
            ReturnCode::RejectedCongestion => {
                let msg_id = sub_ack.get_msg_id();
                warn!(
                    "Received congestion warning for message {}. Try again in {} seconds",
                    msg_id,
                    T_WAIT.as_secs()
                );

                if let Some(pending) = self.msg_id_map.remove(&msg_id) {
                    pending
                        .response_tx
                        .send(Ok(ActionResponse::Congestion { msg_id }))
                        .await;
                } else {
                    warn!("No return channel for received msg_id. Ignore SubAck");
                }
            }
            ReturnCode::RejectedInvalidTopicId => return Err(Error::Rejected),
            ReturnCode::RejectedNotSupported => return Err(Error::Rejected),
        }
        Ok(())
    }

    async fn handle_reg_ack(&mut self, reg_ack: RegAck) -> Result<(), Error> {
        let return_code = reg_ack.get_return_code();

        debug!("RegAck {:?}", return_code);

        match return_code {
            ReturnCode::Accepted => {
                let topic_id = TopicId::Normal(reg_ack.get_topic_id());

                info!(
                    "Topic registration successful! Received Topic Id {:?}",
                    topic_id
                );

                if let Some(PendingRequest {
                    request: Request::Registration { .. },
                    response_tx,
                    ..
                }) = self.msg_id_map.remove(&reg_ack.get_msg_id())
                {
                    response_tx
                        .send(Ok(ActionResponse::TopicInfo {
                            msg_id: reg_ack.get_msg_id(),
                            topic: topic_id,
                        }))
                        .await;
                } else {
                    warn!("No return channel for received msg_id. Ignore RegAck");
                }
            }
            ReturnCode::RejectedCongestion => {
                let msg_id = reg_ack.get_msg_id();
                warn!(
                    "received congestion warning for message {}. Try again in {} seconds",
                    msg_id,
                    T_WAIT.as_secs()
                );

                if let Some(pending) = self.msg_id_map.remove(&msg_id) {
                    pending
                        .response_tx
                        .send(Ok(ActionResponse::Congestion { msg_id }))
                        .await;
                } else {
                    warn!("No return channel for received msg_id. Ignore RegAck");
                }
            }
            ReturnCode::RejectedInvalidTopicId => return Err(Error::Rejected),
            ReturnCode::RejectedNotSupported => return Err(Error::Rejected),
        }
        Ok(())
    }

    async fn handle_publish(&mut self, publish: &Publish, data: Payload) -> Result<(), Error> {
        let topic_id = publish.get_topic_id();

        debug!("publish topic={:?}", topic_id);

        let Some(&channel_bitmap) = self.topic_map.get(&topic_id) else {
            error!("Topic in message not found in Topic Map!");
            return self
                .acknowledge(publish, ReturnCode::RejectedInvalidTopicId)
                .await;
        };

        // Each set bit of the bitmap is the index of a subscribed client in the message_tx_list
        let mut channel_bitmap = channel_bitmap;
        let mut delivered_to_all = true;
        while channel_bitmap != 0 {
            let channel_id = channel_bitmap.trailing_zeros() as usize;
            let channel = self.message_tx_list[channel_id].as_ref().unwrap();

            let message = Message {
                topic: topic_id,
                payload: data.clone(),
            };
            if channel.try_send(message).is_err() {
                warn!(
                    "Client {} is busy, message {} not delivered",
                    channel_id,
                    publish.get_msg_id()
                );
                delivered_to_all = false;
            }

            channel_bitmap &= !(1 << channel_id);
        }

        if delivered_to_all {
            self.acknowledge(publish, ReturnCode::Accepted).await
        } else {
            // Without a PUBACK, the gateway retransmits a QoS 1 message
            // A QoS 0 message may be lost by definition
            Ok(())
        }
    }

    /// Acknowledges an incoming PUBLISH, if its QoS asks for it.
    async fn acknowledge(
        &mut self,
        publish: &Publish,
        return_code: ReturnCode,
    ) -> Result<(), Error> {
        match publish.get_qos() {
            QoS::One => {
                self.pub_ack(publish.get_msg_id(), publish.get_topic_id(), return_code)
                    .await
            }
            _ => Ok(()),
        }
    }

    async fn handle_pub_ack(&mut self, pub_ack: &PubAck) -> Result<(), Error> {
        let msg_id = pub_ack.get_msg_id();

        debug!(
            "Received pub_ack for msg_id {} on topic {}",
            msg_id,
            pub_ack.get_topic_id()
        );

        // Only the message ID tells which client published, as several clients may share a topic
        if let Some(PendingRequest {
            request: Request::Publication { .. },
            response_tx,
            ..
        }) = self.msg_id_map.remove(&msg_id)
        {
            response_tx
                .send(Ok(ActionResponse::PubAck {
                    msg_id,
                    return_code: pub_ack.get_return_code(),
                }))
                .await;
        } else {
            warn!("No return channel for received msg_id. Ignore PubAck");
        }
        Ok(())
    }

    pub async fn connect(
        &mut self,
        keep_alive: u16,
        client_id: &[u8],
        will: bool,
        clean_session: bool,
    ) -> Result<(), Error> {
        debug!("connect");

        self.check_state(State::Disconnected)?;
        let mut buf: [u8; MAX_PAYLOAD_SIZE + 32] = [0; MAX_PAYLOAD_SIZE + 32];

        let packet = Packet::connect(keep_alive, clean_session, will, client_id);

        {
            let packet_slice = packet.write_to_buf(&mut buf);
            self.send_packet(packet_slice).await?;
        }

        // wait for connack
        match self
            .transport
            .receive_packet(&mut buf)
            .with_timeout(T_RETRY)
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
                }
                ReturnCode::RejectedCongestion => todo!(),
                ReturnCode::RejectedInvalidTopicId => todo!(),
                ReturnCode::RejectedNotSupported => todo!(),
            },
            _ => {
                error!("Transmission failed!");
                return Err(Error::TransmissionFailed);
            }
        }
        Ok(())
    }

    pub async fn disconnect(&mut self, duration: Option<u16>) -> Result<(), Error> {
        debug!("disconnect");

        self.check_state(State::Active)?;

        let mut buf: [u8; MAX_PAYLOAD_SIZE + 32] = [0; MAX_PAYLOAD_SIZE + 32];

        let packet = Packet::disconnect(duration);
        {
            let packet_slice = packet.write_to_buf(&mut buf);
            self.send_packet(packet_slice).await?;
        }

        self.state = State::Disconnected;
        Ok(())
    }

    pub async fn subscribe(
        &mut self,
        msg_id: u16,
        topic: &TopicFilter,
        dup: bool,
        qos: QoS,
    ) -> Result<(), Error> {
        debug!(
            "send subscribe. state: {:?}. msg_id: {:?}",
            self.state, msg_id
        );
        self.check_state(State::Active)?;

        let mut buf: [u8; MAX_PAYLOAD_SIZE + 32] = [0; MAX_PAYLOAD_SIZE + 32];

        let packet = Packet::subscribe(topic, dup, qos, msg_id);

        {
            let packet_slice = packet.write_to_buf(&mut buf);
            self.send_packet(packet_slice).await?;
        }

        Ok(())
    }

    pub async fn unsubscribe(&mut self, topic: &TopicFilter) -> Result<u16, Error> {
        let msg_id = self.get_next_msg_id();

        let mut buf: [u8; MAX_PAYLOAD_SIZE + 32] = [0; MAX_PAYLOAD_SIZE + 32];

        let packet = Packet::unsubscribe(topic, msg_id);

        {
            let packet_slice = packet.write_to_buf(&mut buf);
            self.send_packet(packet_slice).await?;
        }

        Ok(msg_id)
    }

    pub async fn register(&mut self, msg_id: u16, topic_name: &str) -> Result<(), Error> {
        debug!(
            "send register. state: {:?}. msg_id: {:?}",
            self.state, msg_id
        );
        self.check_state(State::Active)?;

        let mut buf: [u8; MAX_PAYLOAD_SIZE + 32] = [0; MAX_PAYLOAD_SIZE + 32];

        let packet = Packet::register(topic_name, msg_id);

        {
            let packet_slice = packet.write_to_buf(&mut buf);
            self.send_packet(packet_slice).await?;
        }

        Ok(())
    }

    pub async fn publish(
        &mut self,
        msg_id: u16,
        topic: TopicId,
        payload: &[u8],
        quality_of_service: QoS,
        dup: bool,
    ) -> Result<(), Error> {
        self.check_state(State::Active)?;

        let mut buf: [u8; MAX_PAYLOAD_SIZE + 32] = [0; MAX_PAYLOAD_SIZE + 32];

        let packet = Packet::publish(msg_id, topic, quality_of_service, dup, payload);

        {
            let packet_slice = packet.write_to_buf(&mut buf);
            trace!("Publish Bytes: {:?}", packet_slice);
            self.send_packet(packet_slice).await?;
        }
        Ok(())
    }

    pub async fn pub_ack(
        &mut self,
        msg_id: u16,
        topic: TopicId,
        return_code: ReturnCode,
    ) -> Result<(), Error> {
        self.check_state(State::Active)?;

        // fixed packet size
        let mut buf: [u8; 56] = [0; 56];

        let packet = Packet::pub_ack(msg_id, topic, return_code);

        {
            let packet_slice = packet.write_to_buf(&mut buf);
            debug!("PubAck Bytes: {:?}", packet_slice);
            self.send_packet(packet_slice).await?;
        }
        Ok(())
    }

    async fn ping_resp(&mut self) -> Result<(), Error> {
        // fixed packet size
        let mut buf: [u8; 16] = [0; 16];

        let packet = Packet::ping_resp();

        {
            let packet_slice = packet.write_to_buf(&mut buf);
            self.send_packet(packet_slice).await?;
        }
        Ok(())
    }

    async fn ping_req(&mut self) -> Result<(), Error> {
        let mut buf: [u8; MAX_PAYLOAD_SIZE + 32] = [0; MAX_PAYLOAD_SIZE + 32];

        let packet = Packet::ping_req(self.client_id);
        {
            let packet_slice = packet.write_to_buf(&mut buf);
            self.send_packet(packet_slice).await?;
        }
        Ok(())
    }

    async fn send_packet(&mut self, buf: &[u8]) -> Result<(), Error> {
        match self.transport.send(&buf).await {
            Ok(_) => Ok(()),
            Err(_) => Err(Error::TransmissionFailed),
        }
    }

    fn check_state(&self, state: State) -> Result<(), Error> {
        if self.state != state {
            return Err(Error::InvalidState);
        }
        Ok(())
    }

    fn get_next_msg_id(&mut self) -> u16 {
        loop {
            let msg_id = self.msg_id;
            self.msg_id = self.msg_id.wrapping_add(1);

            if msg_id != 0x0000 && !self.msg_id_map.contains_key(&msg_id) {
                return msg_id;
            }
        }
    }
}
