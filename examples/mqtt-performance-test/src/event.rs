use ariel_os::debug::log::defmt;
use mountain_mqtt::{client::EventHandlerError, packets::publish::ApplicationMessage};
use mountain_mqtt_arielos::mqtt_manager::FromApplicationMessage;

pub const TOPIC_RECV: &str = "perf_a"; // send
// pub const TOPIC_RECV: &str = "perf_b"; // recv

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, defmt::Format)]
pub enum Event {
    Message(u64),
}

impl<const P: usize> FromApplicationMessage<P> for Event {
    fn from_application_message(
        message: &ApplicationMessage<'_, P>,
    ) -> Result<Self, EventHandlerError> {
        let received = match message.topic_name {
            TOPIC_RECV => Ok(Self::Message(u64::from_be_bytes(
                message.payload.try_into().unwrap(),
            ))),
            _ => Err(EventHandlerError::UnexpectedApplicationMessageTopic),
        }?;

        Ok(received)
    }
}
