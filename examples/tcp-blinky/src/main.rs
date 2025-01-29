#![no_main]
#![no_std]
#![feature(impl_trait_in_assoc_type)]
#![feature(used_with_arg)]

mod pins;

use ariel_os::{
    debug::log::*,
    gpio::{Level, Output},
    net,
    reexports::embassy_net::tcp::TcpSocket,
    time::Duration,
};
use embedded_io_async::Write;

#[ariel_os::config(network)]
const NETWORK_CONFIG: ariel_os::reexports::embassy_net::Config = {
    use ariel_os::reexports::embassy_net::{self, Ipv4Address};

    embassy_net::Config::ipv4_static(embassy_net::StaticConfigV4 {
        address: embassy_net::Ipv4Cidr::new(Ipv4Address::new(10, 42, 0, 62), 24),
        dns_servers: heapless::Vec::new(),
        gateway: Some(Ipv4Address::new(10, 42, 0, 1)),
    })
};

#[ariel_os::task(autostart, peripherals)]
async fn tcp_echo(peripherals: pins::LedPeripherals) {
    let stack = net::network_stack().await.unwrap();

    let mut rx_buffer = [0; 4096];
    let mut tx_buffer = [0; 4096];
    let mut buf = [0; 4096];

    let mut led = Output::new(peripherals.led, Level::Low);

    // The micro:bit uses an LED matrix; pull the column line low.
    #[cfg(context = "bbc-microbit-v2")]
    let _led_col1 = Output::new(peripherals.led_col1, Level::Low);

    loop {
        let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
        // socket.set_timeout(Some(Duration::from_secs(10)));

        info!("Listening on TCP:1234...");
        if let Err(e) = socket.accept(1234).await {
            info!("accept error: {:?}", e);
            continue;
        }

        info!("Received connection from {:?}", socket.remote_endpoint());

        loop {
            let n = match socket.read(&mut buf).await {
                Ok(0) => {
                    info!("read EOF");
                    break;
                }
                Ok(n) => n,
                Err(e) => {
                    info!("read error: {:?}", e);
                    break;
                }
            };

            let msg = match core::str::from_utf8(&buf[..n]) {
                Ok(it) => it.trim(),
                Err(_) => {
                    info!("encoding error");
                    break;
                }
            };

            info!("message received: {}", msg);

            let r;
            match msg {
                "0" => {
                    led.set_high();
                    r = socket.write_all(b"LED turned off\r\n").await;
                }
                "1" => {
                    led.set_low();
                    r = socket.write_all(b"LED turned on\r\n").await;
                }
                _ => r = socket.write_all(&buf[..n]).await,
            }

            match r {
                Ok(()) => {}
                Err(e) => {
                    info!("write error: {:?}", e);
                    break;
                }
            };
        }
    }
}
