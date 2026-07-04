#![no_main]
#![no_std]

mod pins;

use ariel_os::{
    config::str_from_env_or,
    debug::log::*,
    gpio::{Input, Level, Output, Pull},
    mqttsn::{
        client::{Client, Message, Topic},
        serialization::flags::QoS,
        settings::Settings,
    },
    time::{Duration, Instant, Timer},
};

#[ariel_os::task(autostart, peripherals)]
async fn mqtt_sn_performance_sender(peripherals: pins::Peripherals) {
    static QOS: QoS = QoS::One;

    let mut pin = Output::builder(peripherals.signal_pin, Level::High).build();

    info!("mqtt_sn_performance_sender()");

    ariel_os::asynch::spawner()
        .spawn(ariel_os::mqttsn::start(
            Settings::builder()
                .client_id("ariel_0")
                // .keepalive(60)
                .build(),
        ))
        .unwrap();

    static CLIENT_SEND: Client = Client::new(QOS);
    let topic_id_send = CLIENT_SEND
        .register(Topic::from_long("perf_b"))
        .await
        .unwrap();

    static CLIENT_RECV: Client = Client::new(QOS);
    let topic_id_recv = CLIENT_RECV
        .subscribe(Topic::from_long("perf_a"))
        .await
        .unwrap();

    let max_iterations = 1000;
    info!("10 seconds to start...");

    Timer::after_secs(10).await;

    for i in 0..max_iterations {
        pin.toggle();
        info!("Start!");

        let t_start = Instant::now().as_micros();
        let payload = t_start.to_be_bytes();
        let _ = CLIENT_SEND
            .publish(Topic::from_id(topic_id_send), &payload)
            .await;

        let msg = CLIENT_RECV.receive().await;
        let t_roundtrip = Instant::now().as_micros() - t_start;
        info!("Iteration {}", i);
        match msg {
            Message::Publish {
                msg_id: _,
                topic: _,
                payload,
            } => {
                let t_delta = u64::from_be_bytes(payload.into_array().unwrap());
                info!("  One-way took {} µs", t_delta);
            }
            _ => {
                info!("  MESSAGE UNEXPECTED");
            }
        }
        info!("  Roundtrip took {} µs", t_roundtrip);
        Timer::after_secs(1).await;
    }
    info!("BENCHMARK DONE");
}
