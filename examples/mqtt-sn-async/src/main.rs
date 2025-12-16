#![no_main]
#![no_std]

mod pins;
use ariel_os::{
    debug::log::*,
    gpio::{Level, Output},
    time::{Duration, Timer},
};
use ariel_os_mqttsn_async::{
    client::{Client, Message, Topic},
    settings::Settings,
};

// #[ariel_os::task(autostart)]
// async fn mqtt_sn_test1() {
//     static MY_CLIENT: Client = Client::new();

//     ariel_os::asynch::spawner()
//         .spawn(ariel_os_mqttsn_async::start())
//         .unwrap();

//     info!("mqtt_sn_test()");

//     let topic_id = MY_CLIENT
//         .subscribe(Topic::from_long("my_topic"))
//         .await
//         .unwrap();
//     info!("topic_id: {}", topic_id);

//     loop {
//         let msg = MY_CLIENT.receive().await;
//         match msg {
//             Message::Publish { topic, payload } => {
//                 if let Ok(utf8) = str::from_utf8(&payload) {
//                     info!("got message for topic_id {}: \"{}\"", topic, utf8);
//                 } else {
//                     info!("got message for topic_id {} (non-utf8)", topic)
//                 }
//             }
//             Message::TopicInfo { msgid, topic_id } => {
//                 info!(
//                     "CLIENT UNEXPECTED: got msg_id {} -> topic_id {}",
//                     msgid, topic_id
//                 )
//             }
//         }
//     }

//     // info!("mqtt_sn_test() end");
// }

// #[ariel_os::task(autostart)]
// async fn mqtt_sn_test2() {
//     static MY_CLIENT: Client = Client::new();

//     info!("mqtt_sn_test2()");

//     let topic_id = MY_CLIENT
//         .subscribe(Topic::from_long("my_topic"))
//         .await
//         .unwrap();
//     info!("other topic_id: {}", topic_id);

//     loop {
//         let msg = MY_CLIENT.receive().await;
//         match msg {
//             Message::Publish { topic, payload } => {
//                 if let Ok(utf8) = str::from_utf8(&payload) {
//                     info!("other got message for topic_id {}: \"{}\"", topic, utf8);
//                 } else {
//                     info!("other got message for topic_id {} (non-utf8)", topic)
//                 }
//             }
//             Message::TopicInfo { msgid, topic_id } => {
//                 info!(
//                     "CLIENT UNEXPECTED: got msg_id {} -> topic_id {}",
//                     msgid, topic_id
//                 )
//             }
//         }
//     }
// }

// #[ariel_os::task(autostart)]
// async fn mqtt_sn_test3() {
//     static MY_CLIENT: Client = Client::new();

//     info!("mqtt_sn_test3()");

//     let topic_id = MY_CLIENT
//         .register(Topic::from_long("my_topic"))
//         .await
//         .unwrap();
//     info!("publish topic_id: {}", topic_id);

//     loop {
//         info!("Publish iteration");
//         let payload = b"Test Message";
//         let _ = MY_CLIENT.publish(Topic::from_id(topic_id), payload).await;
//         Timer::after(Duration::from_millis(1000)).await;
//     }

//     // info!("mqtt_sn_test() end");
// }

#[ariel_os::task(autostart, peripherals)]
async fn mqtt_sn_test4(peripherals: pins::LedPeripherals) {
    let mut led = Output::new(peripherals.led, Level::High);

    static MY_CLIENT: Client = Client::new();

    ariel_os::asynch::spawner()
        .spawn(ariel_os_mqttsn_async::start(Settings::default()))
        .unwrap();

    info!("mqtt_sn_test4()");

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
                    match utf8 {
                        "0" => {
                            led.set_high();
                            info!("LED turned off");
                        }
                        "1" => {
                            led.set_low();
                            info!("LED turned on");
                        }
                        _ => {
                            info!("invalid LED command");
                            continue;
                        }
                    }
                } else {
                    info!("got message for topic_id {} (non-utf8)", topic);
                }
            }
            _ => {
                info!("MESSAGE UNEXPECTED");
            }
        }
    }
}

// #[ariel_os::task(autostart)]
// async fn mqtt_sn_counter() {
//     let mut counter: usize = 0;

//     loop {
//         info!("{} seconds passed...", counter);
//         counter += 1;
//         Timer::after_secs(1).await
//     }
// }
