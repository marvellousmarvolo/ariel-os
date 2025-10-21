#![no_main]
#![no_std]

mod pins;
use ariel_os::{
    debug::log::*,
    gpio::{Level, Output},
};
use ariel_os_mqttsn_async::{Message, MqttClient, Topic};

#[ariel_os::task(autostart)]
async fn mqtt_sn_test() {
    static MY_CLIENT: MqttClient = MqttClient::new();

    ariel_os::asynch::spawner()
        .spawn(ariel_os_mqttsn_async::client())
        .unwrap();

    info!("mqtt_sn_test()");

    let topic_id = MY_CLIENT
        .subscribe(Topic::from_long("my_topic"))
        .await
        .unwrap();
    info!("topic_id: {}", topic_id);

    loop {
        let msg = MY_CLIENT.receive().await;
        match msg {
            Message::Publish { topic, payload } => {
                if let Ok(utf8) = str::from_utf8(&payload) {
                    info!("got message for topic_id {}: \"{}\"", topic, utf8);
                } else {
                    info!("got message for topic_id {} (non-utf8)", topic)
                }
            }
            Message::TopicIs { msgid, topic_id } => {
                info!(
                    "CLIENT UNEXPECTED: got msg_id {} -> topic_id {}",
                    msgid, topic_id
                )
            }
        }
    }

    info!("mqtt_sn_test() end");
}

#[ariel_os::task(autostart)]
async fn mqtt_sn_test2() {
    static MY_CLIENT: MqttClient = MqttClient::new();

    info!("mqtt_sn_test2()");

    let topic_id = MY_CLIENT
        .subscribe(Topic::from_long("my_topic"))
        .await
        .unwrap();
    info!("other topic_id: {}", topic_id);

    loop {
        let msg = MY_CLIENT.receive().await;
        match msg {
            Message::Publish { topic, payload } => {
                if let Ok(utf8) = str::from_utf8(&payload) {
                    info!("other got message for topic_id {}: \"{}\"", topic, utf8);
                } else {
                    info!("other got message for topic_id {} (non-utf8)", topic)
                }
            }
            Message::TopicIs { msgid, topic_id } => {
                info!(
                    "CLIENT UNEXPECTED: got msg_id {} -> topic_id {}",
                    msgid, topic_id
                )
            }
        }
    }

    info!("mqtt_sn_test() end");
}
