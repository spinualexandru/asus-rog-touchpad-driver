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

/// Get a layout by name
pub fn get_layout(name: &str) -> Result<Arc<dyn NumpadLayout>> {
    match name.to_lowercase().as_str() {
        "g634jy" | "g634jyr" => Ok(Arc::new(G634jyLayout::new())),
        _ => Err(DriverError::LayoutNotFound(name.to_string())),
    }
}
