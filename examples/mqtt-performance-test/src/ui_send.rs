use crate::action::Action;
use crate::channels::{ActionPub, EventSub};
use crate::event::Event;

use ariel_os::{
    debug::log::*,
    gpio::{IntEnabledInput, Output},
    time::{Duration, Instant, Timer},
};

#[ariel_os::task()]
pub async fn ui_task(mut event_sub: EventSub, action_pub: ActionPub, mut btn: IntEnabledInput) {
    let mut loop_count = 0;

    loop {
        info!("Press Button to start testing!");

        let _ = btn.wait_for_low().await;

        info!("Start!");

        let t_start = Instant::now().as_micros();
        let payload = t_start.to_be_bytes();

        if action_pub.free_capacity() > 8 {
            action_pub.publish_immediate(Action::Publish(t_start));
        }

        let msg = event_sub.next_message_pure().await;
        let t_roundtrip = Instant::now().as_micros() - t_start;

        info!("Iteration {}", loop_count);
        match msg {
            Event::Message(payload) => {
                info!("  One-way took {} µs", payload);
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
