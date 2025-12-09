#![no_main]
#![no_std]

mod action;
mod channels;
mod event;
mod example_mqtt_manager;
mod pins;
mod ui;

use crate::action::Action;
use crate::channels::{ActionChannel, EventChannel};
use crate::event::Event;
use crate::example_mqtt_manager::init;
use crate::ui::ui_task;

use ariel_os::{
    asynch,
    debug::log::*,
    gpio::{Input, Level, Output, Pull},
    net,
    reexports::embassy_net::{IpAddress, IpEndpoint, Ipv4Address, tcp::TcpSocket},
    time::{Duration, Timer},
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, pubsub::PubSubChannel};
use static_cell::StaticCell;

static EVENT_CHANNEL: StaticCell<EventChannel> = StaticCell::new();
static ACTION_CHANNEL: StaticCell<ActionChannel> = StaticCell::new();

#[ariel_os::task(autostart, peripherals)]
async fn mountain_mqtt_test(peripherals: pins::Peripherals) {
    info!("Setup...");
    let ip = Ipv4Address::new(10, 42, 0, 1);
    let port = 1883;
    let timeout_millis = 5000;

    let stack = net::network_stack().await.unwrap();
    stack.wait_config_up().await;

    let mut rx_buffer = [0; 4096];
    let mut tx_buffer = [0; 4096];
    let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
    socket.set_timeout(Some(Duration::from_millis(timeout_millis)));

    if let Err(e) = socket
        .connect(IpEndpoint::new(IpAddress::Ipv4(ip), port))
        .await
    {
        info!("connect error: {:?}", e);
    }

    let event_channel =
        EVENT_CHANNEL.init(PubSubChannel::<CriticalSectionRawMutex, Event, 16, 4, 2>::new());
    let event_pub_mqtt = event_channel.publisher().unwrap();
    let event_sub_ui = event_channel.subscriber().unwrap();

    let action_channel =
        ACTION_CHANNEL.init(PubSubChannel::<CriticalSectionRawMutex, Action, 16, 4, 4>::new());
    let action_pub_ui = action_channel.publisher().unwrap();
    let action_sub = action_channel.subscriber().unwrap();

    let spawner = asynch::Spawner::for_current_executor().await;

    let led = Output::builder(peripherals.led, Level::Low).build();
    let button = Input::builder(peripherals.btn1, Pull::Up)
        .build_with_interrupt()
        .unwrap();

    spawner
        .spawn(ui_task(event_sub_ui, action_pub_ui, led, button))
        .unwrap();

    let client_id: &str = "ariel";

    init(
        &spawner,
        stack,
        &client_id,
        event_pub_mqtt,
        action_sub,
        ip,
        port,
    )
    .await;

    loop {
        Timer::after(Duration::from_secs(5)).await;
    }
}
