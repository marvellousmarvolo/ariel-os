use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::pubsub::{PubSubChannel, Publisher, Subscriber};

use crate::action::Action;
use crate::event::Event;

const ACTION_CAP: usize = 16;
const ACTION_SUBS: usize = 4;
const ACTION_PUBS: usize = 4;
pub type ActionChannel = PubSubChannel<CriticalSectionRawMutex, Action, ACTION_CAP, ACTION_SUBS, ACTION_PUBS>;
pub type ActionPub = Publisher<'static, CriticalSectionRawMutex, Action, ACTION_CAP, ACTION_SUBS, ACTION_PUBS>;
pub type ActionSub =
    Subscriber<'static, CriticalSectionRawMutex, Action, ACTION_CAP, ACTION_SUBS, ACTION_PUBS>;

const EVENT_CAP: usize = 16;
const EVENT_SUBS: usize = 4;
const EVENT_PUBS: usize = 2;
pub type EventChannel = PubSubChannel<CriticalSectionRawMutex, Event, EVENT_CAP, EVENT_SUBS, EVENT_PUBS>;
pub type EventPub = Publisher<'static, CriticalSectionRawMutex, Event, EVENT_CAP, EVENT_SUBS, EVENT_PUBS>;
pub type EventSub = Subscriber<'static, CriticalSectionRawMutex, Event, EVENT_CAP, EVENT_SUBS, EVENT_PUBS>;
