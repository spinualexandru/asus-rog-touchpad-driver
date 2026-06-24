use super::NumpadLayout;
use evdev::KeyCode;

const NUMERIC_COLUMNS: [(f64, f64); 3] = [(0.08, 0.25), (0.31, 0.43), (0.50, 0.63)];
const OPERATOR_COLUMN: (f64, f64) = (0.68, 0.77);
const RIGHT_COLUMN: (f64, f64) = (0.85, 1.00);

const MAIN_ROWS: [(f64, f64); 4] = [(0.18, 0.33), (0.39, 0.53), (0.60, 0.73), (0.81, 0.94)];
const RIGHT_ROWS: [(f64, f64); 3] = [(0.00, 0.32), (0.42, 0.58), (0.68, 0.94)];

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

        let row = band_index(y, &MAIN_ROWS)?;
        if in_band(x, OPERATOR_COLUMN) {
            return Some(match row {
                0 => KeyCode::KEY_KPSLASH,
                1 => KeyCode::KEY_KPASTERISK,
                2 => KeyCode::KEY_KPMINUS,
                3 => KeyCode::KEY_KPPLUS,
                _ => return None,
            });
        }

        let col = band_index(x, &NUMERIC_COLUMNS)?;
        Some(match row {
            0 => [KeyCode::KEY_KP7, KeyCode::KEY_KP8, KeyCode::KEY_KP9][col],
            1 => [KeyCode::KEY_KP4, KeyCode::KEY_KP5, KeyCode::KEY_KP6][col],
            2 => [KeyCode::KEY_KP1, KeyCode::KEY_KP2, KeyCode::KEY_KP3][col],
            3 if col < 2 => KeyCode::KEY_KP0,
            3 => KeyCode::KEY_KPDOT,
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
        assert_eq!(key_at(0.14, 0.25), Some(KeyCode::KEY_KP7));
        assert_eq!(key_at(0.37, 0.25), Some(KeyCode::KEY_KP8));
        assert_eq!(key_at(0.55, 0.25), Some(KeyCode::KEY_KP9));
        assert_eq!(key_at(0.14, 0.46), Some(KeyCode::KEY_KP4));
        assert_eq!(key_at(0.37, 0.46), Some(KeyCode::KEY_KP5));
        assert_eq!(key_at(0.55, 0.46), Some(KeyCode::KEY_KP6));
        assert_eq!(key_at(0.14, 0.66), Some(KeyCode::KEY_KP1));
        assert_eq!(key_at(0.37, 0.66), Some(KeyCode::KEY_KP2));
        assert_eq!(key_at(0.55, 0.66), Some(KeyCode::KEY_KP3));
        assert_eq!(key_at(0.14, 0.86), Some(KeyCode::KEY_KP0));
        assert_eq!(key_at(0.37, 0.86), Some(KeyCode::KEY_KP0));
        assert_eq!(key_at(0.55, 0.86), Some(KeyCode::KEY_KPDOT));
    }

    #[test]
    fn maps_g634jy_operator_and_control_strip_hitboxes() {
        assert_eq!(key_at(0.70, 0.25), Some(KeyCode::KEY_KPSLASH));
        assert_eq!(key_at(0.70, 0.46), Some(KeyCode::KEY_KPASTERISK));
        assert_eq!(key_at(0.70, 0.66), Some(KeyCode::KEY_KPMINUS));
        assert_eq!(key_at(0.70, 0.86), Some(KeyCode::KEY_KPPLUS));
        assert_eq!(key_at(0.90, 0.20), None);
        assert_eq!(key_at(0.90, 0.50), Some(KeyCode::KEY_BACKSPACE));
        assert_eq!(key_at(0.90, 0.80), Some(KeyCode::KEY_KPENTER));
    }

    #[test]
    fn detects_g634jy_toggle_zone_separately_from_keys() {
        let layout = G634jyLayout::new();

        assert!(layout.is_toggle_position(0.90, 0.20));
        assert!(!layout.is_toggle_position(0.90, 0.50));
        assert_eq!(layout.key_at_position(0.90, 0.20), None);
    }

    #[test]
    fn keeps_g634jy_photo_boundaries_out_of_uniform_grid_regressions() {
        assert_eq!(key_at(0.62, 0.25), Some(KeyCode::KEY_KP9));
        assert_eq!(key_at(0.70, 0.25), Some(KeyCode::KEY_KPSLASH));
    }

    #[test]
    fn leaves_g634jy_unlit_margins_dead() {
        assert_eq!(key_at(0.02, 0.25), None);
        assert_eq!(key_at(0.14, 0.98), None);
    }

    #[test]
    fn leaves_g634jy_separator_gaps_dead() {
        assert_eq!(key_at(0.28, 0.25), None);
        assert_eq!(key_at(0.46, 0.25), None);
        assert_eq!(key_at(0.65, 0.25), None);
        assert_eq!(key_at(0.80, 0.25), None);
        assert_eq!(key_at(0.14, 0.36), None);
        assert_eq!(key_at(0.14, 0.57), None);
        assert_eq!(key_at(0.14, 0.76), None);
    }
}
