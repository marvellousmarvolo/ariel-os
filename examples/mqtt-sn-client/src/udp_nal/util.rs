//! Helpers for [`udp_nal`] -- conversion and error types

use core::net::{SocketAddr, SocketAddrV4, SocketAddrV6};

use embassy_net::{udp, IpAddress, IpEndpoint};

/// Converts socket address types between [`embedded_nal_async`] and [`embassy_net`] (internally
/// `smol`).
///
/// # Errors
///
/// This produces an error if an address family is unavailable.
#[allow(
    clippy::unnecessary_wraps,
    reason = "errors are currently only impossible because for crate feature synchronization reasons, all cfg handling is commented out"
)]
pub(super) fn sockaddr_nal2smol(sockaddr: SocketAddr) -> Result<IpEndpoint, Error> {
    Ok(IpEndpoint::from(sockaddr))
}

pub(super) fn sockaddr_smol2nal(endpoint: IpEndpoint) -> SocketAddr {
    match endpoint.addr {
        IpAddress::Ipv4(addr) => SocketAddr::V4(SocketAddrV4::new(addr, endpoint.port)),
        IpAddress::Ipv6(addr) => SocketAddr::V6(SocketAddrV6::new(addr, endpoint.port, 0, 0)),
    }
}

/// Is the IP address in this type the unspecified address?
///
/// FIXME: What of `::ffff:0.0.0.0`? Is that expected to bind to all v4 addresses?
pub(super) fn is_unspec_ip(addr: SocketAddr) -> bool {
    match addr {
        SocketAddr::V4(sockaddr) => sockaddr.ip().octets() == [0; 4],
        SocketAddr::V6(sockaddr) => sockaddr.ip().octets() == [0; 16],
    }
}

/// Unified error type for [`embedded_nal_async`] operations on UDP sockets
#[derive(Debug)]
#[non_exhaustive]
#[allow(
    clippy::enum_variant_names,
    reason = "false positive -- they're not called SomethingError because they are a Self (which is named Error), but because they contain a type SomethingError"
)]
pub enum Error {
    /// Error stemming from failure to send
    RecvError(udp::RecvError),
    /// Error stemming from failure to send
    SendError(udp::SendError),
    /// Error stemming from failure to bind to an address/port
    BindError(udp::BindError),
    /// Error stemming from failure to represent the given address family for lack of enabled
    /// embassy-net features
    #[expect(dead_code, reason = "feature selection currently disabled")]
    AddressFamilyUnavailable,
}

impl embedded_io_async::Error for Error {
    fn kind(&self) -> embedded_io_async::ErrorKind {
        match self {
            Self::SendError(udp::SendError::NoRoute) | Self::BindError(udp::BindError::NoRoute) => {
                embedded_io_async::ErrorKind::AddrNotAvailable
            }
            Self::AddressFamilyUnavailable => embedded_io_async::ErrorKind::AddrNotAvailable,
            // These should not happen b/c our sockets are typestated.
            Self::SendError(udp::SendError::SocketNotBound) |
                Self::BindError(udp::BindError::InvalidState) |
            // This should not happen b/c in embedded_nal_async this is not expressed through an
            // error.
            // FIXME we're not there in this impl yet.
            Self::RecvError(udp::RecvError::Truncated) => embedded_io_async::ErrorKind::Other,
        }
    }
}
impl From<udp::BindError> for Error {
    fn from(err: udp::BindError) -> Self {
        Self::BindError(err)
    }
}
impl From<udp::RecvError> for Error {
    fn from(err: udp::RecvError) -> Self {
        Self::RecvError(err)
    }
}
impl From<udp::SendError> for Error {
    fn from(err: udp::SendError) -> Self {
        Self::SendError(err)
    }
}
