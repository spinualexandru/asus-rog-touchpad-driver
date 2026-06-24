use super::NumpadLayout;
use evdev::KeyCode;

const NUMERIC_COLUMNS: [(f64, f64); 3] = [(0.05, 0.22), (0.25, 0.40), (0.45, 0.55)];
const OPERATOR_COLUMN: (f64, f64) = (0.60, 0.75);
const RIGHT_COLUMN: (f64, f64) = (0.80, 0.95);
const ZERO_COLUMN: (f64, f64) = (0.05, 0.40);
const DOT_COLUMN: (f64, f64) = (0.45, 0.60);

const MAIN_ROWS: [(f64, f64); 4] = [(0.05, 0.25), (0.30, 0.50), (0.55, 0.75), (0.80, 0.95)];
const OPERATOR_ROWS: [(f64, f64); 4] = [(0.05, 0.30), (0.30, 0.55), (0.60, 0.75), (0.75, 0.95)];
const RIGHT_ROWS: [(f64, f64); 3] = [(0.00, 0.30), (0.30, 0.50), (0.55, 0.95)];

/// ROG Strix SCAR 16 G634JY / G634JYR layout
/// ASUF1416:00 2808:0108
/// LED backlight works using I2C address 0x38
pub struct G634jyLayout {
    keys: [[KeyCode; 5]; 4],
}

impl G634jyLayout {
    pub fn new() -> Self {
        Self {
            keys: [
                [
                    KeyCode::KEY_KP7,
                    KeyCode::KEY_KP8,
                    KeyCode::KEY_KP9,
                    KeyCode::KEY_KPSLASH,
                    KeyCode::KEY_BACKSPACE,
                ],
                [
                    KeyCode::KEY_KP4,
                    KeyCode::KEY_KP5,
                    KeyCode::KEY_KP6,
                    KeyCode::KEY_KPASTERISK,
                    KeyCode::KEY_BACKSPACE,
                ],
                [
                    KeyCode::KEY_KP1,
                    KeyCode::KEY_KP2,
                    KeyCode::KEY_KP3,
                    KeyCode::KEY_KPMINUS,
                    KeyCode::KEY_KPENTER,
                ],
                [
                    KeyCode::KEY_KP0,
                    KeyCode::KEY_KP0,
                    KeyCode::KEY_KPDOT,
                    KeyCode::KEY_KPPLUS,
                    KeyCode::KEY_KPENTER,
                ],
            ],
        }
    }
}

impl Default for G634jyLayout {
    fn default() -> Self {
        Self::new()
    }
}

fn in_band(value: f64, (start, end): (f64, f64)) -> bool {
    value >= start && (value < end || (end >= 1.0 && value <= end))
}

fn band_index(value: f64, bands: &[(f64, f64)]) -> Option<usize> {
    bands.iter().position(|band| in_band(value, *band))
}

impl NumpadLayout for G634jyLayout {
    fn name(&self) -> &'static str {
        "g634jy"
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
        self.keys
            .get(row as usize)
            .and_then(|r| r.get(col as usize))
            .copied()
    }

    fn is_toggle_position(&self, x: f64, y: f64) -> bool {
        in_band(x, RIGHT_COLUMN) && in_band(y, RIGHT_ROWS[0])
    }

    fn key_at_position(&self, x: f64, y: f64) -> Option<KeyCode> {
        if in_band(x, RIGHT_COLUMN) {
            return match band_index(y, &RIGHT_ROWS)? {
                0 => None,
                1 => Some(KeyCode::KEY_BACKSPACE),
                2 => Some(KeyCode::KEY_KPENTER),
                _ => None,
            };
        }

        if in_band(x, OPERATOR_COLUMN) {
            let row = band_index(y, &OPERATOR_ROWS)?;
            return Some(match row {
                0 => KeyCode::KEY_KPSLASH,
                1 => KeyCode::KEY_KPASTERISK,
                2 => KeyCode::KEY_KPMINUS,
                3 => KeyCode::KEY_KPPLUS,
                _ => return None,
            });
        }

        if in_band(y, MAIN_ROWS[3]) {
            if in_band(x, ZERO_COLUMN) {
                return Some(KeyCode::KEY_KP0);
            }
            if in_band(x, DOT_COLUMN) {
                return Some(KeyCode::KEY_KPDOT);
            }
        }

        let row = band_index(y, &MAIN_ROWS)?;
        let col = band_index(x, &NUMERIC_COLUMNS)?;
        Some(match row {
            0 => [KeyCode::KEY_KP7, KeyCode::KEY_KP8, KeyCode::KEY_KP9][col],
            1 => [KeyCode::KEY_KP4, KeyCode::KEY_KP5, KeyCode::KEY_KP6][col],
            2 => [KeyCode::KEY_KP1, KeyCode::KEY_KP2, KeyCode::KEY_KP3][col],
            _ => return None,
        })
    }

    fn all_keys(&self) -> Vec<KeyCode> {
        vec![
            KeyCode::KEY_KP0,
            KeyCode::KEY_KP1,
            KeyCode::KEY_KP2,
            KeyCode::KEY_KP3,
            KeyCode::KEY_KP4,
            KeyCode::KEY_KP5,
            KeyCode::KEY_KP6,
            KeyCode::KEY_KP7,
            KeyCode::KEY_KP8,
            KeyCode::KEY_KP9,
            KeyCode::KEY_KPDOT,
            KeyCode::KEY_KPENTER,
            KeyCode::KEY_KPPLUS,
            KeyCode::KEY_KPMINUS,
            KeyCode::KEY_KPASTERISK,
            KeyCode::KEY_KPSLASH,
            KeyCode::KEY_BACKSPACE,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_at(x: f64, y: f64) -> Option<KeyCode> {
        G634jyLayout::new().key_at_position(x, y)
    }

    #[test]
    fn maps_g634jy_photo_hitboxes() {
        assert_eq!(key_at(0.14, 0.15), Some(KeyCode::KEY_KP7));
        assert_eq!(key_at(0.32, 0.15), Some(KeyCode::KEY_KP8));
        assert_eq!(key_at(0.50, 0.15), Some(KeyCode::KEY_KP9));
        assert_eq!(key_at(0.14, 0.40), Some(KeyCode::KEY_KP4));
        assert_eq!(key_at(0.32, 0.40), Some(KeyCode::KEY_KP5));
        assert_eq!(key_at(0.50, 0.40), Some(KeyCode::KEY_KP6));
        assert_eq!(key_at(0.14, 0.65), Some(KeyCode::KEY_KP1));
        assert_eq!(key_at(0.32, 0.65), Some(KeyCode::KEY_KP2));
        assert_eq!(key_at(0.50, 0.65), Some(KeyCode::KEY_KP3));
        assert_eq!(key_at(0.14, 0.87), Some(KeyCode::KEY_KP0));
        assert_eq!(key_at(0.32, 0.87), Some(KeyCode::KEY_KP0));
    }

    #[test]
    fn maps_g634jy_operator_and_control_strip_hitboxes() {
        assert_eq!(key_at(0.67, 0.17), Some(KeyCode::KEY_KPSLASH));
        assert_eq!(key_at(0.67, 0.42), Some(KeyCode::KEY_KPASTERISK));
        assert_eq!(key_at(0.67, 0.67), Some(KeyCode::KEY_KPMINUS));
        assert_eq!(key_at(0.67, 0.85), Some(KeyCode::KEY_KPPLUS));
        assert_eq!(key_at(0.87, 0.15), None);
        assert_eq!(key_at(0.87, 0.40), Some(KeyCode::KEY_BACKSPACE));
        assert_eq!(key_at(0.87, 0.75), Some(KeyCode::KEY_KPENTER));
    }

    #[test]
    fn detects_g634jy_toggle_zone_separately_from_keys() {
        let layout = G634jyLayout::new();

        assert!(layout.is_toggle_position(0.87, 0.15));
        assert!(!layout.is_toggle_position(0.87, 0.40));
        assert_eq!(layout.key_at_position(0.87, 0.15), None);
    }

    #[test]
    fn maps_g634jy_wide_zero_and_dot() {
        assert_eq!(key_at(0.23, 0.87), Some(KeyCode::KEY_KP0));
        assert_eq!(key_at(0.50, 0.87), Some(KeyCode::KEY_KPDOT));
        assert_eq!(key_at(0.58, 0.87), Some(KeyCode::KEY_KPDOT));
    }

    #[test]
    fn leaves_g634jy_unlit_margins_dead() {
        assert_eq!(key_at(0.02, 0.15), None);
        assert_eq!(key_at(0.14, 0.98), None);
    }

    #[test]
    fn leaves_g634jy_separator_gaps_dead() {
        assert_eq!(key_at(0.23, 0.15), None);
        assert_eq!(key_at(0.42, 0.15), None);
        assert_eq!(key_at(0.57, 0.15), None);
        assert_eq!(key_at(0.77, 0.15), None);
        assert_eq!(key_at(0.14, 0.27), None);
        assert_eq!(key_at(0.14, 0.52), None);
        assert_eq!(key_at(0.14, 0.77), None);
        assert_eq!(key_at(0.87, 0.52), None);
    }
}
