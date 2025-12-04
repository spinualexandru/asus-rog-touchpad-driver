use crate::error::{DriverError, Result};
use log::{debug, info};
use regex::Regex;
use std::fs;
use std::thread;
use std::time::Duration;

const PROC_DEVICES_PATH: &str = "/proc/bus/input/devices";

/// Information about a detected input device
#[derive(Debug, Clone)]
pub struct InputDeviceInfo {
    pub name: String,
    pub event_path: String,
    pub i2c_bus: Option<u8>,
}

/// Information about detected devices needed by the driver
#[derive(Debug)]
pub struct DetectedDevices {
    pub touchpad: InputDeviceInfo,
    pub keyboard: InputDeviceInfo,
    pub i2c_address: u8,
}

/// Parse /proc/bus/input/devices to find touchpad and keyboard
pub fn detect_devices(try_times: u32, try_sleep: Duration) -> Result<DetectedDevices> {
    let touchpad_name_re = Regex::new(r#"Name="(ASUE|ASUF|ELAN).*Touchpad"#).unwrap();
    let keyboard_name_re =
        Regex::new(r#"Name="(AT Translated Set 2 keyboard|Asus Keyboard)"#).unwrap();
    let i2c_bus_re = Regex::new(r"i2c-(\d+)/").unwrap();
    let event_re = Regex::new(r"event(\d+)").unwrap();

    for attempt in 0..try_times {
        if attempt > 0 {
            debug!("Device detection attempt {}/{}", attempt + 1, try_times);
            thread::sleep(try_sleep);
        }

        let content = fs::read_to_string(PROC_DEVICES_PATH).map_err(|e| {
            DriverError::ParseError(format!("Cannot read {}: {}", PROC_DEVICES_PATH, e))
        })?;

        let mut touchpad: Option<InputDeviceInfo> = None;
        let mut keyboard: Option<InputDeviceInfo> = None;

        // Parse device blocks (separated by blank lines)
        for block in content.split("\n\n") {
            let lines: Vec<&str> = block.lines().collect();

            // Check if this is a touchpad
            if touchpad.is_none() {
                if let Some(name_line) = lines.iter().find(|l| l.starts_with("N: ")) {
                    if touchpad_name_re.is_match(name_line) {
                        let mut info = InputDeviceInfo {
                            name: extract_name(name_line),
                            event_path: String::new(),
                            i2c_bus: None,
                        };

                        // Extract I2C bus from sysfs line
                        if let Some(sysfs_line) = lines.iter().find(|l| l.starts_with("S: ")) {
                            if let Some(caps) = i2c_bus_re.captures(sysfs_line) {
                                info.i2c_bus = caps.get(1).and_then(|m| m.as_str().parse().ok());
                            }
                        }

                        // Extract event number from handlers line
                        if let Some(handlers_line) = lines.iter().find(|l| l.starts_with("H: ")) {
                            if let Some(caps) = event_re.captures(handlers_line) {
                                if let Some(event_num) = caps.get(1).map(|m| m.as_str()) {
                                    info.event_path = format!("/dev/input/event{}", event_num);
                                }
                            }
                        }

                        if !info.event_path.is_empty() {
                            debug!("Found touchpad: {} at {}", info.name, info.event_path);
                            touchpad = Some(info);
                        }
                    }
                }
            }

            // Check if this is a keyboard
            if keyboard.is_none() {
                if let Some(name_line) = lines.iter().find(|l| l.starts_with("N: ")) {
                    if keyboard_name_re.is_match(name_line) {
                        let mut info = InputDeviceInfo {
                            name: extract_name(name_line),
                            event_path: String::new(),
                            i2c_bus: None,
                        };

                        if let Some(handlers_line) = lines.iter().find(|l| l.starts_with("H: ")) {
                            if let Some(caps) = event_re.captures(handlers_line) {
                                if let Some(event_num) = caps.get(1).map(|m| m.as_str()) {
                                    info.event_path = format!("/dev/input/event{}", event_num);
                                }
                            }
                        }

                        if !info.event_path.is_empty() {
                            debug!("Found keyboard: {} at {}", info.name, info.event_path);
                            keyboard = Some(info);
                        }
                    }
                }
            }

            if touchpad.is_some() && keyboard.is_some() {
                break;
            }
        }

        if let (Some(tp), Some(kb)) = (touchpad, keyboard) {
            // Determine I2C address based on device name
            // ASUF1416, ASUF1205, ASUF1204 use address 0x38
            let i2c_address = if tp.name.contains("ASUF") {
                0x38
            } else {
                0x15
            };

            info!("Detected touchpad: {} (I2C addr: 0x{:02x})", tp.name, i2c_address);
            info!("Detected keyboard: {}", kb.name);

            return Ok(DetectedDevices {
                touchpad: tp,
                keyboard: kb,
                i2c_address,
            });
        }
    }

    Err(DriverError::DetectionTimeout(try_times))
}

fn extract_name(name_line: &str) -> String {
    name_line
        .trim_start_matches("N: Name=\"")
        .trim_end_matches('"')
        .to_string()
}
