use crate::i2c::Brightness;
use crate::layouts::NumpadLayout;
use evdev::KeyCode;

/// Touch position in normalized coordinates (0.0 - 1.0)
#[derive(Debug, Clone, Copy, Default)]
pub struct TouchPosition {
    pub x: f64,
    pub y: f64,
}

/// Corner detection zones
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    TopRight, // Numpad toggle
    TopLeft,  // Calculator/brightness
    None,
}

impl TouchPosition {
    pub fn corner(&self) -> Corner {
        // Top-right: x > 80%, y < 25%
        if self.x > 0.80 && self.y < 0.25 {
            Corner::TopRight
        }
        // Top-left: x < 6%, y < 7%
        else if self.x < 0.06 && self.y < 0.07 {
            Corner::TopLeft
        } else {
            Corner::None
        }
    }
}

/// State machine for numpad operation
pub struct NumpadState {
    pub enabled: bool,
    pub brightness: Brightness,
    pub current_position: TouchPosition,
    pub pressed_key: Option<KeyCode>,
}

impl NumpadState {
    pub fn new() -> Self {
        Self {
            enabled: false,
            brightness: Brightness::High, // Start at full brightness
            current_position: TouchPosition::default(),
            pressed_key: None,
        }
    }

    /// Update X position from raw touchpad value
    pub fn update_x(&mut self, value: i32, min_x: i32, max_x: i32) {
        self.current_position.x = (value - min_x) as f64 / (max_x - min_x + 1) as f64;
    }

    /// Update Y position from raw touchpad value
    pub fn update_y(&mut self, value: i32, min_y: i32, max_y: i32) {
        self.current_position.y = (value - min_y) as f64 / (max_y - min_y) as f64;
    }

    /// Calculate grid position from current touch coordinates
    pub fn grid_position(&self, layout: &dyn NumpadLayout) -> Option<(u32, u32)> {
        let cols = layout.cols() as f64;
        let rows = layout.rows() as f64;
        let top_offset = layout.top_offset();

        let col = (cols * self.current_position.x).floor() as i32;
        let row_f = (rows * self.current_position.y) - top_offset;

        // Ignore top_offset region (negative row)
        if row_f < 0.0 {
            return None;
        }

        let row = row_f.floor() as i32;

        // Bounds check
        if col >= 0 && col < layout.cols() as i32 && row >= 0 && row < layout.rows() as i32 {
            Some((row as u32, col as u32))
        } else {
            None
        }
    }

    /// Cycle to next brightness level
    pub fn cycle_brightness(&mut self) {
        self.brightness = self.brightness.next();
    }
}

impl Default for NumpadState {
    fn default() -> Self {
        Self::new()
    }
}
