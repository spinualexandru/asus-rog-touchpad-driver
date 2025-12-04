use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, InputEvent, KeyCode, SynchronizationCode};
use log::debug;
use std::io;

pub struct VirtualKeyboard {
    device: evdev::uinput::VirtualDevice,
}

impl VirtualKeyboard {
    /// Create a new virtual keyboard with the specified keys enabled
    pub fn new(keys: &[KeyCode]) -> io::Result<Self> {
        let mut key_set = AttributeSet::<KeyCode>::new();

        // Add all layout keys
        for key in keys {
            key_set.insert(*key);
        }

        // Add control keys
        key_set.insert(KeyCode::KEY_LEFTSHIFT);
        key_set.insert(KeyCode::KEY_NUMLOCK);
        key_set.insert(KeyCode::KEY_CALC);

        let device = VirtualDevice::builder()?
            .name("Asus Touchpad/Numpad")
            .with_keys(&key_set)?
            .build()?;

        debug!("Created virtual keyboard device");
        Ok(Self { device })
    }

    /// Send a key press event
    pub fn press_key(&mut self, key: KeyCode) -> io::Result<()> {
        let events = [
            InputEvent::new_now(evdev::EventType::KEY.0, key.0, 1),
            InputEvent::new_now(evdev::EventType::SYNCHRONIZATION.0, SynchronizationCode::SYN_REPORT.0, 0),
        ];
        self.device.emit(&events)
    }

    /// Send a key release event
    pub fn release_key(&mut self, key: KeyCode) -> io::Result<()> {
        let events = [
            InputEvent::new_now(evdev::EventType::KEY.0, key.0, 0),
            InputEvent::new_now(evdev::EventType::SYNCHRONIZATION.0, SynchronizationCode::SYN_REPORT.0, 0),
        ];
        self.device.emit(&events)
    }

    /// Send key press with shift modifier
    pub fn press_key_with_shift(&mut self, key: KeyCode) -> io::Result<()> {
        let events = [
            InputEvent::new_now(evdev::EventType::KEY.0, KeyCode::KEY_LEFTSHIFT.0, 1),
            InputEvent::new_now(evdev::EventType::KEY.0, key.0, 1),
            InputEvent::new_now(evdev::EventType::SYNCHRONIZATION.0, SynchronizationCode::SYN_REPORT.0, 0),
        ];
        self.device.emit(&events)
    }

    /// Release key with shift modifier
    pub fn release_key_with_shift(&mut self, key: KeyCode) -> io::Result<()> {
        let events = [
            InputEvent::new_now(evdev::EventType::KEY.0, KeyCode::KEY_LEFTSHIFT.0, 0),
            InputEvent::new_now(evdev::EventType::KEY.0, key.0, 0),
            InputEvent::new_now(evdev::EventType::SYNCHRONIZATION.0, SynchronizationCode::SYN_REPORT.0, 0),
        ];
        self.device.emit(&events)
    }

    /// Send numlock toggle event
    pub fn toggle_numlock(&mut self, enabled: bool) -> io::Result<()> {
        let value = if enabled { 1 } else { 0 };
        let events = [
            InputEvent::new_now(evdev::EventType::KEY.0, KeyCode::KEY_NUMLOCK.0, value),
            InputEvent::new_now(evdev::EventType::SYNCHRONIZATION.0, SynchronizationCode::SYN_REPORT.0, 0),
        ];
        self.device.emit(&events)
    }
}
