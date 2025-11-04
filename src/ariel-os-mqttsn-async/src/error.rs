use ariel_os_debug::log::defmt;

#[derive(Debug, Copy, Clone, PartialEq, defmt::Format)]
pub enum Error {
    InvalidState,
    Timeout,
    TransmissionFailed,
    ConversionFailed,
    Rejected,
    InvalidIdType,
    NoFreeSubscriberSlot,
    PayloadTooBig,
}
