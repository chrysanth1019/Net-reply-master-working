use crate::proxy::{ProxyManager, Slave};
use crate::utils::bytes_to_u32;

use bytes::{BufMut, Bytes, BytesMut};
use log::{debug, error};
use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PacketType {
    Data = 0x00,
    Command = 0x01,
}

impl PacketType {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x00 => Some(PacketType::Data),
            0x01 => Some(PacketType::Command),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum CommandType {
    SpeedCheck = 0x01,
    VersionCheck = 0x02,
    Heartbeat = 0x03,
    LocationCheck = 0x04,
    InitSession = 0x05,
}

impl CommandType {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(CommandType::SpeedCheck),
            0x02 => Some(CommandType::VersionCheck),
            0x03 => Some(CommandType::Heartbeat),
            0x04 => Some(CommandType::LocationCheck),
            0x05 => Some(CommandType::InitSession),
            _ => None,
        }
    }
}

fn build_command_frame(
    packet_type: PacketType,
    session_id: u32,
    command_type: Option<CommandType>,
    payload: &[u8],
) -> Bytes {
    debug!(
        "Building frame: packet_type={:?}, session_id={}, command_type={:?}, payload_len={}",
        packet_type,
        session_id,
        command_type,
        payload.len()
    );

    let mut frame = BytesMut::with_capacity(10 + payload.len());
    frame.put_u8(packet_type as u8);
    frame.put_u32(session_id);

    if let Some(cmd) = command_type {
        frame.put_u8(cmd as u8);
    } else {
        frame.put_u8(0x00);
    }

    frame.put_u32(payload.len() as u32);
    frame.put_slice(payload);
    frame.freeze()
}

pub fn build_speed_test_command(url: &str) -> Bytes {
    debug!("Building speed test command for URL: {}", url);
    build_command_frame(
        PacketType::Command,
        0,
        Some(CommandType::SpeedCheck),
        url.as_bytes(),
    )
}

pub fn build_version_check_command() -> Bytes {
    debug!("Building version check command");
    build_command_frame(PacketType::Command, 0, Some(CommandType::VersionCheck), &[])
}

pub fn build_heartbeat_command() -> Bytes {
    debug!("Building heartbeat command");
    build_command_frame(PacketType::Command, 0, Some(CommandType::Heartbeat), &[])
}

pub fn build_init_session_command(session_id: u32, payload: &str) -> Bytes {
    debug!(
        "Building init session command: session_id={}, payload={}",
        session_id, payload
    );
    build_command_frame(
        PacketType::Command,
        session_id,
        Some(CommandType::InitSession),
        payload.as_bytes(),
    )
}

pub fn build_location_check_command(ip: &str) -> Bytes {
    debug!("Building location check command for IP: {}", ip);
    build_command_frame(
        PacketType::Command,
        0,
        Some(CommandType::LocationCheck),
        format!("https://ipinfo.io/widget/demo/{}", ip).as_bytes(),
    )
}

pub fn build_data_frame(session_id: u32, payload: &[u8]) -> Bytes {
    debug!(
        "Building data frame: session_id={}, payload_len={}",
        session_id,
        payload.len()
    );
    build_command_frame(PacketType::Data, session_id, None, payload)
}

pub fn parse_header(buffer: &[u8]) -> (Option<PacketType>, u32, usize, Option<CommandType>) {
    debug!("Parsing header from buffer with length: {}", buffer.len());

    if buffer.len() < 10 {
        debug!("Buffer too short to parse header");
        return (None, 0, 0, None);
    }

    let packet_type = PacketType::from_u8(buffer[0]);
    let session_id = bytes_to_u32(&buffer[1..5]);
    let payload_len = u32::from_be_bytes([buffer[6], buffer[7], buffer[8], buffer[9]]) as usize;

    // Check for CommandType if the packet is a command
    let command_type = if packet_type == Some(PacketType::Command) {
        CommandType::from_u8(buffer[5])
    } else {
        None
    };

    debug!(
        "Parsed header: packet_type={:?}, session_id={}, payload_len={}, command_type={:?}",
        packet_type, session_id, payload_len, command_type
    );

    (packet_type, session_id, payload_len, command_type)
}

pub async fn process_packet(
    packet_type: Option<PacketType>,
    command_type: Option<CommandType>,
    payload: Bytes,
    session_id: u32,
    slave: &Slave,
    proxy_manager: &Arc<AsyncMutex<ProxyManager>>,
    last_seen: &mut Instant,
) -> Result<(), std::io::Error> {
    debug!(
        "Processing packet: packet_type={:?}, command_type={:?}, session_id={}, payload_len={}",
        packet_type,
        command_type,
        session_id,
        payload.len()
    );

    match packet_type {
        Some(PacketType::Command) => {
            match command_type {
                Some(CommandType::Heartbeat) => {
                    // Update last_seen on valid heartbeat response
                    if payload.as_ref() == b"ALIVE" {
                        *last_seen = Instant::now();
                        debug!("Received heartbeat response from slave {}", slave.ip_addr);
                    } else {
                        debug!(
                            "Invalid heartbeat response from slave {}: {:?}",
                            slave.ip_addr, payload
                        );
                    }
                }
                _ => debug!(
                    "Ignoring unsupported command packet from slave {}: {:?}",
                    slave.ip_addr, command_type
                ),
            }
        }
        Some(PacketType::Data) => {
            debug!(
                "Routing data payload from slave {}: sid={}, bytes={}",
                slave.ip_addr,
                session_id,
                payload.len()
            );
            proxy_manager
                .lock()
                .await
                .route_to_client(session_id, payload)
                .await;
        }
        None => {
            error!("Unknown PacketType received from slave: {}", slave.ip_addr);
        }
    }

    Ok(())
}
