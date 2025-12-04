use anyhow::{Context, Result};
use evdev::{AbsoluteAxisCode, KeyCode};
use log::{debug, error, info, warn};
use std::env;
use std::time::Duration;

mod device;
mod error;
mod i2c;
mod input;
mod layouts;
mod numpad;

use device::detect_devices;
use i2c::{try_create_led_controller, LedController};
use input::{TouchpadBounds, TouchpadReader, VirtualKeyboard};
use layouts::{get_layout, NumpadLayout};
use numpad::{Corner, NumpadState};

/// Runtime context holding all mutable driver state
struct DriverContext<'a> {
    state: NumpadState,
    virtual_kb: VirtualKeyboard,
    led: Option<LedController>,
    touchpad: TouchpadReader,
    layout: &'a dyn NumpadLayout,
    bounds: TouchpadBounds,
    percentage_key: KeyCode,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // Parse command line arguments
    let args: Vec<String> = env::args().collect();
    let model = args.get(1).map(|s| s.as_str()).unwrap_or("g634jy");
    let percentage_key_code: u16 = args
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(6);

    info!("Starting ASUS Touchpad Numpad Driver");
    info!("Model: {}, Percentage key: {}", model, percentage_key_code);

    // Get layout
    let layout = get_layout(model).context("Failed to load layout")?;

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
    info!(
        "Found keyboard: {} at {}",
        devices.keyboard.name, devices.keyboard.event_path
    );
    info!("Using I2C address: 0x{:02x}", devices.i2c_address);

    // Initialize touchpad reader
    let touchpad =
        TouchpadReader::open(&devices.touchpad.event_path).context("Failed to open touchpad")?;

    let bounds = touchpad.bounds();
    debug!(
        "Touchpad bounds: x={}-{}, y={}-{}",
        bounds.min_x, bounds.max_x, bounds.min_y, bounds.max_y
    );

    // Initialize virtual keyboard
    let virtual_kb =
        VirtualKeyboard::new(&layout.all_keys()).context("Failed to create virtual keyboard")?;

    // Initialize LED controller (optional - warn and continue on failure)
    let led = try_create_led_controller(devices.touchpad.i2c_bus, devices.i2c_address);

    // Create driver context
    let mut ctx = DriverContext {
        state: NumpadState::new(),
        virtual_kb,
        led,
        touchpad,
        layout: layout.as_ref(),
        bounds,
        percentage_key: KeyCode(percentage_key_code),
    };

    info!("Entering main event loop");

    // Main event loop
    loop {
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
}

fn process_event(event: &evdev::InputEvent, ctx: &mut DriverContext) -> Result<()> {
    use evdev::EventType;

    match event.event_type() {
        EventType::ABSOLUTE => {
            let code = AbsoluteAxisCode(event.code());
            match code {
                AbsoluteAxisCode::ABS_MT_POSITION_X => {
                    ctx.state
                        .update_x(event.value(), ctx.bounds.min_x, ctx.bounds.max_x);
                }
                AbsoluteAxisCode::ABS_MT_POSITION_Y => {
                    ctx.state
                        .update_y(event.value(), ctx.bounds.min_y, ctx.bounds.max_y);
                }
                _ => {}
            }
        }
        EventType::KEY => {
            let key = KeyCode(event.code());
            if key == KeyCode::BTN_TOOL_FINGER {
                handle_finger_event(event.value(), ctx)?;
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

        if let Some(key) = ctx.state.pressed_key.take() {
            debug!("Releasing key: {:?}", key);
            if key == ctx.percentage_key {
                ctx.virtual_kb.release_key_with_shift(key)?;
            } else {
                ctx.virtual_kb.release_key(key)?;
            }
        }
    } else if value == 1 && ctx.state.pressed_key.is_none() {
        // Finger down - handle corner detection or key press
        debug!(
            "Finger down at x={:.2}, y={:.2}",
            ctx.state.current_position.x, ctx.state.current_position.y
        );

        let corner = ctx.state.current_position.corner();

        match corner {
            Corner::TopRight => {
                // Toggle numpad
                ctx.state.enabled = !ctx.state.enabled;
                if ctx.state.enabled {
                    ctx.touchpad.grab()?;
                    ctx.virtual_kb.toggle_numlock(true)?;
                    if let Some(ref mut led_ctrl) = ctx.led {
                        if let Err(e) = led_ctrl.set_brightness(ctx.state.brightness) {
                            warn!("Failed to set LED brightness: {}", e);
                        }
                    }
                    info!("Numpad enabled");
                } else {
                    ctx.touchpad.ungrab()?;
                    ctx.virtual_kb.toggle_numlock(false)?;
                    if let Some(ref mut led_ctrl) = ctx.led {
                        if let Err(e) = led_ctrl.turn_off() {
                            warn!("Failed to turn off LED: {}", e);
                        }
                    }
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
                    ctx.virtual_kb.press_key(KeyCode::KEY_CALC)?;
                    ctx.virtual_kb.release_key(KeyCode::KEY_CALC)?;
                    debug!("Calculator key sent");
                }
            }
            Corner::None if ctx.state.enabled => {
                // Numpad key press
                if let Some((row, col)) = ctx.state.grid_position(ctx.layout) {
                    if let Some(mut key) = ctx.layout.key_at(row, col) {
                        // Handle percentage key mapping (KEY_5 -> percentage_key)
                        if key == KeyCode::KEY_5 {
                            key = ctx.percentage_key;
                        }

                        debug!("Key press: {:?} at ({}, {})", key, row, col);

                        if key == ctx.percentage_key {
                            ctx.virtual_kb.press_key_with_shift(key)?;
                        } else {
                            ctx.virtual_kb.press_key(key)?;
                        }
                        ctx.state.pressed_key = Some(key);
                    }
                }
            }
            _ => {}
        }
    }

    Ok(())
}
