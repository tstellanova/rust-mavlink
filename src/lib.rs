//! The MAVLink common message set
//!
//! TODO: a parser for no_std environments
#![cfg_attr(not(feature = "std"), feature(alloc))]
#![cfg_attr(not(feature = "std"), no_std)]
#[cfg(not(feature = "std"))]
extern crate alloc;


#[cfg(feature = "std")]
use std::io::{Read, Result, Write};


#[cfg(feature = "std")]
extern crate byteorder;
#[cfg(feature = "std")]
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

#[cfg(feature = "std")]
mod connection;
#[cfg(feature = "std")]
pub use self::connection::{connect, MavConnection};

extern crate bytes;
use bytes::{Buf, Bytes, IntoBuf};

extern crate num_traits;
extern crate num_derive;
extern crate bitflags;
#[macro_use]

#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
#[allow(unused_variables)]
#[allow(unused_mut)]
pub mod common {
    use MavlinkVersion;
    include!(concat!(env!("OUT_DIR"), "/common.rs"));
}

/// Encapsulation of all possible Mavlink messages defined in common.xml
pub use self::common::MavMessage as MavMessage;

/// Metadata from a MAVLink packet header
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct MavHeader {
    pub system_id: u8,
    pub component_id: u8,
    pub sequence: u8,
}

/// Versions of the Mavlink protocol that we support
#[derive(Debug, Copy, Clone)]
pub enum MavlinkVersion {
    V1,
    V2,
}

/// Message framing marker for mavlink v1
pub const MAV_STX: u8 = 0xFE;

/// Message framing marker for mavlink v2
pub const MAV_STX_V2: u8 = 0xFD;


impl MavHeader {
    /// Return a default GCS header, seq is replaced by the connector
    /// so it can be ignored. Set `component_id` to your desired component ID.
    pub fn get_default_header() -> MavHeader {
        MavHeader {
            system_id: 255,
            component_id: 0,
            sequence: 0,
        }
    }
}

/// Encapsulation of the Mavlink message and the header,
/// important to preserve information about the sender system
/// and component id
#[derive(Debug, Clone)]
pub struct MavFrame {
    pub header: MavHeader,
    pub msg: MavMessage,
    pub protocol_version: MavlinkVersion,
}

impl MavFrame {
    /// Create a new frame with given message
//    pub fn new(msg: MavMessage) -> MavFrame {
//        MavFrame {
//            header: MavHeader::get_default_header(),
//            msg
//        }
//    }

    /// Serialize MavFrame into a vector, so it can be sent over a socket, for example.
    pub fn ser(&self) -> Vec<u8> {
        let mut v = vec![];

        // serialize header
        v.push(self.header.system_id);
        v.push(self.header.component_id);
        v.push(self.header.sequence);

        // message id
        match self.protocol_version {
            MavlinkVersion::V2 => {
                let bytes: [u8; 4] = self.msg.message_id().to_le_bytes();
                v.extend_from_slice(&bytes);
            },
            MavlinkVersion::V1 => {
                v.push(self.msg.message_id() as u8); //limit ID to u8 on mavlink v1
            }
        }

        // serialize message
        v.append(&mut self.msg.ser());

        v
    }

    /// Deserialize MavFrame from a slice that has been received from, for example, a socket.
    pub fn deser(version: MavlinkVersion, input: &[u8]) -> Option<Self> {
        let mut buf = Bytes::from(input).into_buf();

        let system_id = buf.get_u8();
        let component_id = buf.get_u8();
        let sequence = buf.get_u8();
        let header = MavHeader{system_id,component_id,sequence};

        let msg_id =  match version {
            MavlinkVersion::V2 => {
                buf.get_u32_le()
            },
            MavlinkVersion::V1 => {
                buf.get_u8() as u32
            }
        };


        if let Some(msg) = MavMessage::parse(version, msg_id, &buf.collect::<Vec<u8>>()) {
            Some(MavFrame {header, msg, protocol_version: version })
        } else {
            None
        }
    }

    /// Return the frame header
    pub fn header(&self) -> MavHeader {
        self.header
    }
}

pub fn read_versioned_msg<R: Read>(r: &mut R, version: MavlinkVersion) -> Result<(MavHeader, MavMessage)> {
    match version {
        MavlinkVersion::V2 => {
            read_v2_msg(r)
        },
        MavlinkVersion::V1 => {
            read_v1_msg(r)
        }
    }
}

/// Read a MAVLink v1  message from a Read stream.
pub fn read_v1_msg<R: Read>(r: &mut R) -> Result<(MavHeader, MavMessage)> {
    loop {
        if r.read_u8()? != MAV_STX {
            continue;
        }
        let len = r.read_u8()? as usize;
        let seq = r.read_u8()?;
        let sysid = r.read_u8()?;
        let compid = r.read_u8()?;
        let msgid = r.read_u8()?;

        let mut payload_buf = [0; 255];
        let payload = &mut payload_buf[..len];

        r.read_exact(payload)?;

        let crc = r.read_u16::<LittleEndian>()?;

        let mut crc_calc = crc16::State::<crc16::MCRF4XX>::new();
        crc_calc.update(&[len as u8, seq, sysid, compid, msgid]);
        crc_calc.update(payload);
        crc_calc.update(&[MavMessage::extra_crc(msgid.into() )]);
        let recvd_crc = crc_calc.get();
        if recvd_crc != crc {
            // bad crc: ignore message
            //println!("msg id {} len {} , crc got {} expected {}", msgid, len, crc, recvd_crc );
            continue;
        }

        if let Some(msg) = MavMessage::parse(MavlinkVersion::V1, msgid.into(), payload) {
            return Ok((
                MavHeader {
                    sequence: seq,
                    system_id: sysid,
                    component_id: compid,
                },
                msg,
            ));
        }
    }
}

const MAVLINK_IFLAG_SIGNED: u8 = 0x01;

/// Read a MAVLink v2  message from a Read stream.
pub fn read_v2_msg<R: Read>(r: &mut R) -> Result<(MavHeader, MavMessage)> {
    loop {
        // search for the magic framing value indicating start of mavlink message
        if r.read_u8()? != MAV_STX_V2 {
            continue;
        }

//        println!("Got STX2");
        let payload_len = r.read_u8()? as usize;
//        println!("Got payload_len: {}", payload_len);
        let incompat_flags = r.read_u8()?;
//        println!("Got incompat flags: {}", incompat_flags);
        let compat_flags = r.read_u8()?;
//        println!("Got compat flags: {}", compat_flags);

        let seq = r.read_u8()?;
//        println!("Got seq: {}", seq);

        let sysid = r.read_u8()?;
//        println!("Got sysid: {}", sysid);

        let compid = r.read_u8()?;
//        println!("Got compid: {}", compid);

        let mut msgid_buf = [0;4];
        msgid_buf[0] = r.read_u8()?;
        msgid_buf[1] = r.read_u8()?;
        msgid_buf[2] = r.read_u8()?;

        let header_buf = &[payload_len as u8,
            incompat_flags, compat_flags,
            seq, sysid, compid,
            msgid_buf[0],msgid_buf[1],msgid_buf[2]];

        let msgid: u32 = u32::from_le_bytes(msgid_buf);
//        println!("Got msgid: {}", msgid);

        //provide a buffer that is the maximum payload size
        let mut payload_buf = [0; 255];
        let payload = &mut payload_buf[..payload_len];

        r.read_exact(payload)?;

        let crc = r.read_u16::<LittleEndian>()?;

        if (incompat_flags & 0x01) == MAVLINK_IFLAG_SIGNED {
            let mut sign = [0;13];
            r.read_exact(&mut sign)?;
        }

        let mut crc_calc = crc16::State::<crc16::MCRF4XX>::new();
        crc_calc.update(header_buf);
        crc_calc.update(payload);
        let extra_crc = MavMessage::extra_crc(msgid);

        crc_calc.update(&[extra_crc]);
        let recvd_crc = crc_calc.get();
        if recvd_crc != crc {
            // bad crc: ignore message
            // println!("msg id {} payload_len {} , crc got {} expected {}", msgid, payload_len, crc, recvd_crc );
            continue;
        }

        if let Some(msg) = MavMessage::parse(MavlinkVersion::V2, msgid, payload) {
            return Ok((
                MavHeader {
                    sequence: seq,
                    system_id: sysid,
                    component_id: compid,
                },
                msg,
            ));
        }
        else {
            return Err(
                std::io::Error::new( std::io::ErrorKind::InvalidData, "Invalid MavMessage")
            );
        }
    }
}


/// Write a message using the given mavlink version
pub fn write_versioned_msg<W: Write>(w: &mut W,  version: MavlinkVersion,
                                     header: MavHeader, data: &MavMessage) -> Result<()> {
    match version {
        MavlinkVersion::V2 => {
            write_v2_msg(w, header, data)
        },
        MavlinkVersion::V1 => {
            write_v1_msg(w, header, data)
        }
    }
}

/// Write a MAVLink v2 message to a Write stream.
pub fn write_v2_msg<W: Write>(w: &mut W, header: MavHeader, data: &MavMessage) -> Result<()> {
    let msgid = data.message_id();
    let payload = data.ser();
//    println!("write payload_len : {}", payload.len());

    let header = &[
        MAV_STX_V2,
        payload.len() as u8,
        0, //incompat_flags
        0, //compat_flags
        header.sequence,
        header.system_id,
        header.component_id,
        (msgid & 0x0000FF) as u8,
        ((msgid & 0x00FF00) >> 8) as u8 ,
        ((msgid & 0xFF0000) >> 16) as u8,
    ];

//    println!("write H: {:?}",header );

    let mut crc = crc16::State::<crc16::MCRF4XX>::new();
    crc.update(&header[1..]);
//    let header_crc = crc.get();
    crc.update(&payload[..]);
//    let base_crc = crc.get();
    let extra_crc = MavMessage::extra_crc(msgid);
//    println!("write header_crc: {} base_crc: {} extra_crc: {}",
//             header_crc, base_crc, extra_crc);
    crc.update(&[extra_crc]);

    w.write_all(header)?;
    w.write_all(&payload[..])?;
    w.write_u16::<LittleEndian>(crc.get())?;

    Ok(())
}

/// Write a MAVLink v1 message to a Write stream.
pub fn write_v1_msg<W: Write>(w: &mut W, header: MavHeader, data: &MavMessage) -> Result<()> {
    let msgid = data.message_id();
    let payload = data.ser();

    let header = &[
        MAV_STX,
        payload.len() as u8,
        header.sequence,
        header.system_id,
        header.component_id,
        msgid as u8,
    ];

    let mut crc = crc16::State::<crc16::MCRF4XX>::new();
    crc.update(&header[1..]);
    crc.update(&payload[..]);
    crc.update(&[MavMessage::extra_crc(msgid)]);

    w.write_all(header)?;
    w.write_all(&payload[..])?;
    w.write_u16::<LittleEndian>(crc.get())?;

    Ok(())
}







