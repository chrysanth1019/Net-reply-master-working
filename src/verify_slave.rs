use tokio::time;
use bytes::BytesMut;
use serde_json;
use log::{debug, error};
use std::sync::Arc;

use crate::packet::{
    parse_command_header,
    build_check_url_command,
    build_check_version_command,
    CommandPayload,
};
use crate::proxy::Slave;

use crate::buffer_pool::MAX_BUF_SIZE;
use crate::utils::CLIENT_REQUEST_TIMEOUT;

const ALLOWED_SLAVE_VERSIONS: &[&str] = &["1.0.9"];

pub async fn perform_version_check(temp_slave: &mut Slave) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut buffer = BytesMut::with_capacity(MAX_BUF_SIZE);
    let version_command = build_check_version_command();

    if let Err(e) = temp_slave.write_stream(&version_command).await {
        error!("Failed to send version check command to slave {}: {}", temp_slave.ip_addr, e);
        return Err(format!("Failed to send version check command: {}", e).into());
    }

    match time::timeout(CLIENT_REQUEST_TIMEOUT, temp_slave.read_stream(&mut buffer)).await {
        Ok(Ok(len)) if len > 2 => {
            let (_, _, payload) = parse_command_header(&buffer[..len]);
            let response_payload = CommandPayload::from_bytes(payload)?;
            if let Some(version) = response_payload.version {
                if !ALLOWED_SLAVE_VERSIONS.contains(&version.as_str()) {
                    return Err(format!(
                        "Slave {} has unsupported version: {}",
                        temp_slave.ip_addr, version
                    )
                    .into());
                }
                temp_slave.set_version(version.clone());
                debug!("Slave {} passed version check: {}", temp_slave.ip_addr, version);
            } else {
                return Err("Version field missing in response".into());
            }
        }
        Ok(Ok(_)) => {
            return Err("Version check response is invalid or empty".into());
        }
        Ok(Err(e)) => {
            return Err(format!("Version check read error: {}", e).into());
        }
        Err(_) => {
            return Err("Version check response timed out".into());
        }
    }

    Ok(())
}

pub async fn perform_geolocation_check(
    temp_slave: &mut Slave,
    allowed_locations: &Arc<Vec<String>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut buffer = BytesMut::with_capacity(MAX_BUF_SIZE);
    let check_url = format!("https://ipinfo.io/widget/demo/{}", temp_slave.ip_addr);
    let location_command = build_check_url_command("geolocation", &check_url.as_str());

    if let Err(e) = temp_slave.write_stream(&location_command).await {
        error!("Failed to send geolocation check command to slave {}: {}", temp_slave.ip_addr, e);
        return Err(format!("Failed to send geolocation check command: {}", e).into());
    }

    match time::timeout(CLIENT_REQUEST_TIMEOUT, temp_slave.read_stream(&mut buffer)).await {
        Ok(Ok(len)) if len > 2 => {
            let (_, _, payload) = parse_command_header(&buffer[..len]);
            let response_payload = CommandPayload::from_bytes(payload)?;
            if let Some(location_data) = response_payload.url {
                let location: serde_json::Value = serde_json::from_str(&location_data)?;
                if let Some(country) = location["data"]["country"].as_str() {
                    temp_slave.set_location(country.to_string());

                    if !allowed_locations.is_empty()
                        && !allowed_locations.iter().any(|loc| loc.eq_ignore_ascii_case(country))
                    {
                        return Err(format!(
                            "Slave {} is in a restricted location: {}",
                            temp_slave.ip_addr, country
                        )
                        .into());
                    }
                    debug!("Slave {} passed location check: {}", temp_slave.ip_addr, country);
                } else {
                    return Err("Missing country field in location response".into());
                }
            } else {
                return Err("Location field missing in response".into());
            }
        }
        Ok(Ok(_)) => {
            return Err("Location check response is invalid or empty".into());
        }
        Ok(Err(e)) => {
            return Err(format!("Location check read error: {}", e).into());
        }
        Err(_) => {
            return Err("Location check response timed out".into());
        }
    }

    Ok(())
}

pub async fn perform_speed_test(temp_slave: &mut Slave) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut buffer = BytesMut::with_capacity(MAX_BUF_SIZE);
    let speed_test_command = build_check_url_command(
        "speed_test",
        "https://speed.cloudflare.com/__down?bytes=5000000",
    );

    if let Err(e) = temp_slave.write_stream(&speed_test_command).await {
        error!("Failed to send speed test command to slave {}: {}", temp_slave.ip_addr, e);
        return Err(format!("Failed to send speed test command: {}", e).into());
    }

    match time::timeout(CLIENT_REQUEST_TIMEOUT, temp_slave.read_stream(&mut buffer)).await {
        Ok(Ok(len)) if len > 2 => {
            let (_, _, payload) = parse_command_header(&buffer[..len]);
            let response_payload = CommandPayload::from_bytes(payload).map_err(|e| {
                error!("Failed to deserialize payload: {}", e);
                e
            })?;
            if let Some(speed_data) = response_payload.url {
                match speed_data.parse::<f64>() {
                    Ok(speed) => {
                        temp_slave.set_speed(speed);
                        debug!("Slave {} passed speed test: {:.2} Mbps", temp_slave.ip_addr, speed);
                    }
                    Err(e) => {
                        error!("Failed to parse speed data: {}", e);
                        return Err(e.into());
                    }
                }
            } else {
                return Err("Speed field missing in response".into());
            }
        }
        Ok(Ok(_)) => {
            return Err("Speed test response is invalid or empty".into());
        }
        Ok(Err(e)) => {
            return Err(format!("Speed test read error: {}", e).into());
        }
        Err(_) => {
            return Err("Speed test response timed out".into());
        }
    }

    Ok(())
}