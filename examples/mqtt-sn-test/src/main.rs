#![no_main]
#![no_std]

use ariel_os::{
    debug::log::*,
    gpio::{Input, Pull},
    mqttsn::{
        client::{Client, Message},
        serialization::flags::QoS,
        settings::Settings,
        topic::TopicFilter,
    },
    time::Timer,
};
use ariel_os_boards::pins;

const SEND_TOPIC: &str = "ariel/test/out";
const RECEIVE_TOPIC: &str = "ariel/test/in";
const PAYLOAD: &[u8] = b"Hello from Ariel OS!";

static SEND_CLIENT: Client = Client::new(QoS::Zero);
static RECEIVE_CLIENT: Client = Client::new(QoS::Zero);

#[ariel_os::task(autostart, peripherals)]
async fn mqtt_sn_test(buttons: pins::ButtonPeripherals) {
    info!("mqtt_sn_test()");

    let spawner = ariel_os::asynch::spawner();
    spawner
        .spawn(ariel_os::mqttsn::start(
            Settings::builder().client_id("ariel_test").build(),
        ))
        .unwrap();

    let send_topic = SEND_CLIENT.register(SEND_TOPIC).await.unwrap();
    info!("Publishing on {} as {:?}", SEND_TOPIC, send_topic);

    let receive_topic = RECEIVE_CLIENT
        .subscribe(TopicFilter::name(RECEIVE_TOPIC).unwrap())
        .await
        .unwrap();
    info!("Listening to {} as {:?}", RECEIVE_TOPIC, receive_topic);

    spawner.spawn(log_messages()).unwrap();

    let mut button = Input::builder(buttons.button0, Pull::Up)
        .build_with_interrupt()
        .unwrap();

    info!("Press button to publish!");
    loop {
        button.wait_for_falling_edge().await;

        match SEND_CLIENT.publish(send_topic, PAYLOAD).await {
            Ok(()) => info!("Published on {}", SEND_TOPIC),
            Err(e) => warn!("Publishing failed: {:?}", e),
        }

        Timer::after_millis(200).await;
        button.wait_for_high().await;
    }
}

#[ariel_os::task()]
async fn log_messages() {
    loop {
        let Message { topic, payload } = RECEIVE_CLIENT.receive().await;

        match str::from_utf8(&payload) {
            Ok(text) => info!("Received on {:?}: \"{}\"", topic, text),
            Err(_) => info!("Received on {:?}: {:?}", topic, payload.as_slice()),
        }
    }
}
