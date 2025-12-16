use crate::{
    Topic,
    serialization::{
        flags::{Flags, QoS, TopicIdType},
        header::{Header, MsgType, calculate_message_length},
        message_variable_part as mvp,
        packet::Error::{PacketNotRecognized, ParsingFailed},
    },
};
use ariel_os_debug::log::*;
use bilge::arbitrary_int::{u40, u48};

#[derive(PartialEq)]
pub enum Packet<'a> {
    // Advertise(Header, Advertise),
    // SearchGw(Header, SearchGw),
    // GwInfo(Header, GwInfo, [u8; 4096]),
    Connect {
        header: Header,
        connect: mvp::Connect,
        client_id: &'a [u8],
    },
    Disconnect {
        header: Header,
        duration: Option<u16>,
    },
    ConnAck {
        header: Header,
        conn_ack: mvp::ConnAck,
    },
    Register {
        header: Header,
        register: mvp::Register,
        topic: &'a Topic,
    },
    RegAck {
        header: Header,
        reg_ack: mvp::RegAck,
    },
    Publish {
        header: Header,
        publish: mvp::Publish,
        data: &'a [u8],
    },
    Subscribe {
        header: Header,
        subscribe: mvp::Subscribe,
        topic: &'a Topic,
    },
    SubAck {
        header: Header,
        sub_ack: mvp::SubAck,
    },
    Unsubscribe {
        header: Header,
        unsubscribe: mvp::Unsubscribe,
        topic: &'a Topic,
    },
    PingReq {
        header: Header,
        client_id: &'a [u8], // >= 1
    },
    PingResp {
        header: Header,
    },
}

macro_rules! construct_buffer {
    ($buf:tt, $header: tt, $mvp: tt, $mvp_len:expr) => {{
        let mvp_size = $header.size() + $mvp_len;

        $buf[..$header.size()].copy_from_slice(&$header.to_be_bytes()[..$header.size()]);
        $buf[$header.size()..mvp_size].copy_from_slice(&$mvp.to_be_bytes()[..$mvp_len]);

        mvp_size
    }};
    ($buf:tt, $header: tt, $mvp: tt, $mvp_len:expr, $payload: tt) => {{
        construct_buffer!($buf, $header, $mvp, $mvp_len);

        let mvp_size = $header.size() + $mvp_len;

        $buf[mvp_size..$header.length()].copy_from_slice($payload);

        mvp_size
    }};
}

pub enum Error {
    PacketNotRecognized,
    ParsingFailed,
}

impl Packet<'_> {
    pub fn try_from(bytes: &[u8]) -> Result<Packet<'_>, Error> {
        debug!("bytes: {:?}", bytes);

        let header = Header::try_from(bytes).unwrap();

        debug!("header: {:?}", header);

        let msg_type = header.msg_type();

        info!("msg_type: {:?}", msg_type);

        match msg_type {
            // MsgType::Advertise => {}
            // MsgType::SearchGw => {}
            // MsgType::GwInfo => {}
            // MsgType::Connect => {} // no recv
            MsgType::ConnAck => {
                let mvp_size = header.size() + mvp::ConnAck::SIZE;

                let mvp: [u8; mvp::ConnAck::SIZE] =
                    bytes[header.size()..mvp_size].try_into().unwrap();

                let conn_ack = match mvp::ConnAck::try_from(u8::from_be_bytes(mvp)) {
                    Ok(it) => it,
                    Err(_) => return Err(ParsingFailed),
                };

                Ok(Packet::ConnAck { header, conn_ack })
            }
            // MsgType::WillTopicReq => {}
            // MsgType::WillTopic => {}
            // MsgType::WillMsgReq => {}
            // MsgType::WillMsg => {}
            // MsgType::Register => {}
            MsgType::RegAck => {
                let mvp_size = header.size() + mvp::RegAck::SIZE;

                let mvp: [u8; mvp::RegAck::SIZE] = match bytes[header.size()..mvp_size].try_into() {
                    Ok(it) => it,
                    Err(_) => return Err(ParsingFailed),
                };

                let reg_ack = match mvp::RegAck::try_from(u40::from_be_bytes(mvp)) {
                    Ok(it) => it,
                    Err(_) => return Err(ParsingFailed),
                };

                Ok(Packet::RegAck { header, reg_ack })
            }
            MsgType::Publish => {
                let mvp_size = header.size() + mvp::Publish::SIZE;

                let mvp: [u8; mvp::Publish::SIZE] = match bytes[header.size()..mvp_size].try_into()
                {
                    Ok(it) => it,
                    Err(_) => return Err(ParsingFailed),
                };

                let publish = match mvp::Publish::try_from(u40::from_be_bytes(mvp)) {
                    Ok(it) => it,
                    Err(_) => return Err(ParsingFailed),
                };

                Ok(Packet::Publish {
                    header,
                    publish,
                    data: &bytes[mvp_size..],
                })
            }
            // MsgType::PubAck => {}
            // MsgType::PubComp => {}
            // MsgType::PubRec => {}
            // MsgType::PubRel => {}
            // MsgType::Subscribe => {
            //     let mvp_size = header.size() + mvp::Subscribe::SIZE;

            //     let mvp: [u8; mvp::Subscribe::SIZE] =
            //         bytes[header.size()..mvp_size].try_into().unwrap();

            //     let subscribe = mvp::Subscribe::try_from(u24::from_be_bytes(mvp)).unwrap();

            //     Ok(Packet::Subscribe {
            //         header,
            //         subscribe,
            //         topic: Topic::from_bytes(&bytes[mvp_size..]),
            //     })
            // }
            MsgType::SubAck => {
                let mvp_size = header.size() + mvp::SubAck::SIZE;

                let mvp: [u8; mvp::SubAck::SIZE] =
                    bytes[header.size()..mvp_size].try_into().unwrap();

                let sub_ack = mvp::SubAck::try_from(u48::from_be_bytes(mvp)).unwrap();

                Ok(Packet::SubAck { header, sub_ack })
            }
            // MsgType::Unsubscribe => {}
            // MsgType::UnsubAck => {}
            MsgType::PingReq => {
                let size = header.size();

                Ok(Packet::PingReq {
                    header,
                    client_id: &bytes[size..],
                })
            }
            // MsgType::PingResp => {}
            MsgType::Disconnect => {
                let size = header.size();

                match size > header.length() {
                    true => {
                        let duration_bytes: [u8; 2] = bytes[size..size + 2].try_into().unwrap();
                        Ok(Packet::Disconnect {
                            header,
                            duration: Some(u16::from_be_bytes(duration_bytes)),
                        })
                    }
                    false => Ok(Packet::Disconnect {
                        header,
                        duration: None,
                    }),
                }
            }
            // MsgType::WillTopicUpd => {}
            // MsgType::WillTopicEsp => {}
            // MsgType::WillMsgUpd => {}
            // MsgType::WillMsgEsp => {}
            // MsgType::Encapsulated => {}
            _ => Err(PacketNotRecognized),
        }
    }
    //
    pub fn write_to_buf<'a>(&self, buf: &'a mut [u8]) -> &'a [u8] {
        match self {
            // MsgType::Advertise => {}
            // Packet::SearchGw { header, search_gw } => {
            //     construct_buffer!(buf, header, search_gw, mvp::SearchGw::SIZE);
            // }
            // MsgType::GwInfo => {}
            Packet::Connect {
                header,
                connect,
                client_id,
            } => {
                if client_id.len() > 23 {
                    panic!("Client Id longer than maximum of 23 characters");
                }
                construct_buffer!(buf, header, connect, mvp::Connect::SIZE, client_id);
            }
            // MsgType::ConnAck => {} // no send
            // MsgType::WillTopicReq => {}
            // MsgType::WillTopic => {}
            // MsgType::WillMsgReq => {}
            // MsgType::WillMsg => {}
            Packet::Register {
                header,
                register,
                topic,
            } => {
                let written = construct_buffer!(buf, header, register, mvp::Register::SIZE);
                topic.to_buf(&mut buf[written..]);
            }
            // MsgType::RegAck => {}
            Packet::Publish {
                header,
                publish,
                data,
            } => {
                construct_buffer!(buf, header, publish, mvp::Publish::SIZE, data);
            }
            // MsgType::PubAck => {}
            // MsgType::PubComp => {}
            // MsgType::PubRec => {}
            // MsgType::PubRel => {}
            Packet::Subscribe {
                header,
                subscribe,
                topic,
            } => {
                let written = construct_buffer!(buf, header, subscribe, mvp::Subscribe::SIZE);
                topic.to_buf(&mut buf[written..]);
            }
            // MsgType::SubAck => {} // no send
            // MsgType::Unsubscribe => {}
            // MsgType::UnsubAck => {}
            Packet::PingReq { header, client_id } => {
                buf[..header.size()].copy_from_slice(&header.to_be_bytes()[..header.size()]);
                buf[header.size()..header.size() + client_id.len()].copy_from_slice(client_id);
            }
            Packet::PingResp { header } => {
                buf[..header.size()].copy_from_slice(&header.to_be_bytes()[..header.size()]);
            }
            Packet::Disconnect { header, duration } => match duration {
                Some(duration_value) => {
                    let duration_bytes = duration_value.to_be_bytes();
                    buf[..header.size()].copy_from_slice(&header.to_be_bytes()[..header.size()]);
                    buf[header.size()..header.size() + duration_bytes.len()]
                        .copy_from_slice(&duration_bytes);
                }
                None => {
                    buf[..header.size()].copy_from_slice(&header.to_be_bytes()[..header.size()])
                }
            },
            // MsgType::WillTopicUpd => {}
            // MsgType::WillTopicEsp => {}
            // MsgType::WillMsgUpd => {}
            // MsgType::WillMsgEsp => {}
            // MsgType::Encapsulated => {}
            _ => {
                info!("Packet not recognized");
            }
        }
        &buf[..self.length()]
    }

    pub fn get_msg_type(&self) -> MsgType {
        match self {
            Packet::Connect { header, .. } => header.msg_type(),
            Packet::ConnAck { header, .. } => header.msg_type(),
            Packet::Register { header, .. } => header.msg_type(),
            Packet::RegAck { header, .. } => header.msg_type(),
            Packet::Publish { header, .. } => header.msg_type(),
            Packet::Subscribe { header, .. } => header.msg_type(),
            Packet::SubAck { header, .. } => header.msg_type(),
            Packet::Unsubscribe { header, .. } => header.msg_type(),
            Packet::PingReq { header, .. } => header.msg_type(),
            Packet::PingResp { header, .. } => header.msg_type(),
            Packet::Disconnect { header, .. } => header.msg_type(),
        }
    }

    pub fn length(&self) -> usize {
        match self {
            Packet::Connect { header, .. } => header.length(),
            Packet::ConnAck { header, .. } => header.length(),
            Packet::Register { header, .. } => header.length(),
            Packet::RegAck { header, .. } => header.length(),
            Packet::Publish { header, .. } => header.length(),
            Packet::Subscribe { header, .. } => header.length(),
            Packet::SubAck { header, .. } => header.length(),
            Packet::Unsubscribe { header, .. } => header.length(),
            Packet::PingReq { header, .. } => header.length(),
            Packet::PingResp { header, .. } => header.length(),
            Packet::Disconnect { header, .. } => header.length(),
        }
    }
}

// packet creation methods
impl Packet<'_> {
    pub(crate) fn connect(
        keep_alive: u16,
        clean_session: bool,
        will: bool,
        client_id: &[u8],
    ) -> Packet<'_> {
        // build "connect" packet
        let flags = Flags::new(
            TopicIdType::IdNormal,
            clean_session,
            will,
            false,
            QoS::Zero,
            false,
        );

        let msg_len = calculate_message_length(client_id.len() + mvp::Connect::SIZE);

        Packet::Connect {
            header: Header::new(MsgType::Connect, msg_len),
            connect: mvp::Connect::new(keep_alive, 0x01, flags),
            client_id,
        }
    }

    pub(crate) fn disconnect<'a>(duration: Option<u16>) -> Packet<'a> {
        match duration {
            Some(duration_value) => Packet::Disconnect {
                header: Header::new(MsgType::Disconnect, calculate_message_length(2)),
                duration: Some(duration_value),
            },
            None => Packet::Disconnect {
                header: Header::new(MsgType::Disconnect, calculate_message_length(0)),
                duration: None,
            },
        }
    }

    pub(crate) fn subscribe<'a>(
        topic: &'a crate::Topic,
        dup: bool,
        qos: QoS,
        msg_id: u16,
    ) -> Packet<'a> {
        let flags = Flags::new(topic.into(), true, false, false, qos, dup);
        let msg_len = calculate_message_length(topic.len() + mvp::Subscribe::SIZE);

        Packet::Subscribe {
            header: Header::new(MsgType::Subscribe, msg_len),
            subscribe: mvp::Subscribe::new(msg_id, flags),
            topic: &topic,
        }
    }

    pub(crate) fn unsubscribe<'a>(topic: &'a crate::Topic, msg_id: u16) -> Packet<'a> {
        let flags = Flags::new(topic.into(), false, false, false, QoS::Zero, false);
        let msg_len = calculate_message_length(topic.len() + mvp::Unsubscribe::SIZE);

        Packet::Unsubscribe {
            header: Header::new(MsgType::Subscribe, msg_len),
            unsubscribe: mvp::Unsubscribe::new(msg_id, flags),
            topic: &topic,
        }
    }

    pub(crate) fn register<'a>(topic: &'a crate::Topic, msg_id: u16) -> Packet<'a> {
        let msg_len = calculate_message_length(topic.len() + mvp::Register::SIZE);

        Packet::Register {
            header: Header::new(MsgType::Register, msg_len),
            register: mvp::Register::new(msg_id, 0x0000u16), // topic_id is irrelevant on client
            topic: &topic,
        }
    }

    pub(crate) fn publish<'a>(topic: &'a crate::Topic, qos: QoS, payload: &'a [u8]) -> Packet<'a> {
        let (topic_id_type, topic_value) = match topic {
            Topic::ShortName(name) => (TopicIdType::ShortName, &u16::from_be_bytes(*name)),
            Topic::Id(id) => (TopicIdType::IdPredefined, id),
            Topic::LongName(_) => panic!(),
        };

        let flags = Flags::new(topic_id_type, false, false, false, qos, false);

        let length = calculate_message_length(payload.len() + mvp::Publish::SIZE);

        Packet::Publish {
            header: Header::new(MsgType::Publish, length),
            publish: mvp::Publish::new(0x0000u16, *topic_value, flags), // msg_id 0x0000 on QoS 0 & -1
            data: payload,
        }
    }

    pub(crate) fn ping_req<'a>(client_id: &'a [u8]) -> Packet<'a> {
        Packet::PingReq {
            header: Header::new(MsgType::PingReq, 16),
            client_id,
        }
    }

    pub(crate) fn ping_resp<'a>() -> Packet<'a> {
        Packet::PingResp {
            header: Header::new(MsgType::PingResp, 16),
        }
    }
}

// #[cfg(test)]
// #[embedded_test::tests]
// mod tests {
//     use crate::{
//         flags::{Flags, QoS, TopicIdType},
//         header::{Header, MsgType},
//         message_variable_part,
//         packet::Packet,
//     };
//     use core::result;
//
//     #[test]
//     fn publish_read() {
//         let result = Packet::try_from(&[
//             0x12u8, 0x0Cu8, 0x62u8, 0x00u8, 0x01u8, 0x00u8, 0x00u8, 0x68u8, 0x65u8, 0x6Cu8, 0x6Cu8,
//             0x6Fu8, 0x20u8, 0x77u8, 0x6Fu8, 0x72u8, 0x6Cu8, 0x64u8,
//         ]);
//
//         match result {
//             Packet::Publish {
//                 header,
//                 publish,
//                 data,
//             } => {
//                 assert_eq!(header.msg_type(), MsgType::Publish);
//                 assert_eq!(data, "hello world");
//             }
//             _ => {}
//         }
//
//         //todo result.write_to_buf() "roundtrip test"
//     }
//
//     #[test]
//     fn publish_write() {
//         let data = "hello world";
//
//         let flags = Flags::new(
//             TopicIdType::IdPredefined,
//             false,
//             false,
//             false,
//             QoS::Zero,
//             false,
//         );
//
//         let length = (7 + data.len()) as u16;
//
//         let result = Packet::Publish {
//             header: Header::new(MsgType::Publish, length),
//             publish: message_variable_part::Publish::new(0, 1, flags),
//             data,
//         };
//
//         match result {
//             Packet::Publish {
//                 header,
//                 publish,
//                 data,
//             } => {
//                 assert_eq!(header.msg_type(), MsgType::Publish);
//                 assert_eq!(data, "hello world");
//             }
//             _ => {}
//         }
//     }
// }
