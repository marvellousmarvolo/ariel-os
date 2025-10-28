use crate::flags::Flags;
use ariel_os::debug::log::defmt;
use bilge::prelude::*;

#[bitsize(8)]
#[derive(TryFromBits, Debug, PartialEq, defmt::Format)]
pub enum ReturnCode {
    Accepted = 0x00,
    RejectedCongestion = 0x01,
    RejectedInvalidTopicId = 0x02,
    RejectedNotSupported = 0x03,
}

#[bitsize(24)]
#[derive(TryFromBits, DebugBits, PartialEq)]
pub struct Advertise {
    duration: u16,
    gw_id: u8,
}

#[bitsize(8)]
#[derive(TryFromBits, DebugBits, PartialEq)]
pub struct SearchGw {
    radius: u8,
}

impl SearchGw {
    pub const SIZE: usize = 1;

    pub fn to_be_bytes(&self) -> [u8; Self::SIZE] {
        self.value.to_be_bytes()
    }
}

// pub struct GwInfo {
//     /// address of the indicated GW; optional
//     gw_add: usize,
//     gw_id: u8,
// }

#[bitsize(32)]
#[derive(TryFromBits, DebugBits, PartialEq)]
pub struct Connect {
    duration: u16,
    protocol_id: u8,
    flags: Flags,
}

impl Connect {
    pub const SIZE: usize = 4;
    pub fn to_be_bytes(&self) -> [u8; Self::SIZE] {
        self.value.to_be_bytes()
    }
}

#[bitsize(8)]
#[derive(TryFromBits, DebugBits, PartialEq, defmt::Format)]
pub struct ConnAck {
    return_code: ReturnCode,
}

impl ConnAck {
    pub const SIZE: usize = 1;

    pub fn get_return_code(&self) -> ReturnCode {
        self.return_code()
    }
}

// pub struct WillTopicReq {}
//
// pub struct WillTopic {
//     will_topic: u8,
//     flags: Flags,
// }
//
// pub struct WillMsgReq {}
//
// pub struct WillMsg {
//     will_msg: u8,
// }

#[bitsize(32)]
#[derive(TryFromBits, DebugBits, PartialEq)]
pub struct Register {
    msg_id: u16,
    topic_id: u16,
}

impl Register {
    pub const SIZE: usize = 4;

    pub fn to_be_bytes(&self) -> [u8; Self::SIZE] {
        self.value.to_be_bytes()
    }
}

#[bitsize(40)]
#[derive(TryFromBits, DebugBits, PartialEq)]
pub struct RegAck {
    return_code: ReturnCode,
    msg_id: u16,
    topic_id: u16,
}

impl RegAck {
    pub const SIZE: usize = 5;

    pub fn get_topic_id(&self) -> u16 {
        self.topic_id()
    }

    pub fn get_return_code(&self) -> ReturnCode {
        self.return_code()
    }
}

#[bitsize(40)]
#[derive(TryFromBits, DebugBits, PartialEq)]
pub struct Publish {
    /// only relevant in case of QoS levels 1 and 2, otherwise coded 0x0000
    msg_id: u16,
    /// contains the topic id value or the short topic name for which the data is published
    topic_id: u16,
    flags: Flags,
}

impl Publish {
    pub const SIZE: usize = 5;

    pub fn to_be_bytes(&self) -> [u8; Self::SIZE] {
        self.value.to_be_bytes()
    }

    pub fn get_topic_id(&self) -> u16 {
        self.topic_id()
    }
}

#[bitsize(40)]
#[derive(TryFromBits, DebugBits, PartialEq)]
pub struct PubAck {
    return_code: ReturnCode,
    msg_id: u16,
    topic_id: u16,
}

#[bitsize(16)]
#[derive(TryFromBits, DebugBits, PartialEq)]
pub struct PubRec {
    msg_id: u16,
}

#[bitsize(24)]
#[derive(TryFromBits, DebugBits, PartialEq)]
pub struct Subscribe {
    msg_id: u16,
    flags: Flags,
}

impl Subscribe {
    pub const SIZE: usize = 3;

    pub fn to_be_bytes(&self) -> [u8; Self::SIZE] {
        self.value.to_be_bytes()
    }
}

#[bitsize(48)]
#[derive(TryFromBits, DebugBits, PartialEq)]
pub struct SubAck {
    return_code: ReturnCode,
    msg_id: u16,
    topic_id: u16,
    flags: Flags,
}

impl SubAck {
    pub const SIZE: usize = 6;

    pub fn get_topic_id(&self) -> u16 {
        self.topic_id()
    }

    pub fn get_return_code(&self) -> ReturnCode {
        self.return_code()
    }
}

#[bitsize(40)]
#[derive(TryFromBits, DebugBits, PartialEq)]
pub struct Unsubscribe {
    topic_id: u16, // or topic_name length n
    msg_id: u16,
    flags: Flags,
}

#[bitsize(16)]
#[derive(TryFromBits, DebugBits, PartialEq)]
pub struct UnsubAck {
    msg_id: u16,
}

#[bitsize(16)]
#[derive(TryFromBits, DebugBits, PartialEq)]
pub struct Disconnect {
    duration: u16, // optional
}

// pub struct WillTopicUpd {
//     will_topic: u8,
//     flags: Flags,
// }
//
// pub struct WillMsgUpd {
//     will_msg: u8,
// }

#[bitsize(8)]
#[derive(TryFromBits, DebugBits, PartialEq)]
pub struct WillTopicResp {
    return_code: ReturnCode,
}

#[bitsize(8)]
#[derive(TryFromBits, DebugBits, PartialEq)]
pub struct WillMsgcResp {
    return_code: ReturnCode,
}

// pub struct ForwarderEncapsulation {}

// fn try_from(msg_type: MsgType, bytes: &[u8]) -> (MessageVariablePart, &[u8]) {
//     match msg_type {
//         MsgType::Advertise => {
//             Advertise::try_from(bytes).unwrap();
//         }
//         MsgType::SearchGw => {}
//         MsgType::GwInfo => {}
//         MsgType::Connect => {}
//         MsgType::ConnAck => {}
//         MsgType::WillTopicReq => {}
//         MsgType::WillTopic => {}
//         MsgType::WillMsgReq => {}
//         MsgType::WillMsg => {}
//         MsgType::Register => {}
//         MsgType::RegAck => {}
//         MsgType::Publish => {}
//         MsgType::PubAck => {}
//         MsgType::PubComp => {}
//         MsgType::PubRec => {}
//         MsgType::PubRel => {}
//         MsgType::Subscribe => {}
//         MsgType::SubAck => {}
//         MsgType::Unsubscribe => {}
//         MsgType::UnsubAck => {}
//         MsgType::PingReq => {}
//         MsgType::PingResp => {}
//         MsgType::Disconnect => {}
//         MsgType::WillTopicUpd => {}
//         MsgType::WillTopicEsp => {}
//         MsgType::WillMsgUpd => {}
//         MsgType::WillMsgEsp => {}
//         MsgType::Encapsulated => {}
//     }
// }
