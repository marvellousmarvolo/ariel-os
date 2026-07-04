use crate::action::Action;
use crate::channels::{ActionPub, EventSub};
use crate::event::Event;

use ariel_os::{
    debug::log::*,
    gpio::IntEnabledInput,
    time::{Duration, Instant, Timer},
};

#[ariel_os::task()]
pub async fn ui_task(mut event_sub: EventSub, action_pub: ActionPub, mut pin: IntEnabledInput) {
    let mut loop_count = 0;

    info!("Wait for start signal...");

    loop {
        pin.wait_for_any_edge().await;
        info!("Start!");

        let t_local = Instant::now().as_micros();
        let msg = event_sub.next_message_pure().await;
        let t_delta = Instant::now().as_micros() - t_local;
        info!("Iteration {}", loop_count);
        info!("  One-way took {} µs", t_delta);

        match msg {
            Event::Message(payload) => {
                if action_pub.free_capacity() > 8 {
                    action_pub.publish_immediate(Action::Publish(t_delta));
                }
            }
            _ => {
                info!("  MESSAGE UNEXPECTED");
            }
        }

        loop_count += 1;
        Timer::after_secs(1).await;
    }
    info!("BENCHMARK DONE");
}
