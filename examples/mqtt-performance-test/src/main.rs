#![no_std]
#![no_main]
#![allow(async_fn_in_trait)]

mod action;
mod channels;
mod event;
mod example_mqtt_manager;
mod pins;
mod ui_send;
// mod ui_recv;

use crate::action::Action;
use crate::channels::{ActionChannel, EventChannel};
use crate::event::Event;
use crate::ui_send::ui_task;
// use crate::ui_recv::ui_task;

use ariel_os::{
    asynch::Spawner,
    debug::log::*,
    gpio::{Input, Level, Output, Pull},
    // net,
    // reexports::embassy_net::{Ipv4Address, Stack},
    time::{Duration, Timer},
};
use embassy_net::Ipv4Address;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, pubsub::PubSubChannel};
use static_cell::StaticCell;

const MQTT_HOST: &str = env!("MQTT_BROKER_ADDR");
const MQTT_PORT: &str = "1883"; //env!("MQTT_PORT");

static EVENT_CHANNEL: StaticCell<EventChannel> = StaticCell::new();
static ACTION_CHANNEL: StaticCell<ActionChannel> = StaticCell::new();

#[ariel_os::task(autostart, peripherals)]
async fn main(peripherals: pins::Peripherals) {
    // Init peripherals

    let mut btn = Input::builder(peripherals.btn1, Pull::Up)
        .build_with_interrupt()
        .unwrap();

    let mut pin = Input::builder(peripherals.signal_pin, Pull::Up)
        .build_with_interrupt()
        .unwrap();

    let event_channel =
        EVENT_CHANNEL.init(PubSubChannel::<CriticalSectionRawMutex, Event, 16, 4, 2>::new());
    let event_pub_mqtt = event_channel.publisher().unwrap();
    let event_sub_ui = event_channel.subscriber().unwrap();

    let action_channel =
        ACTION_CHANNEL.init(PubSubChannel::<CriticalSectionRawMutex, Action, 16, 4, 4>::new());
    let action_pub_ui = action_channel.publisher().unwrap();
    let action_sub = action_channel.subscriber().unwrap();

    let host = MQTT_HOST.parse::<Ipv4Address>().unwrap();
    let port = MQTT_PORT.parse::<u16>().unwrap();

    let spawner = ariel_os::asynch::spawner();

    spawner
        .spawn(ui_task(event_sub_ui, action_pub_ui, btn))
        .unwrap();
    let client_id: &str = "ariel_0";

    // spawner
    //     .spawn(ui_task(event_sub_ui, action_pub_ui, pin))
    //     .unwrap();
    // let client_id: &str = "ariel_1";

    example_mqtt_manager::init(
        &spawner,
        // stack<'static>,
        &client_id,
        event_pub_mqtt,
        action_sub,
        host,
        port,
    )
    .await;

    loop {
        Timer::after(Duration::from_secs(5)).await;
    }
}

// CONFIG_WIFI_NETWORK=<ssid> CONFIG_WIFI_PASSWORD=<pwd> MQTT_HOST=<broker_ip> MQTT_PORT=<broker_port> laze build ...
