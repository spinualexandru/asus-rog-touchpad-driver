use anyhow::{Context, Result};
use cli::{parse_cli, CliCommand, RunArgs};
use evdev::{AbsoluteAxisCode, KeyCode, LedCode, SynchronizationCode};
use log::{debug, error, info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

mod cli;
mod device;
mod error;
mod i2c;
mod input;
mod layouts;
mod numpad;

use device::detect_devices;
use i2c::{try_create_led_controller, LedController};
use input::{keys_with_extra, TouchpadBounds, TouchpadReader, VirtualKeyboard};
use layouts::{get_layout, NumpadLayout};
use numpad::{Corner, NumpadState, TouchPosition};

/// Runtime context holding all mutable driver state
struct DriverContext<'a> {
    state: NumpadState,
    virtual_kb: VirtualKeyboard,
    led: Option<LedController>,
    touchpad: TouchpadReader,
    layout: &'a dyn NumpadLayout,
    bounds: TouchpadBounds,
    percentage_key: KeyCode,
    pending_finger_event: Option<i32>,
    numlock_was_on: Option<bool>,
    numlock_toggled_by_driver: bool,
}

static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = parse_cli();
    match cli.command.unwrap_or(CliCommand::Run(RunArgs::default())) {
        CliCommand::Run(args) => run_driver(args),
        command => cli::execute_command(command),
    }
}

fn run_driver(args: RunArgs) -> Result<()> {
    install_signal_handlers();

    info!("Starting ASUS Touchpad Numpad Driver");
    info!(
        "Model: {}, Percentage key: {}",
        args.model, args.percentage_key
    );

    // Get layout
    let layout = get_layout(&args.model).context("Failed to load layout")?;

    // Detect devices with retries
    let devices = detect_devices(
        layout.try_times(),
        Duration::from_millis(layout.try_sleep_ms()),
    )
    .context("Failed to detect devices")?;

    info!(
        "Found touchpad: {} at {}",
        devices.touchpad.name, devices.touchpad.event_path
    );
    if let Some(ref keyboard) = devices.keyboard {
        info!(
            "Found keyboard: {} at {}",
            keyboard.name, keyboard.event_path
        );
    }
    info!("Using I2C address: 0x{:02x}", devices.i2c_address);

    // Initialize touchpad reader
    let touchpad =
        TouchpadReader::open(&devices.touchpad.event_path).context("Failed to open touchpad")?;

    let bounds = touchpad.bounds();
    debug!(
        "Touchpad bounds: x={}-{}, y={}-{}",
        bounds.min_x, bounds.max_x, bounds.min_y, bounds.max_y
    );

    let percentage_key = KeyCode(args.percentage_key);
    let virtual_keys = keys_with_extra(layout.all_keys(), percentage_key);

    // Initialize virtual keyboard
    let virtual_kb =
        VirtualKeyboard::new(&virtual_keys).context("Failed to create virtual keyboard")?;

    // Initialize LED controller (optional - warn and continue on failure)
    let led = try_create_led_controller(devices.touchpad.i2c_bus, devices.i2c_address);
    let numlock_was_on =
        read_numlock_state(devices.keyboard.as_ref().map(|kb| kb.event_path.as_str()));

    // Create driver context
    let mut ctx = DriverContext {
        state: NumpadState::new(),
        virtual_kb,
        led,
        touchpad,
        layout: layout.as_ref(),
        bounds,
        percentage_key,
        pending_finger_event: None,
        numlock_was_on,
        numlock_toggled_by_driver: false,
    };

    info!("Entering main event loop");
    notify_systemd(&[("READY", "1"), ("STATUS", "Driver running")]);

    // Main event loop
    while !SHUTDOWN_REQUESTED.load(Ordering::SeqCst) {
        match ctx.touchpad.fetch_events() {
            Ok(events) => {
                for event in events {
                    if let Err(e) = process_event(&event, &mut ctx) {
                        error!("Error processing event: {}", e);
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No events available, sleep briefly
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => {
                error!("Error reading events: {}", e);
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }

    info!("Shutdown requested, cleaning up driver state");
    notify_systemd(&[("STOPPING", "1"), ("STATUS", "Driver stopping")]);
    cleanup(&mut ctx);
    Ok(())
}

fn process_event(event: &evdev::InputEvent, ctx: &mut DriverContext) -> Result<()> {
    use evdev::EventType;

    match event.event_type() {
        EventType::ABSOLUTE => {
            let code = AbsoluteAxisCode(event.code());
            match code {
                AbsoluteAxisCode::ABS_MT_POSITION_X | AbsoluteAxisCode::ABS_X => {
                    ctx.state
                        .update_x(event.value(), ctx.bounds.min_x, ctx.bounds.max_x);
                }
                AbsoluteAxisCode::ABS_MT_POSITION_Y | AbsoluteAxisCode::ABS_Y => {
                    ctx.state
                        .update_y(event.value(), ctx.bounds.min_y, ctx.bounds.max_y);
                }
                _ => {}
            }
        }
        EventType::KEY => {
            let key = KeyCode(event.code());
            if key == KeyCode::BTN_TOOL_FINGER {
                ctx.pending_finger_event = Some(event.value());
            }
        }
        EventType::SYNCHRONIZATION
            if SynchronizationCode(event.code()) == SynchronizationCode::SYN_REPORT =>
        {
            if let Some(value) = ctx.pending_finger_event.take() {
                handle_finger_event(value, ctx)?;
            }
        }
        _ => {}
    }

    Ok(())
}

fn handle_finger_event(value: i32, ctx: &mut DriverContext) -> Result<()> {
    if value == 0 {
        // Finger up - release any pressed key
        debug!(
            "Finger up at x={:.2}, y={:.2}",
            ctx.state.current_position.x, ctx.state.current_position.y
        );

        release_pressed_key(ctx)?;
    } else if value == 1 && ctx.state.pressed_key.is_none() {
        // Finger down - handle corner detection or key press
        debug!(
            "Finger down at x={:.2}, y={:.2}",
            ctx.state.current_position.x, ctx.state.current_position.y
        );

        let position = ctx.state.current_position;
        let corner = corner_at_position(ctx.layout, position);

        match corner {
            Corner::TopRight => {
                // Toggle numpad
                if !ctx.state.enabled {
                    enable_numpad(ctx)?;
                    ctx.state.enabled = true;
                    info!("Numpad enabled");
                } else {
                    disable_numpad(ctx)?;
                    ctx.state.enabled = false;
                    info!("Numpad disabled");
                }
            }
            Corner::TopLeft => {
                if ctx.state.enabled {
                    // Cycle brightness
                    ctx.state.cycle_brightness();
                    if let Some(ref mut led_ctrl) = ctx.led {
                        if let Err(e) = led_ctrl.set_brightness(ctx.state.brightness) {
                            warn!("Failed to change brightness: {}", e);
                        }
                    }
                    debug!("Brightness changed to {:?}", ctx.state.brightness);
                } else {
                    // Launch calculator
                    ctx.virtual_kb.click_key(KeyCode::KEY_CALC)?;
                    debug!("Calculator key sent");
                }
            }
            Corner::None if ctx.state.enabled => {
                // Numpad key press
                if let Some(key) = ctx
                    .layout
                    .key_at_position(position.x, position.y)
                    .map(|key| map_layout_key(key, ctx.percentage_key))
                {
                    debug!(
                        "Key press: {:?} at x={:.2}, y={:.2}",
                        key, position.x, position.y
                    );

                    if key == ctx.percentage_key {
                        ctx.virtual_kb.press_key_with_shift(key)?;
                    } else {
                        ctx.virtual_kb.press_key(key)?;
                    }
                    ctx.state.pressed_key = Some(key);
                }
            }
            _ => {}
        }
    }

    Ok(())
}

fn corner_at_position(layout: &dyn NumpadLayout, position: TouchPosition) -> Corner {
    if layout.is_toggle_position(position.x, position.y) {
        Corner::TopRight
    } else if position.corner() == Corner::TopLeft {
        Corner::TopLeft
    } else {
        Corner::None
    }
}

fn enable_numpad(ctx: &mut DriverContext) -> Result<()> {
    ctx.touchpad.grab()?;
    if !ctx.numlock_was_on.unwrap_or(false) && !ctx.numlock_toggled_by_driver {
        ctx.virtual_kb.click_numlock()?;
        ctx.numlock_toggled_by_driver = true;
    }
    if let Some(ref mut led_ctrl) = ctx.led {
        if let Err(e) = led_ctrl.set_brightness(ctx.state.brightness) {
            warn!("Failed to set LED brightness: {}", e);
        }
    }
    Ok(())
}

fn disable_numpad(ctx: &mut DriverContext) -> Result<()> {
    release_pressed_key(ctx)?;
    ctx.touchpad.ungrab()?;
    if ctx.numlock_toggled_by_driver {
        ctx.virtual_kb.click_numlock()?;
        ctx.numlock_toggled_by_driver = false;
    }
    if let Some(ref mut led_ctrl) = ctx.led {
        if let Err(e) = led_ctrl.turn_off() {
            warn!("Failed to turn off LED: {}", e);
        }
    }
    Ok(())
}

fn release_pressed_key(ctx: &mut DriverContext) -> Result<()> {
    if let Some(key) = ctx.state.pressed_key.take() {
        debug!("Releasing key: {:?}", key);
        if key == ctx.percentage_key {
            ctx.virtual_kb.release_key_with_shift(key)?;
        } else {
            ctx.virtual_kb.release_key(key)?;
        }
    }
    Ok(())
}

fn cleanup(ctx: &mut DriverContext) {
    if let Err(e) = disable_numpad(ctx) {
        warn!("Failed to fully clean up driver state: {}", e);
    }
    ctx.state.enabled = false;
}

fn map_layout_key(key: KeyCode, percentage_key: KeyCode) -> KeyCode {
    if key == KeyCode::KEY_KP5 || key == KeyCode::KEY_5 {
        percentage_key
    } else {
        key
    }
}

fn read_numlock_state(keyboard_path: Option<&str>) -> Option<bool> {
    let keyboard_path = keyboard_path?;
    match evdev::Device::open(keyboard_path).and_then(|device| device.get_led_state()) {
        Ok(leds) => {
            let numlock_on = leds.contains(LedCode::LED_NUML);
            debug!("Initial NumLock state: {}", numlock_on);
            Some(numlock_on)
        }
        Err(e) => {
            warn!(
                "Could not read initial NumLock state from {}: {}",
                keyboard_path, e
            );
            None
        }
    }
}

fn install_signal_handlers() {
    unsafe extern "C" fn handle_signal(_: libc::c_int) {
        SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
    }

    unsafe {
        libc::signal(
            libc::SIGINT,
            handle_signal as *const () as libc::sighandler_t,
        );
        libc::signal(
            libc::SIGTERM,
            handle_signal as *const () as libc::sighandler_t,
        );
    }
}

fn notify_systemd(state: &[(&str, &str)]) {
    match systemd::daemon::notify(false, state.iter()) {
        Ok(true) => debug!("Sent systemd status notification"),
        Ok(false) => debug!("No systemd notification socket available"),
        Err(e) => warn!("Failed to notify systemd: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_keypad_five_to_percentage_key() {
        assert_eq!(
            map_layout_key(KeyCode::KEY_KP5, KeyCode::KEY_6),
            KeyCode::KEY_6
        );
        assert_eq!(
            map_layout_key(KeyCode::KEY_5, KeyCode::KEY_6),
            KeyCode::KEY_6
        );
        assert_eq!(
            map_layout_key(KeyCode::KEY_KP4, KeyCode::KEY_6),
            KeyCode::KEY_KP4
        );
    }

    #[test]
    fn g634jy_corner_detection_respects_layout_toggle_dead_zone() {
        let layout = layouts::G634jyLayout::new();

        assert_eq!(
            corner_at_position(&layout, TouchPosition { x: 0.82, y: 0.20 }),
            Corner::None
        );
        assert_eq!(
            corner_at_position(&layout, TouchPosition { x: 0.90, y: 0.20 }),
            Corner::TopRight
        );
        assert_eq!(
            corner_at_position(&layout, TouchPosition { x: 0.05, y: 0.05 }),
            Corner::TopLeft
        );
    }
}
