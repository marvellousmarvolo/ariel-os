use crate::error::Error;
use ariel_os_debug_log::info;
use embassy_net::{
    IpEndpoint,
    tcp::TcpSocket,
    udp::{SendError, UdpSocket},
};

#[allow(async_fn_in_trait)]
pub trait Transport {
    async fn send(&mut self, data: &[u8]) -> Result<(), Error>;
    async fn recv(&mut self, buf: &mut [u8]) -> Result<usize, Error>;
}

pub struct UdpTransport<'a> {
    socket: UdpSocket<'a>,
    remote: IpEndpoint,
}

impl<'a> UdpTransport<'a> {
    pub fn bind(
        mut socket: UdpSocket<'a>,
        local: IpEndpoint,
        remote: IpEndpoint,
    ) -> Result<Self, Error> {
        socket.bind(local.port).map_err(|_| Error::ConnectError)?;
        Ok(Self { socket, remote })
    }
}

impl<'a> Transport for UdpTransport<'a> {
    async fn send(&mut self, buf: &[u8]) -> Result<(), Error> {
        self.socket
            .send_to(buf, self.remote)
            .await
            .map_err(|_| Error::TransmissionFailed)
    }

    async fn recv(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        let (n, _remote) = self
            .socket
            .recv_from(buf)
            .await
            .map_err(|_| Error::SocketNotBound)?;
        Ok(n)
    }
}

pub struct TcpTransport<'a> {
    socket: TcpSocket<'a>,
}

impl<'a> TcpTransport<'a> {
    pub async fn connect(mut socket: TcpSocket<'a>, endpoint: IpEndpoint) -> Result<Self, Error> {
        socket
            .connect(endpoint)
            .await
            .map_err(|_| Error::ConnectError)?;
        socket.set_nagle_enabled(false);
        socket.set_timeout(None);
        Ok(Self { socket })
    }
}

impl<'a> Transport for TcpTransport<'a> {
    async fn send(&mut self, buf: &[u8]) -> Result<(), Error> {
        self.socket
            .write(buf)
            .await
            .map(|_| ())
            .map_err(|_| Error::TransmissionFailed)
    }

    async fn recv(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        self.socket
            .read(buf)
            .await
            .map_err(|_| Error::TransmissionFailed)
    }
}
