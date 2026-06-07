use ariel_os::{
    debug::log::*,
    mountain_mqtt::{
        client::{Client, ClientError},
        data::quality_of_service::QualityOfService,
        mqtt_manager::{ConnectionId, MqttOperations},
    },
};

pub const TOPIC_SEND: &str = "perf_b"; // send
// pub const TOPIC_SEND: &str = "perf_a"; // recv

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, defmt::Format)]
pub enum Action {
    Publish(u64),
}

impl MqttOperations for Action {
    async fn perform<'a, 'b, C>(
        &'b mut self,
        client: &mut C,
        _client_id: &'a str,
        _connection_id: ConnectionId,
        _is_retry: bool,
    ) -> Result<(), ClientError>
    where
        C: Client<'a>,
    {
        match self {
            Action::Publish(timestamp) => {
                client
                    .publish(
                        TOPIC_SEND,
                        &timestamp.to_be_bytes(),
                        QualityOfService::Qos0,
                        false,
                    )
                    .await?;
            }
        }
        Ok(())
    }
}
