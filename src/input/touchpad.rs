use evdev::{AbsoluteAxisCode, Device};
use log::debug;
use std::io;
use std::path::Path;

/// Touchpad dimensions from absinfo
#[derive(Debug, Clone, Copy)]
pub struct TouchpadBounds {
    pub min_x: i32,
    pub max_x: i32,
    pub min_y: i32,
    pub max_y: i32,
}

/// Touchpad input handler
pub struct TouchpadReader {
    device: Device,
    bounds: TouchpadBounds,
    grabbed: bool,
}

impl TouchpadReader {
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let device = Device::open(path.as_ref())?;

        // Get absolute axis state array
        let abs_state = device.get_abs_state()?;

        // Index into the array using axis type code
        let x_idx = AbsoluteAxisCode::ABS_X.0 as usize;
        let y_idx = AbsoluteAxisCode::ABS_Y.0 as usize;

        let x_info = &abs_state[x_idx];
        let y_info = &abs_state[y_idx];

        let bounds = TouchpadBounds {
            min_x: x_info.minimum,
            max_x: x_info.maximum,
            min_y: y_info.minimum,
            max_y: y_info.maximum,
        };

        debug!(
            "Touchpad bounds: x={}-{}, y={}-{}",
            bounds.min_x, bounds.max_x, bounds.min_y, bounds.max_y
        );

        Ok(Self {
            device,
            bounds,
            grabbed: false,
        })
    }

    pub fn bounds(&self) -> TouchpadBounds {
        self.bounds
    }

    /// Grab exclusive access to the touchpad
    pub fn grab(&mut self) -> io::Result<()> {
        if !self.grabbed {
            self.device
                .grab()
                .map_err(|e| io::Error::other(format!("Failed to grab device: {}", e)))?;
            self.grabbed = true;
            debug!("Touchpad grabbed");
        }
        Ok(())
    }

    /// Release exclusive access
    pub fn ungrab(&mut self) -> io::Result<()> {
        if self.grabbed {
            self.device
                .ungrab()
                .map_err(|e| io::Error::other(format!("Failed to ungrab device: {}", e)))?;
            self.grabbed = false;
            debug!("Touchpad ungrabbed");
        }
        Ok(())
    }

    /// Fetch events and collect them into a Vec to avoid borrow issues
    pub fn fetch_events(&mut self) -> io::Result<Vec<evdev::InputEvent>> {
        self.device
            .fetch_events()
            .map(|iter| iter.collect())
            .map_err(|e| io::Error::other(format!("Failed to fetch events: {}", e)))
    }
}
