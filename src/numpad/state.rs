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
        self.current_position.x = normalize_axis(value, min_x, max_x);
    }

    /// Update Y position from raw touchpad value
    pub fn update_y(&mut self, value: i32, min_y: i32, max_y: i32) {
        self.current_position.y = normalize_axis(value, min_y, max_y);
    }

    /// Calculate grid position from current touch coordinates
    pub fn grid_position(&self, layout: &dyn NumpadLayout) -> Option<(u32, u32)> {
        let cols = layout.cols() as f64;
        let rows = layout.rows() as f64;
        if cols == 0.0 || rows == 0.0 {
            return None;
        }

        let top_offset = layout.top_offset().clamp(0.0, 0.95);

        // Ignore the top control area before scaling the remaining area into rows.
        if self.current_position.y < top_offset {
            return None;
        }

        let col = scaled_index(self.current_position.x, cols);
        let row_f = rows * ((self.current_position.y - top_offset) / (1.0 - top_offset));
        let row = scaled_index(row_f / rows, rows);

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

fn normalize_axis(value: i32, min: i32, max: i32) -> f64 {
    if max <= min {
        return 0.0;
    }

    ((value - min) as f64 / (max - min) as f64).clamp(0.0, 1.0)
}

fn scaled_index(value: f64, count: f64) -> i32 {
    (count * value.clamp(0.0, 1.0)).floor().min(count - 1.0) as i32
}

impl Default for NumpadState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestLayout;

    impl NumpadLayout for TestLayout {
        fn name(&self) -> &'static str {
            "test"
        }

        fn cols(&self) -> u32 {
            5
        }

        fn rows(&self) -> u32 {
            4
        }

        fn top_offset(&self) -> f64 {
            0.10
        }

        fn key_at(&self, _row: u32, _col: u32) -> Option<KeyCode> {
            None
        }

        fn all_keys(&self) -> Vec<KeyCode> {
            Vec::new()
        }
    }

    #[test]
    fn normalizes_and_clamps_axis_values() {
        assert_eq!(normalize_axis(50, 0, 100), 0.5);
        assert_eq!(normalize_axis(-10, 0, 100), 0.0);
        assert_eq!(normalize_axis(110, 0, 100), 1.0);
        assert_eq!(normalize_axis(1, 1, 1), 0.0);
    }

    #[test]
    fn top_offset_skips_fraction_of_touchpad_height() {
        let layout = TestLayout;
        let mut state = NumpadState::new();

        state.current_position = TouchPosition { x: 0.5, y: 0.09 };
        assert_eq!(state.grid_position(&layout), None);

        state.current_position = TouchPosition { x: 0.5, y: 0.10 };
        assert_eq!(state.grid_position(&layout), Some((0, 2)));

        state.current_position = TouchPosition { x: 0.99, y: 0.99 };
        assert_eq!(state.grid_position(&layout), Some((3, 4)));

        state.current_position = TouchPosition { x: 1.0, y: 1.0 };
        assert_eq!(state.grid_position(&layout), Some((3, 4)));
    }

    #[test]
    fn corner_detection_uses_expected_zones() {
        assert_eq!(
            TouchPosition { x: 0.90, y: 0.10 }.corner(),
            Corner::TopRight
        );
        assert_eq!(TouchPosition { x: 0.05, y: 0.05 }.corner(), Corner::TopLeft);
        assert_eq!(TouchPosition { x: 0.50, y: 0.50 }.corner(), Corner::None);
    }
}
