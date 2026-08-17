use crate::i2c::Brightness;
use evdev::KeyCode;

/// Touch position in normalized coordinates (0.0 - 1.0)
#[derive(Debug, Clone, Copy, Default)]
pub struct TouchPosition {
    pub x: f64,
    pub y: f64,
}

/// Corner detection zones
///
/// Both zones are resolved by the active layout (`is_toggle_position` /
/// `is_calc_position`), not here — the geometry has to agree with the layout's
/// key bands, and only the layout knows those.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    TopRight, // Numpad toggle
    TopLeft,  // Calculator/brightness
    None,
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
        self.current_position.x = normalize_axis(value, min_x, max_x);
    }

    /// Update Y position from raw touchpad value
    pub fn update_y(&mut self, value: i32, min_y: i32, max_y: i32) {
        self.current_position.y = normalize_axis(value, min_y, max_y);
    }

    /// Cycle to next brightness level
    pub fn cycle_brightness(&mut self) {
        self.brightness = self.brightness.next();
    }
}

/// Maps a raw device value onto the 0.0–1.0 fraction everything downstream works in.
pub fn normalize_axis(value: i32, min: i32, max: i32) -> f64 {
    if max <= min {
        return 0.0;
    }

    ((value - min) as f64 / (max - min) as f64).clamp(0.0, 1.0)
}

impl Default for NumpadState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_and_clamps_axis_values() {
        assert_eq!(normalize_axis(50, 0, 100), 0.5);
        assert_eq!(normalize_axis(-10, 0, 100), 0.0);
        assert_eq!(normalize_axis(110, 0, 100), 1.0);
        assert_eq!(normalize_axis(1, 1, 1), 0.0);
    }

    #[test]
    fn cycles_brightness_without_ever_landing_on_off() {
        // Off is reachable only via turn_off() on disable; cycling must not dim
        // the pad to invisible while the numpad is still enabled.
        let mut state = NumpadState::new();
        for _ in 0..8 {
            state.cycle_brightness();
            assert_ne!(state.brightness, Brightness::Off);
        }
    }
}
