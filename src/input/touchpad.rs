use evdev::{AbsoluteAxisCode, Device};
use log::debug;
use std::io;
use std::os::fd::{AsFd, BorrowedFd};
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

        let x_axis = if device
            .supported_absolute_axes()
            .is_some_and(|axes| axes.contains(AbsoluteAxisCode::ABS_MT_POSITION_X))
        {
            AbsoluteAxisCode::ABS_MT_POSITION_X
        } else {
            AbsoluteAxisCode::ABS_X
        };
        let y_axis = if device
            .supported_absolute_axes()
            .is_some_and(|axes| axes.contains(AbsoluteAxisCode::ABS_MT_POSITION_Y))
        {
            AbsoluteAxisCode::ABS_MT_POSITION_Y
        } else {
            AbsoluteAxisCode::ABS_Y
        };

        // Index into the array using the same axis type read by the event loop.
        let x_idx = x_axis.0 as usize;
        let y_idx = y_axis.0 as usize;

        let x_info = &abs_state[x_idx];
        let y_info = &abs_state[y_idx];

        if x_info.maximum <= x_info.minimum || y_info.maximum <= y_info.minimum {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "touchpad reported invalid absolute axis bounds",
            ));
        }

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

    /// The device fd, for waiting on it alongside something else.
    ///
    /// Borrowed rather than owned on purpose: the event loop only ever hands it to
    /// `poll()`, and closing it out from under the reader would drop the grab.
    pub fn as_fd(&self) -> BorrowedFd<'_> {
        self.device.as_fd()
    }

    /// Puts the device fd in (non-)blocking mode.
    ///
    /// `Device::open` leaves it blocking. A loop that waits in `poll()` must set
    /// this, or a wakeup that turns out to carry no events parks the process in
    /// `read()` anyway — the very stall the poll was there to avoid.
    pub fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
        self.device.set_nonblocking(nonblocking)
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

    /// Fetch events and collect them into a Vec to avoid borrow issues.
    ///
    /// The error is returned unwrapped so callers keep its `ErrorKind`: the event
    /// loop distinguishes `Interrupted` (a signal landed, check for shutdown) and
    /// `WouldBlock` (non-blocking fd with nothing queued) from a genuinely broken
    /// device.
    pub fn fetch_events(&mut self) -> io::Result<Vec<evdev::InputEvent>> {
        self.device.fetch_events().map(|iter| iter.collect())
    }
}
