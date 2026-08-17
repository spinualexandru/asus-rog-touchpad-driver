use super::NumpadLayout;
use evdev::KeyCode;

const NUMERIC_COLUMNS: [(f64, f64); 3] = [(0.05, 0.22), (0.25, 0.40), (0.45, 0.55)];
const OPERATOR_COLUMN: (f64, f64) = (0.60, 0.75);
const RIGHT_COLUMN: (f64, f64) = (0.80, 0.95);
const ZERO_COLUMN: (f64, f64) = (0.05, 0.40);
const DOT_COLUMN: (f64, f64) = (0.45, 0.60);

const MAIN_ROWS: [(f64, f64); 4] = [(0.05, 0.25), (0.30, 0.50), (0.55, 0.75), (0.80, 0.95)];
/// Unlike every other column, the operator rows are deliberately contiguous:
/// `/`-`*` and `-`-`+` have no dead band between them, so an edge-of-key tap still
/// registers as one of the two. Only the `*`-`-` boundary carries a gap. This is
/// intentional — do not "fix" it into MAIN_ROWS spacing without recalibrating
/// against the physical pad.
const OPERATOR_ROWS: [(f64, f64); 4] = [(0.05, 0.30), (0.30, 0.55), (0.60, 0.75), (0.75, 0.95)];
const RIGHT_ROWS: [(f64, f64); 3] = [(0.00, 0.30), (0.30, 0.50), (0.55, 0.95)];

/// Calculator / brightness zone, in the unlit top-left margin.
///
/// Held strictly left of `NUMERIC_COLUMNS[0]` so it cannot shadow the "7" key —
/// the driver tests this zone before it hit-tests keys.
const CALC_MAX_X: f64 = 0.05;
const CALC_MAX_Y: f64 = 0.10;

/// ROG Strix SCAR 16 G634JY / G634JYR layout
/// ASUF1416:00 2808:0108
/// LED backlight works using I2C address 0x38
///
/// The pad's silkscreen is not a uniform grid, so this layout defines its
/// geometry entirely through the band constants above and `key_at_position`;
/// the trait's grid methods are left at their defaults.
pub struct G634jyLayout;

impl G634jyLayout {
    pub fn new() -> Self {
        Self
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

    fn is_toggle_position(&self, x: f64, y: f64) -> bool {
        in_band(x, RIGHT_COLUMN) && in_band(y, RIGHT_ROWS[0])
    }

    fn is_calc_position(&self, x: f64, y: f64) -> bool {
        x < CALC_MAX_X && y < CALC_MAX_Y
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
    fn keeps_g634jy_calc_zone_clear_of_the_seven_key() {
        let layout = G634jyLayout::new();

        // Inside the unlit top-left margin: calculator, and no key underneath.
        assert!(layout.is_calc_position(0.02, 0.02));
        assert_eq!(layout.key_at_position(0.02, 0.02), None);

        // The sliver that the old hard-coded 0.06 x 0.07 corner used to claim now
        // belongs to "7", which is what the silkscreen shows there.
        assert!(!layout.is_calc_position(0.055, 0.06));
        assert_eq!(layout.key_at_position(0.055, 0.06), Some(KeyCode::KEY_KP7));
    }

    /// Number of 0.001-wide samples needed to cover `0.0..=limit` inclusive.
    ///
    /// Sweep bounds are computed from the zone constants instead of being written
    /// out as literals: a literal bound only covers the zone the constants happen
    /// to describe today, so growing a constant would shrink the swept fraction of
    /// its own zone and let the sweep pass over the very overlap it exists to find.
    fn sweep_steps(limit: f64) -> u32 {
        (limit * 1000.0).ceil() as u32
    }

    #[test]
    fn g634jy_calc_zone_never_overlaps_a_key() {
        let layout = G634jyLayout::new();

        // Sweep the whole zone rather than trusting the two constants to stay in
        // sync with NUMERIC_COLUMNS by inspection.
        let mut swept_any = false;
        for xi in 0..=sweep_steps(CALC_MAX_X) {
            for yi in 0..=sweep_steps(CALC_MAX_Y) {
                let (x, y) = (xi as f64 / 1000.0, yi as f64 / 1000.0);
                if layout.is_calc_position(x, y) {
                    swept_any = true;
                    assert_eq!(
                        layout.key_at_position(x, y),
                        None,
                        "calc zone shadows a key at x={x}, y={y}"
                    );
                }
            }
        }

        // Guards against a future edit making the sweep vacuously green.
        assert!(swept_any, "sweep never entered the calc zone");
    }

    #[test]
    fn g634jy_toggle_and_calc_zones_are_disjoint() {
        let layout = G634jyLayout::new();

        for xi in 0..=100 {
            for yi in 0..=100 {
                let (x, y) = (xi as f64 / 100.0, yi as f64 / 100.0);
                assert!(
                    !(layout.is_toggle_position(x, y) && layout.is_calc_position(x, y)),
                    "toggle and calc zones overlap at x={x}, y={y}"
                );
            }
        }
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
