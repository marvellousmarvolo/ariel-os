use crate::{
    N_RETRY, T_RETRY, T_WAIT,
    error::Error,
    serialization::{flags::QoS, message_variable_part::ReturnCode},
};
use ariel_os_debug_log::*;
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{Channel, Sender},
};
use embassy_time::{TimeoutError, Timer, WithTimeout};
use heapless::{String, Vec};

pub const MAX_PAYLOAD_SIZE: usize = 1024; // !usize_from_env_or()
pub const MAX_TOPIC_LENGTH: usize = 64; // !usize_from_env_or()

pub static ACTION_REQUEST_CHANNEL: ActionRequestChannel<'_> = Channel::new();

type ActionReply = Result<ActionResponse, Error>;

pub type Payload = heapless::Vec<u8, MAX_PAYLOAD_SIZE>;
pub type ActionRequestChannel<'ch> = Channel<CriticalSectionRawMutex, ActionRequest<'ch>, 1>;
pub type ActionReplyChannel = Channel<CriticalSectionRawMutex, ActionReply, 1>;
pub type ActionReplySender<'a> = Sender<'a, CriticalSectionRawMutex, ActionReply, 1>;
pub type MessageChannel = Channel<CriticalSectionRawMutex, Message, 1>;
pub type MessageSender = Sender<'static, CriticalSectionRawMutex, Message, 1>;

#[derive(Clone)]
pub struct ActionRequest<'ch> {
    pub action: Action,
    pub response_tx: ActionReplySender<'ch>,
}

pub enum ActionResponse {
    Ok,
    Subscription { msg_id: u16 },
    Registration { msg_id: u16 },
}

#[derive(Clone)]
pub enum Action {
    Subscribe {
        topic: Topic,
        message_tx: MessageSender,
        quality_of_service: QoS,
    },
    Register {
        topic: Topic,
        message_tx: MessageSender,
    },
    Publish {
        topic: Topic,
        payload: Payload,
        quality_of_service: QoS,
    },
    PubAck {
        topic: Topic,
        msg_id: u16,
        return_code: ReturnCode,
    },
    Disconnect {
        duration: Option<u16>,
    },
}

#[derive(Clone)]
pub enum Message {
    Publish {
        msg_id: u16,
        topic: u16,
        payload: Payload,
    },
    TopicInfo {
        msg_id: u16,
        topic: u16,
    },
    Congestion {
        msg_id: u16,
        topic: u16,
    },
    PubAck {
        msg_id: u16,
        topic: u16,
        return_code: ReturnCode,
    },
}

impl Message {
    pub fn get_topic(&self) -> Topic {
        match self {
            Message::Publish {
                msg_id: _, topic, ..
            } => Topic::from_id(*topic),
            Message::TopicInfo { msg_id: _, topic } => Topic::from_id(*topic),
            Message::Congestion { msg_id: _, topic } => Topic::from_id(*topic),
            Message::PubAck {
                msg_id: _, topic, ..
            } => Topic::from_id(*topic),
        }
    }

    pub fn get_msg_id(&self) -> u16 {
        match self {
            Message::Publish { msg_id, .. } => *msg_id,
            Message::TopicInfo { msg_id, .. } => *msg_id,
            Message::Congestion { msg_id, .. } => *msg_id,
            Message::PubAck { msg_id, .. } => *msg_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
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

    pub fn to_buf(&self, buf: &mut [u8]) {
        match self {
            Self::Id(id) => buf[..2].copy_from_slice(&id.to_be_bytes()),
            Self::ShortName(short_name) => buf[..2].copy_from_slice(short_name),
            Self::LongName(long_name) => {
                (&mut buf[..long_name.len()]).copy_from_slice(long_name.as_bytes())
            }
        }
    }
}

pub struct Client {
    quality_of_service: QoS,
    action_response_channel: ActionReplyChannel,
    message_channel: MessageChannel,
}

impl Client {
    pub const fn new(quality_of_service: QoS) -> Self {
        Self {
            quality_of_service,
            action_response_channel: ActionReplyChannel::new(),
            message_channel: MessageChannel::new(),
        }
    }

    pub const fn default() -> Self {
        Self {
            quality_of_service: QoS::Zero,
            action_response_channel: ActionReplyChannel::new(),
            message_channel: MessageChannel::new(),
        }
    }

    pub async fn subscribe(&'static self, topic: Topic) -> Result<u16, Error> {
        loop {
            ACTION_REQUEST_CHANNEL
                .send(ActionRequest {
                    action: Action::Subscribe {
                        topic: topic.clone(),
                        message_tx: self.message_channel.sender(),
                        quality_of_service: self.quality_of_service.clone(),
                    },
                    response_tx: self.action_response_channel.sender(),
                })
                .await;

            if let ActionResponse::Subscription { msg_id: msgid } =
                self.action_response_channel.receive().await?
            {
                info!("got subscribe result msgid: {}", msgid);
                loop {
                    match self.receive().await {
                        Message::TopicInfo { msg_id: _, topic } => {
                            info!("got msg_id {} -> topic_id {}", msgid, topic);
                            return Ok(topic);
                        }
                        Message::Publish {
                            msg_id: _,
                            topic,
                            payload: _,
                        } => {
                            // drop messages during subscription/registration process
                            info!("dropped message for topic_id {}", topic);
                        }
                        Message::Congestion { .. } => {
                            Timer::after(T_WAIT).await;
                            break;
                        }
                        Message::PubAck {
                            msg_id: _,
                            topic,
                            return_code: _,
                        } => {
                            info!("dropped message for topic_id {}", topic);
                        }
                    }
                }
            } else {
                unreachable!()
            }
        }
    }

    pub async fn register(&'static self, topic: Topic) -> Result<u16, Error> {
        loop {
            ACTION_REQUEST_CHANNEL
                .send(ActionRequest {
                    action: Action::Register {
                        topic: topic.clone(),
                        message_tx: self.message_channel.sender(),
                    },
                    response_tx: self.action_response_channel.sender(),
                })
                .await;

            if let ActionResponse::Registration { msg_id: msgid } =
                self.action_response_channel.receive().await?
            {
                info!("got registration result msgid: {}", msgid);
                loop {
                    match self.receive().await {
                        Message::TopicInfo { msg_id: _, topic } => {
                            info!("got msg_id {} -> topic_id {}", msgid, topic);
                            return Ok(topic);
                        }
                        Message::Publish {
                            msg_id: _,
                            topic,
                            payload: _,
                        } => {
                            // drop messages during subscription/registration process
                            info!("dropped message for topic_id {}", topic);
                        }
                        Message::Congestion { .. } => {
                            Timer::after(T_WAIT).await;
                            break;
                        }
                        Message::PubAck {
                            msg_id: _,
                            topic,
                            return_code: _,
                        } => {
                            info!("dropped message for topic_id {}", topic);
                        }
                    }
                }
            } else {
                unreachable!()
            }
        }
    }

    pub async fn publish(&'static self, topic: Topic, payload: &[u8]) -> Result<(), Error> {
        if payload.len() > MAX_PAYLOAD_SIZE {
            return Err(Error::PayloadTooBig);
        }
        let payload_vec: Vec<u8, MAX_PAYLOAD_SIZE> = Vec::from_slice(payload).unwrap();

        for i in 1..=N_RETRY {
            ACTION_REQUEST_CHANNEL
                .send(ActionRequest {
                    action: Action::Publish {
                        topic: topic.clone(),
                        payload: payload_vec.clone(),
                        quality_of_service: self.quality_of_service.clone(),
                    },
                    response_tx: self.action_response_channel.sender(),
                })
                .await;

            let _ = self.action_response_channel.receive().await;

            if self.quality_of_service == QoS::One {
                match self.message_channel.receive().with_timeout(T_RETRY).await {
                    Ok(msg) => match msg {
                        Message::PubAck {
                            msg_id: _,
                            topic,
                            return_code: _,
                        } => {
                            info!("got PubAck for topic id {}", topic);
                            return Ok(());
                        }
                        _ => {
                            info!("Message is no PubAck, ignore");
                            continue;
                        }
                    },
                    Err(TimeoutError) => {
                        info!("Publish timed out {} out of {} times.", i, N_RETRY);
                        // todo reconnect on i = N_RETRY
                        continue;
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn disconnect(&'static self) -> Result<(), Error> {
        ACTION_REQUEST_CHANNEL
            .send(ActionRequest {
                action: Action::Disconnect { duration: None },
                response_tx: self.action_response_channel.sender(),
            })
            .await;

        let _ = self.action_response_channel.receive().await;
        Ok(())
    }

    pub async fn receive(&'static self) -> Message {
        let msg = self.message_channel.receive().await;

        if self.quality_of_service == QoS::One {
            ACTION_REQUEST_CHANNEL
                .send(ActionRequest {
                    action: Action::PubAck {
                        topic: msg.clone().get_topic(),
                        msg_id: msg.clone().get_msg_id(),
                        return_code: ReturnCode::Accepted,
                    },
                    response_tx: self.action_response_channel.sender(),
                })
                .await;

            let _ = self.action_response_channel.receive().await;
        }
        msg
    }
}
