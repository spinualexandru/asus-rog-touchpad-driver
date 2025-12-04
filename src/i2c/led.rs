use i2c_linux::I2c;
use log::{debug, warn};
use std::fs::File;
use std::io;

/// Brightness levels matching the Python driver
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Brightness {
    Off,
    High,   // Brightest (0x01)
    Medium, // Medium (0x18)
    Low,    // Dimmest (0x1f)
}

impl Brightness {
    /// Get the byte value for this brightness level
    pub fn as_byte(self) -> u8 {
        match self {
            Brightness::Off => 0x00,
            Brightness::High => 0x01,
            Brightness::Medium => 0x18,
            Brightness::Low => 0x1f,
        }
    }

    /// Cycle to next brightness level
    pub fn next(self) -> Self {
        match self {
            Brightness::Low => Brightness::Medium,
            Brightness::Medium => Brightness::High,
            Brightness::High => Brightness::Low,
            Brightness::Off => Brightness::Low,
        }
    }
}

/// I2C LED controller for touchpad backlight
pub struct LedController {
    i2c: I2c<File>,
    address: u16,
}

impl LedController {
    /// Create a new LED controller for the given I2C bus
    pub fn new(bus_number: u8, address: u8) -> io::Result<Self> {
        let path = format!("/dev/i2c-{}", bus_number);
        debug!("Opening I2C device: {}", path);
        let i2c = I2c::from_path(&path)?;
        Ok(Self {
            i2c,
            address: address as u16,
        })
    }

    /// Set LED brightness
    /// Command format: 05 00 3d 03 06 00 07 00 0d 14 03 [brightness] ad
    pub fn set_brightness(&mut self, brightness: Brightness) -> io::Result<()> {
        let brightness_byte = brightness.as_byte();

        let command: [u8; 13] = [
            0x05, 0x00, 0x3d, 0x03, 0x06, 0x00, 0x07, 0x00, 0x0d, 0x14, 0x03, brightness_byte, 0xad,
        ];

        debug!(
            "Setting LED brightness to {:?} (0x{:02x})",
            brightness, brightness_byte
        );
        debug!(
            "I2C command: {}",
            command
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<Vec<_>>()
                .join(" ")
        );

        // Set the slave address
        self.i2c.smbus_set_slave_address(self.address, false)?;

        // Write the command using i2c_transfer
        use i2c_linux::{Message, WriteFlags};
        let mut messages = [Message::Write {
            address: self.address,
            data: &command,
            flags: WriteFlags::default(),
        }];

        self.i2c.i2c_transfer(&mut messages)?;

        debug!("LED brightness set successfully");
        Ok(())
    }

    /// Turn off LED (convenience method)
    pub fn turn_off(&mut self) -> io::Result<()> {
        self.set_brightness(Brightness::Off)
    }
}

/// Try to create an LED controller, logging warnings on failure
pub fn try_create_led_controller(bus: Option<u8>, address: u8) -> Option<LedController> {
    match bus {
        Some(bus_num) => match LedController::new(bus_num, address) {
            Ok(led) => Some(led),
            Err(e) => {
                warn!("LED control unavailable: {}", e);
                None
            }
        },
        None => {
            warn!("No I2C bus detected, LED control unavailable");
            None
        }
    }
}
