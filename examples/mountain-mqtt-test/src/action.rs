use mountain_mqtt::{
    client::{Client, ClientError},
    data::quality_of_service::QualityOfService,
    mqtt_manager::{ConnectionId, MqttOperations},
};

pub const TOPIC_BUTTON: &str = "mountain-mqtt-example-button";

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Action {
    Button(bool),
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
            Action::Button(pressed) => {
                let payload = if *pressed { "true" } else { "false" };
                client
                    .publish(
                        TOPIC_BUTTON,
                        payload.as_bytes(),
                        QualityOfService::Qos1,
                        false,
                    )
                    .await?;
            }
        }
        Ok(())
    }
}
