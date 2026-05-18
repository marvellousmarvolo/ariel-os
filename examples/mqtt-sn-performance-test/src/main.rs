#![no_main]
#![no_std]

mod pins;

use ariel_os::{
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

    let mut btn = Input::builder(peripherals.btn1, Pull::Up)
        .build_with_interrupt()
        .unwrap();

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

    let mut loop_count = 0;
    loop {
        info!("Press Button to start testing!");
        let _ = btn.wait_for_low().await;
        info!("Start!");

        let t_start = Instant::now().as_micros();
        let payload = t_start.to_be_bytes();
        let _ = CLIENT_SEND
            .publish(Topic::from_id(topic_id_send), &payload)
            .await;
        let msg = CLIENT_RECV.receive().await;
        let t_roundtrip = Instant::now().as_micros() - t_start;
        info!("Iteration {}", loop_count);
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
        loop_count += 1;
        Timer::after_secs(1).await;
    }
}

// #[ariel_os::task(autostart, peripherals)]
// async fn mqtt_sn_performance_receiver(peripherals: pins::Peripherals) {
//     static QOS: QoS = QoS::One;

//     let mut pin = Input::builder(peripherals.signal_pin, Pull::Up)
//         .build_with_interrupt()
//         .unwrap();

//     info!("mqtt_sn_performance_receiver()");

//     ariel_os::asynch::spawner()
//         .spawn(ariel_os::mqttsn::start(
//             Settings::builder()
//                 .client_id("ariel_1")
//                 // .keepalive(60)
//                 .build(),
//         ))
//         .unwrap();

//     static CLIENT_SEND: Client = Client::new(QOS);
//     let topic_id_send = CLIENT_SEND
//         .register(Topic::from_long("perf_a"))
//         .await
//         .unwrap();

//     static CLIENT_RECV: Client = Client::new(QOS);
//     let topic_id_recv = CLIENT_RECV
//         .subscribe(Topic::from_long("perf_b"))
//         .await
//         .unwrap();

//     let mut loop_count = 0;
//     loop {
//         info!("Wait for start signal...");
//         pin.wait_for_low().await;
//         info!("Start!");

//         let t_local = Instant::now().as_micros();
//         let msg = CLIENT_RECV.receive().await;
//         let t_delta = Instant::now().as_micros() - t_local;
//         info!("Iteration {}", loop_count);
//         info!("  One-way took {} µs", t_delta);
//         match msg {
//             Message::Publish {
//                 msg_id: _,
//                 topic,
//                 payload: _,
//             } => {
//                 // let t_start = u64::from_be_bytes(payload.into_array().unwrap());
//                 let _ = CLIENT_SEND
//                     .publish(Topic::from_id(topic_id_send), &t_delta.to_be_bytes())
//                     .await;
//             }
//             _ => {
//                 info!("  MESSAGE UNEXPECTED");
//             }
//         }
//         loop_count += 1;
//         Timer::after_secs(1).await;
//     }
// }
