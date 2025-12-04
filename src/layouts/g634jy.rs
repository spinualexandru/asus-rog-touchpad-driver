use super::NumpadLayout;
use evdev::KeyCode;

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
