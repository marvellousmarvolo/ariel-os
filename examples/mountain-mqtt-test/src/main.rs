#![no_std]
#![no_main]
#![allow(async_fn_in_trait)]

mod action;
mod channels;
mod event;
mod example_mqtt_manager;
mod pins;
mod ui;

use crate::action::Action;
use crate::channels::{ActionChannel, EventChannel};
use crate::event::Event;
use crate::ui::ui_task;

use ariel_os::{
    asynch::Spawner,
    debug::log::*,
    gpio::{Input, Level, Output, Pull},
    net,
    reexports::embassy_net::{Ipv4Address, Stack},
    time::{Duration, Timer},
};
// use embassy_net::{Ipv4Address, Stack};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, pubsub::PubSubChannel};
use static_cell::StaticCell;

const MQTT_HOST: &str = env!("MQTT_HOST");
const MQTT_PORT: &str = env!("MQTT_PORT");

static EVENT_CHANNEL: StaticCell<EventChannel> = StaticCell::new();
static ACTION_CHANNEL: StaticCell<ActionChannel> = StaticCell::new();

#[ariel_os::task(autostart, peripherals)]
async fn main(peripherals: pins::Peripherals) {
    info!("Hello World!");

    // Init peripherals
    let led = Output::builder(peripherals.led, Level::Low).build();
    let btn = Input::builder(peripherals.btn, Pull::Up)
        .build_with_interrupt()
        .unwrap();

    // Init network stack
    let stack: Stack<'static> = net::network_stack().await.unwrap();

    info!("waiting for stack to be up...");
    stack.wait_config_up().await;
    info!("Stack is up!");

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

    let spawner = Spawner::for_current_executor().await;

    spawner
        .spawn(ui_task(event_sub_ui, action_pub_ui, btn, led))
        .unwrap();

    let client_id: &str = "ariel";

    example_mqtt_manager::init(
        &spawner,
        stack,
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
