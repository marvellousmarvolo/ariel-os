#![no_main]
#![no_std]

mod pins;
use ariel_os::{
    debug::log::*,
    gpio::{Level, Output},
};
use ariel_os_mqttsn_async::{MqttClient, Topic};

#[ariel_os::task(autostart)]
async fn mqtt_sn_test() {
    static MY_CLIENT: MqttClient = MqttClient::new();

    ariel_os::asynch::spawner()
        .spawn(ariel_os_mqttsn_async::client())
        .unwrap();

    info!("mqtt_sn_test()");

    match MY_CLIENT.subscribe(Topic::from_long("my_topic")).await {
        Ok(_) => info!("response: Ok"),
        Err(_) => info!("response: Err"),
    };

    info!("mqtt_sn_test() end");
}

#[ariel_os::task(autostart)]
async fn mqtt_sn_test2() {
    static MY_CLIENT: MqttClient = MqttClient::new();

    info!("mqtt_sn_test2()");

    match MY_CLIENT.subscribe(Topic::from_long("my_other_topic")).await {
        Ok(_) => info!("response: Ok"),
        Err(_) => info!("response: Err"),
    };

    info!("mqtt_sn_test2() end");
}