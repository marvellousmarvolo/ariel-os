use crate::action::Action;
use crate::channels::{ActionPub, EventSub};
use crate::event::Event;

use ariel_os::{
    debug::log::*,
    gpio::Output,
    time::{Duration, Instant, Timer},
};

#[ariel_os::task()]
pub async fn ui_task(
    mut event_sub: EventSub,
    action_pub: ActionPub,
    mut pin: Output,
    max_iterations: usize,
) {
    let max_iterations = 1000;

    info!("20 seconds to start...");
    Timer::after_secs(20).await;

    for i in 0..max_iterations {
        pin.toggle();
        info!("Start!");

        let t_start = Instant::now().as_micros();
        let payload = t_start.to_be_bytes();

        if action_pub.free_capacity() > 8 {
            action_pub.publish_immediate(Action::Publish(t_start));
        }

        let msg = event_sub.next_message_pure().await;
        let t_roundtrip = Instant::now().as_micros() - t_start;

        info!("Iteration {}", i);
        match msg {
            Event::Message(payload) => {
                info!("  One-way took {} µs", payload);
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
