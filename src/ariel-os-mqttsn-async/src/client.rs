use crate::{T_WAIT, error::Error};
use ariel_os::time::Timer;
use ariel_os_debug::log::*;
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{Channel, Sender},
};
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
    },
    Register {
        topic: Topic,
        message_tx: MessageSender,
    },
    Publish {
        topic: Topic,
        payload: Payload,
    },
}

#[derive(Clone)]
pub enum Message {
    Publish { topic: u16, payload: Payload },
    TopicInfo { msgid: u16, topic_id: u16 },
    Congestion,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
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
    action_response_channel: ActionReplyChannel,
    message_channel: MessageChannel,
}

impl Client {
    pub const fn new() -> Self {
        Self {
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
                        Message::TopicInfo { msgid, topic_id } => {
                            info!("got msg_id {} -> topic_id {}", msgid, topic_id);
                            return Ok(topic_id);
                        }
                        Message::Publish { topic, payload: _ } => {
                            // drop messages during subscription/registration process
                            info!("dropped message for topic_id {}", topic);
                        }
                        Message::Congestion => {
                            Timer::after(T_WAIT).await;
                            break;
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
                        Message::TopicInfo { msgid, topic_id } => {
                            info!("got msg_id {} -> topic_id {}", msgid, topic_id);
                            return Ok(topic_id);
                        }
                        Message::Publish { topic, payload: _ } => {
                            // drop messages during subscription/registration process
                            info!("dropped message for topic_id {}", topic)
                        }
                        Message::Congestion => {
                            Timer::after(T_WAIT).await;
                            break;
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

        ACTION_REQUEST_CHANNEL
            .send(ActionRequest {
                action: Action::Publish {
                    topic,
                    payload: payload_vec,
                },
                response_tx: self.action_response_channel.sender(),
            })
            .await;
        let _ = self.action_response_channel.receive().await;
        Ok(())
    }

    pub async fn receive(&'static self) -> Message {
        self.message_channel.receive().await
    }
}
