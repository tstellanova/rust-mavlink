use crate::common::MavMessage;
use crate::{MavHeader};
use crate::MavFrame;

use std::io::{self};

#[cfg(feature = "tcp")]
mod tcp;

#[cfg(feature = "udp")]
mod udp;

#[cfg(feature = "direct-serial")]
mod direct_serial;


/// A MAVLink connection
pub trait MavConnection {

    /// Receive a mavlink message.
    ///
    /// Blocks until a valid frame is received, ignoring invalid messages.
    fn recv(&self) -> io::Result<(MavHeader,MavMessage)>;

    /// Send a mavlink message
    fn send(&self, header: &MavHeader, data: &MavMessage) -> io::Result<()>;

    /// Write whole frame
    fn send_frame(&self, frame: &MavFrame) -> io::Result<()> {
        self.send(&frame.header, &frame.msg)
    }

    /// Read whole frame
    fn recv_frame(&self) -> io::Result<MavFrame> {
        let (header,msg) = self.recv()?;
        Ok(MavFrame{header,msg})
    }

    /// Send a message with default header
    fn send_default(&self, data: &MavMessage) -> io::Result<()> {
        let header = MavHeader::get_default_header();
        self.send(&header, data)
    }
}

/// Connect to a MAVLink node by address string.
///
/// The address must be in one of the following formats:
///
///  * `tcpin:<addr>:<port>` to create a TCP server, listening for incoming connections
///  * `tcpout:<addr>:<port>` to create a TCP client
///  * `udpin:<addr>:<port>` to create a UDP server, listening for incoming packets
///  * `udpout:<addr>:<port>` to create a UDP client
///  * `serial:<port>:<baudrate>` to create a serial connection
///
/// The type of the connection is determined at runtime based on the address type, so the
/// connection is returned as a trait object.
pub fn connect(address: &str) -> io::Result<Box<MavConnection + Sync + Send>> {

    let protocol_err = Err(io::Error::new(
        io::ErrorKind::AddrNotAvailable,
        "Protocol unsupported",
    ));

    if cfg!(feature = "tcp") && address.starts_with("tcp") {
        #[cfg(feature = "tcp")] {
            tcp::select_protocol(address)
        }
        #[cfg(not(feature = "tcp"))] {
            protocol_err
        }
    } else if cfg!(feature = "udp") && address.starts_with("udp") {
        #[cfg(feature = "udp")] {
            udp::select_protocol(address)
        }
        #[cfg(not(feature = "udp"))] {
            protocol_err
        }
    } else if cfg!(feature = "direct-serial") && address.starts_with("serial:") {
        #[cfg(feature = "direct-serial")] {
            Ok(Box::new(direct_serial::open(&address["serial:".len()..])?))
        }
        #[cfg(not(feature = "direct-serial"))] {
            protocol_err
        }
    } else {
        protocol_err
    }
}


