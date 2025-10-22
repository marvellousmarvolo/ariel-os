use crate::error::Error;
use ariel_os::debug::log::*;
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{Channel, Sender},
};
use heapless::String;

pub const MAX_PAYLOAD_SIZE: usize = 1024; // !usize_from_env_or()
pub const MAX_TOPIC_LENGTH: usize = 64; // !usize_from_env_or()

pub static ACTION_REQUEST_CHANNEL: ActionRequestChannel<'_> = Channel::new();

type ActionReply = Result<ActionResult, Error>;

pub type Payload = heapless::Vec<u8, MAX_PAYLOAD_SIZE>;
pub type ActionRequestChannel<'ch> = Channel<CriticalSectionRawMutex, ActionRequest<'ch>, 1>;
pub type ActionReplyChannel = Channel<CriticalSectionRawMutex, ActionReply, 1>;
pub type ActionReplySender<'a> = Sender<'a, CriticalSectionRawMutex, ActionReply, 1>;
pub type MessageChannel = Channel<CriticalSectionRawMutex, Message, 1>;
pub type MessageSender = Sender<'static, CriticalSectionRawMutex, Message, 1>;

#[derive(Clone)]
pub struct ActionRequest<'ch> {
    pub action: Action,
    pub reply_tx: ActionReplySender<'ch>,
}

pub enum ActionResult {
    Ok,
    Subscription { msgid: u16 },
}

#[derive(Clone)]
pub enum Action {
    Subscribe {
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
    TopicIs { msgid: u16, topic_id: u16 },
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
    action_reply_channel: ActionReplyChannel,
    message_channel: MessageChannel,
}

impl Client {
    pub const fn new() -> Self {
        Self {
            action_reply_channel: ActionReplyChannel::new(),
            message_channel: MessageChannel::new(),
        }
    }

    pub async fn subscribe(&'static self, topic: Topic) -> Result<u16, Error> {
        ACTION_REQUEST_CHANNEL
            .send(ActionRequest {
                action: Action::Subscribe {
                    topic,
                    message_tx: self.message_channel.sender(),
                },
                reply_tx: self.action_reply_channel.sender(), // obsolete? Can be represented by message_channel
            })
            .await;

        if let ActionResult::Subscription { msgid } = self.action_reply_channel.receive().await? {
            info!("got subscribe result msgid: {}", msgid);
            loop {
                match self.receive().await {
                    Message::Publish { topic, payload: _ } => {
                        info!("dropped message for topic_id {}", topic)
                    }
                    Message::TopicIs { msgid, topic_id } => {
                        info!("got msg_id {} -> topic_id {}", msgid, topic_id);
                        return Ok(topic_id);
                    }
                }
            }
        } else {
            unreachable!()
        }
    }

    pub async fn receive(&'static self) -> Message {
        self.message_channel.receive().await
    }
}
