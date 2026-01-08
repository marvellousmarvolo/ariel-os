#![no_main]
#![no_std]

mod pins;

use ariel_os::{
    debug::log::*,
    gpio::{Input, IntEnabledInput, Level, Output, Pull},
    time::{Duration, Timer},
};
use ariel_os_mqttsn_async::{
    client::{Client, Message, Topic},
    settings::Settings,
};

#[ariel_os::task(autostart, peripherals)]
async fn mqtt_sn_home_assistant(peripherals: pins::Peripherals) {
    static COMMAND_CLIENT: Client = Client::new();

    let mut led = Output::new(peripherals.led, Level::High);
    let btn = Input::builder(peripherals.btn1, Pull::Up)
        .build_with_interrupt()
        .unwrap();

    let spawner = ariel_os::asynch::spawner();
    spawner
        .spawn(ariel_os_mqttsn_async::start(Settings::default()))
        .unwrap();
    spawner.spawn(discovery(btn)).unwrap();

    info!("mqtt_sn_home_assistant()");

    let command_topic = COMMAND_CLIENT
        .subscribe(Topic::from_long("homeassistant/switch/ariel_os_led/set"))
        .await
        .unwrap();
    info!("command_topic: {}", command_topic);

    loop {
        let msg = COMMAND_CLIENT.receive().await;
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

#[ariel_os::task()]
async fn discovery(mut btn: IntEnabledInput) {
    static DISCOVERY_CLIENT: Client = Client::new();

    let discovery_topic_id = DISCOVERY_CLIENT
        .register(Topic::from_long("homeassistant/switch/ariel_os_led/config"))
        .await
        .unwrap();
    info!("discovery_topic: {}", discovery_topic_id);

    let mut is_discovered = false;

    loop {
        btn.wait_for_low().await;

        let discovery_payload: &str = match is_discovered {
            true => "",
            false => {
                "{\"name\":\"Ariel OS Test LED\",\"device_class\":\"switch\",\"command_topic\":\"homeassistant/switch/ariel_os_led/set\",\"icon\":\"mdi:alarm-light\",\"payload_on\":\"1\",\"payload_off\":\"0\",\"unique_id\":\"led01ad\",\"device\":{\"identifiers\":[\"01ad\"],\"name\":\"Office\"}}"
            }
        };

        info!("discovery message: {:?}", discovery_payload);

        match DISCOVERY_CLIENT
            .publish(
                Topic::from_id(discovery_topic_id),
                discovery_payload.as_bytes(),
            )
            .await
        {
            Ok(_) => info!("Discovery packet sent"),
            Err(e) => info!("{:?}", e),
        }

        is_discovered = !is_discovered;

        Timer::after(Duration::from_millis(1000)).await;
    }
}
