#[cfg(feature = "defmt")]
use ariel_os_debug_log::defmt;
use heapless::String;

use crate::{client::MAX_TOPIC_LENGTH, error::Error, serialization::flags::TopicIdType};

/// The two-byte topic ID carried by PUBLISH, PUBACK, REGACK and SUBACK
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum TopicId {
    /// An identifier assigned by the gateway, in response to a REGISTER or to a SUBSCRIBE by name
    Normal(u16),
    /// An identifier agreed upon in advance between the client and the gateway
    Predefined(u16),
    /// A topic name of exactly two characters, which is short enough to be sent as is
    Short([u8; 2]),
}

impl TopicId {
    /// Rebuilds a topic identifier from the flag and the value found in a PUBLISH packet.
    pub(crate) fn new(id_type: TopicIdType, value: u16) -> Self {
        match id_type {
            TopicIdType::IdNormal => Self::Normal(value),
            TopicIdType::IdPredefined => Self::Predefined(value),
            TopicIdType::ShortName => Self::Short(value.to_be_bytes()),
        }
    }

    /// Returns the two bytes of the topic ID field, read as a big-endian integer
    #[must_use]
    pub fn value(&self) -> u16 {
        match self {
            Self::Normal(id) => *id,
            Self::Predefined(id) => *id,
            Self::Short(name) => u16::from_be_bytes(*name),
        }
    }
}

impl From<&TopicId> for TopicIdType {
    fn from(topic_id: &TopicId) -> Self {
        match topic_id {
            TopicId::Normal(_) => TopicIdType::IdNormal,
            TopicId::Predefined(_) => TopicIdType::IdPredefined,
            TopicId::Short(_) => TopicIdType::ShortName,
        }
    }
}

/// What a client subscribes to, or unsubscribes from
///
/// Note that there is no way to subscribe to a normal topic ID! When the client subscribes by name, the gateway returns the normal ID.
#[derive(Debug, Clone, PartialEq)]
pub enum TopicFilter {
    /// A full topic name, which may contain the wildcards `+` and `#`
    Name(String<MAX_TOPIC_LENGTH>),
    /// A two byte ID agreed upon in advance between the client and the gateway
    Predefined(u16),
    /// A topic name of exactly two characters
    Short([u8; 2]),
}

impl TopicFilter {
    pub fn name(name: &str) -> Result<Self, Error> {
        String::try_from(name)
            .map(Self::Name)
            .map_err(|_| Error::TopicTooLong)
    }

    pub(crate) fn topic_id(&self, assigned: u16) -> TopicId {
        match self {
            Self::Name(_) => TopicId::Normal(assigned),
            Self::Predefined(id) => TopicId::Predefined(*id),
            Self::Short(name) => TopicId::Short(*name),
        }
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Name(name) => name.len(),
            Self::Predefined(_) | Self::Short(_) => 2,
        }
    }

    pub(crate) fn to_buf(&self, buf: &mut [u8]) {
        match self {
            Self::Name(name) => buf[..name.len()].copy_from_slice(name.as_bytes()),
            Self::Predefined(id) => buf[..2].copy_from_slice(&id.to_be_bytes()),
            Self::Short(name) => buf[..2].copy_from_slice(name),
        }
    }
}

impl From<&TopicFilter> for TopicIdType {
    fn from(filter: &TopicFilter) -> Self {
        match filter {
            // In SUBSCRIBE and UNSUBSCRIBE, this value announces a topic name, not an ID!
            TopicFilter::Name(_) => TopicIdType::IdNormal,
            TopicFilter::Predefined(_) => TopicIdType::IdPredefined,
            TopicFilter::Short(_) => TopicIdType::ShortName,
        }
    }
}
