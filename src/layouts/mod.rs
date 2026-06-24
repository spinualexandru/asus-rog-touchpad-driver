mod g634jy;

use crate::error::{DriverError, Result};
use evdev::KeyCode;
use std::sync::Arc;

pub use g634jy::G634jyLayout;

/// Trait defining a numpad layout
#[allow(dead_code)]
pub trait NumpadLayout: Send + Sync {
    /// Layout name for identification
    fn name(&self) -> &'static str;

    /// Number of columns in the grid
    fn cols(&self) -> u32;

    /// Number of rows in the grid
    fn rows(&self) -> u32;

    /// Vertical offset as fraction (0.0 - 1.0) to skip top control area
    fn top_offset(&self) -> f64;

    /// Get the key at the given grid position
    /// Returns None if position is invalid
    fn key_at(&self, row: u32, col: u32) -> Option<KeyCode>;

    /// Returns true when the normalized position is inside the numpad toggle zone.
    fn is_toggle_position(&self, x: f64, y: f64) -> bool {
        x > 0.80 && y < 0.25
    }

    /// Get the key at the given normalized touchpad position.
    fn key_at_position(&self, x: f64, y: f64) -> Option<KeyCode> {
        let cols = self.cols() as f64;
        let rows = self.rows() as f64;
        if cols == 0.0 || rows == 0.0 {
            return None;
        }

        let top_offset = self.top_offset().clamp(0.0, 0.95);
        if y < top_offset {
            return None;
        }

        let col = scaled_index(x, cols);
        let row = scaled_index((y - top_offset) / (1.0 - top_offset), rows);
        if col >= 0 && col < self.cols() as i32 && row >= 0 && row < self.rows() as i32 {
            self.key_at(row as u32, col as u32)
        } else {
            None
        }
    }

    /// All keys used by this layout (for enabling in virtual device)
    fn all_keys(&self) -> Vec<KeyCode>;

    /// Number of detection retry attempts
    fn try_times(&self) -> u32 {
        5
    }

    /// Sleep duration between retry attempts in milliseconds
    fn try_sleep_ms(&self) -> u64 {
        100
    }
}

fn scaled_index(value: f64, count: f64) -> i32 {
    (count * value.clamp(0.0, 1.0)).floor().min(count - 1.0) as i32
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

        fn key_at(&self, row: u32, col: u32) -> Option<KeyCode> {
            match (row, col) {
                (0, 2) => Some(KeyCode::KEY_KP2),
                (3, 4) => Some(KeyCode::KEY_BACKSPACE),
                _ => None,
            }
        }

        fn all_keys(&self) -> Vec<KeyCode> {
            Vec::new()
        }
    }

    #[test]
    fn default_position_lookup_skips_top_offset_and_scales_grid() {
        let layout = TestLayout;

        assert_eq!(layout.key_at_position(0.5, 0.09), None);
        assert_eq!(layout.key_at_position(0.5, 0.10), Some(KeyCode::KEY_KP2));
        assert_eq!(
            layout.key_at_position(0.99, 0.99),
            Some(KeyCode::KEY_BACKSPACE)
        );
        assert_eq!(
            layout.key_at_position(1.0, 1.0),
            Some(KeyCode::KEY_BACKSPACE)
        );
    }
}

/// Get a layout by name
pub fn get_layout(name: &str) -> Result<Arc<dyn NumpadLayout>> {
    match name.to_lowercase().as_str() {
        "g634jy" | "g634jyr" => Ok(Arc::new(G634jyLayout::new())),
        _ => Err(DriverError::LayoutNotFound(name.to_string())),
    }
}
