use crate::{
    T_WAIT,
    error::Error,
    serialization::{flags::QoS, message_variable_part::ReturnCode},
    topic::{TopicFilter, TopicId},
};
use ariel_os_debug_log::*;
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{Channel, Sender},
};
use embassy_time::Timer;
use heapless::{String, Vec};

pub const MAX_PAYLOAD_SIZE: usize = 1024; // !usize_from_env_or()
pub const MAX_TOPIC_LENGTH: usize = 64; // !usize_from_env_or()

pub static ACTION_REQUEST_CHANNEL: ActionRequestChannel = Channel::new();

type ActionReply = Result<ActionResponse, Error>;

pub type Payload = heapless::Vec<u8, MAX_PAYLOAD_SIZE>;
pub type ActionRequestChannel = Channel<CriticalSectionRawMutex, ActionRequest, 1>;
pub type ActionReplyChannel = Channel<CriticalSectionRawMutex, ActionReply, 1>;
pub type ActionReplySender = Sender<'static, CriticalSectionRawMutex, ActionReply, 1>;
pub type MessageChannel = Channel<CriticalSectionRawMutex, Message, 1>;
pub type MessageSender = Sender<'static, CriticalSectionRawMutex, Message, 1>;

#[derive(Clone)]
pub struct ActionRequest {
    pub action: Action,
    pub response_tx: ActionReplySender,
}

pub enum ActionResponse {
    Ok,
    Pending {
        msg_id: u16,
    },
    TopicInfo {
        msg_id: u16,
        topic: TopicId,
    },
    Congestion {
        msg_id: u16,
    },
    PubAck {
        msg_id: u16,
        return_code: ReturnCode,
    },
    NoAnswer {
        msg_id: u16,
    },
}

impl ActionResponse {
    /// Returns the ID of the gateway message to be answered
    fn answered_msg_id(&self) -> Option<u16> {
        match self {
            Self::TopicInfo { msg_id, .. }
            | Self::Congestion { msg_id }
            | Self::PubAck { msg_id, .. }
            | Self::NoAnswer { msg_id } => Some(*msg_id),
            Self::Ok | Self::Pending { .. } => None,
        }
    }
}

#[derive(Clone)]
pub enum Action {
    Subscribe {
        topic: TopicFilter,
        message_tx: MessageSender,
        quality_of_service: QoS,
    },
    Register {
        topic_name: String<MAX_TOPIC_LENGTH>,
    },
    Publish {
        topic: TopicId,
        payload: Payload,
        quality_of_service: QoS,
    },
    Disconnect {
        duration: Option<u16>,
    },
}

#[derive(Clone)]
pub struct Message {
    pub topic: TopicId,
    pub payload: Payload,
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

    pub async fn subscribe(&'static self, topic: TopicFilter) -> Result<TopicId, Error> {
        let subscribe = Action::Subscribe {
            topic,
            message_tx: self.message_channel.sender(),
            quality_of_service: self.quality_of_service,
        };

        match self.exchange(subscribe).await? {
            ActionResponse::TopicInfo { msg_id, topic } => {
                debug!(
                    "Subscription: got msg_id {} -> topic_id {:?}",
                    msg_id, topic
                );
                Ok(topic)
            }
            _ => unreachable!("a SUBSCRIBE is only ever answered by a SUBACK"),
        }
    }

    /// Registers the full `topic_name`, and returns the normal topic ID assigned by the gateway.
    pub async fn register(&'static self, topic_name: &str) -> Result<TopicId, Error> {
        let topic_name: String<MAX_TOPIC_LENGTH> =
            String::try_from(topic_name).map_err(|_| Error::TopicTooLong)?;

        match self.exchange(Action::Register { topic_name }).await? {
            ActionResponse::TopicInfo { msg_id, topic } => {
                debug!(
                    "Registration: got msg_id {} -> topic_id {:?}",
                    msg_id, topic
                );
                Ok(topic)
            }
            _ => unreachable!("a REGISTER is only ever answered by a REGACK"),
        }
    }

    pub async fn publish(&'static self, topic: TopicId, payload: &[u8]) -> Result<(), Error> {
        let publish = Action::Publish {
            topic,
            payload: Vec::from_slice(payload).map_err(|_| Error::PayloadTooBig)?,
            quality_of_service: self.quality_of_service,
        };

        if self.quality_of_service != QoS::One {
            self.request(publish).await?;
            return Ok(());
        }

        match self.exchange(publish).await? {
            ActionResponse::PubAck { msg_id, .. } => {
                info!("got PubAck for message {}", msg_id);
                Ok(())
            }
            _ => unreachable!("a QoS 1 PUBLISH is only ever answered by a PUBACK"),
        }
    }

    pub async fn disconnect(&'static self) -> Result<(), Error> {
        self.request(Action::Disconnect { duration: None }).await?;
        Ok(())
    }

    /// Waits for the next message published on one of the topics this client has subscribed to
    pub async fn receive(&'static self) -> Message {
        self.message_channel.receive().await
    }

    /// Sends `action` to the connection task, and returns its immediate answer
    async fn request(&'static self, action: Action) -> ActionReply {
        ACTION_REQUEST_CHANNEL
            .send(ActionRequest {
                action,
                response_tx: self.action_response_channel.sender(),
            })
            .await;

        loop {
            match self.action_response_channel.receive().await {
                // An answer to an earlier request, which arrived after its caller gave up
                Ok(late) if late.answered_msg_id().is_some() => debug!("dropped late answer"),
                reply => return reply,
            }
        }
    }

    /// Sends an `action` that the gateway answers, and returns its answer.
    async fn exchange(&'static self, action: Action) -> ActionReply {
        loop {
            let msg_id = self.request_pending(action.clone()).await?;

            match self.answer(msg_id).await {
                ActionResponse::Congestion { .. } => {
                    warn!(
                        "Congestion occuring, trying again in {} seconds",
                        T_WAIT.as_secs()
                    );
                    Timer::after(T_WAIT).await;
                }
                ActionResponse::NoAnswer { .. } => {
                    warn!("No answer to message {}, trying again", msg_id);
                }
                answer => return Ok(answer),
            }
        }
    }

    /// Returns the message ID for an `action` to await.
    async fn request_pending(&'static self, action: Action) -> Result<u16, Error> {
        match self.request(action).await? {
            ActionResponse::Pending { msg_id } => Ok(msg_id),
            _ => unreachable!("a request to the gateway is always answered as pending first"),
        }
    }

    /// Waits for the gateway's answer to a message
    async fn answer(&'static self, msg_id: u16) -> ActionResponse {
        loop {
            match self.action_response_channel.receive().await {
                Ok(answer) if answer.answered_msg_id() == Some(msg_id) => return answer,
                _ => debug!("dropped late answer"),
            }
        }
    }
}
