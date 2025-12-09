#[cfg(feature = "defmt")]
use ariel_os_debug::log::defmt;

#[derive(Debug, Copy, Clone, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    InvalidState,
    Timeout,
    TransmissionFailed,
    ConversionFailed,
    Rejected,
    Congestion,
    InvalidIdType,
    NoFreeSubscriberSlot,
    PayloadTooBig,
}
