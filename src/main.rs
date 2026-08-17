use anyhow::{bail, Context, Result};
use cli::{parse_cli, CliCommand, RunArgs};
use evdev::{AbsoluteAxisCode, KeyCode, LedCode, SynchronizationCode};
use log::{debug, error, info, warn};
use std::io;
use std::os::fd::{AsRawFd, RawFd};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::{Duration, Instant};

mod cli;
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
use numpad::{normalize_axis, ContactTracker, Corner, NumpadState, TouchPosition};

/// A corner touch waiting to clear the hold threshold.
///
/// Only used while the pad is ungrabbed; see `handle_finger_event`.
#[derive(Debug, Clone, Copy)]
struct PendingCorner {
    corner: Corner,
    since: Instant,
    /// Set once the action has fired, so lifting the finger does not repeat it.
    fired: bool,
}

impl PendingCorner {
    /// Whether this hold has earned its action, given the clock and where the
    /// finger is *now*.
    ///
    /// All three conditions are load-bearing, and the position check in particular
    /// is not redundant with `poll_pending_corner`'s. A report that carries
    /// `BTN_TOOL_FINGER=0` is dispatched to `handle_finger_event`, never to
    /// `poll_pending_corner` — so when the finger's last movement out of the zone
    /// and its lift arrive in the *same* report, the release path is the only place
    /// left that can notice the slide-out. Without it a hold that ended somewhere
    /// else entirely still fires, enabling the numpad or launching the calculator
    /// after the user has visibly left the corner.
    ///
    /// Pure so the truth table is testable without uinput or `/dev/input`.
    fn should_fire(&self, now: Instant, corner_now: Corner) -> bool {
        !self.fired && now.duration_since(self.since) >= CORNER_HOLD && corner_now == self.corner
    }
}

/// Runtime context holding all mutable driver state
struct DriverContext<'a> {
    state: NumpadState,
    virtual_kb: VirtualKeyboard,
    led: Option<LedController>,
    touchpad: TouchpadReader,
    layout: &'a dyn NumpadLayout,
    bounds: TouchpadBounds,
    pending_finger_event: Option<i32>,
    pending_corner: Option<PendingCorner>,
    /// Multitouch contact lifetimes, so the position always follows a finger that
    /// is still down and only a genuinely new contact can fire a key.
    contacts: ContactTracker,
    keyboard_path: Option<String>,
    numlock_toggled_by_driver: bool,
}

static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Write end of the self-pipe the signal handlers poke, or `-1` before it exists.
///
/// A raw fd in an atomic because that is all a signal handler may touch: an atomic
/// load and a one-byte `write` are async-signal-safe, a `Mutex` or an allocation is not.
static WAKEUP_WRITE_FD: AtomicI32 = AtomicI32::new(-1);

/// Backoff between failed reads of the touchpad device.
const READ_ERROR_BACKOFF: Duration = Duration::from_millis(100);

/// Consecutive failed reads tolerated before giving up and exiting non-zero.
/// At `READ_ERROR_BACKOFF` apiece this is ~5s of sustained failure, long enough
/// to ride out a transient hiccup and short enough that a restart heals quickly.
const MAX_CONSECUTIVE_READ_ERRORS: u32 = 50;

/// How long a corner must be held before it acts, while the pad is ungrabbed.
///
/// With the numpad off the pad is not grabbed, so the compositor sees these
/// touches too and a corner tap is far more likely to be an ordinary gesture than
/// a deliberate one. Requiring a hold is what the ASUS firmware does as well.
const CORNER_HOLD: Duration = Duration::from_millis(400);

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = parse_cli();
    match cli.command.unwrap_or(CliCommand::Run(RunArgs::default())) {
        CliCommand::Run(args) => run_driver(args),
        command => cli::execute_command(command),
    }
}

fn run_driver(args: RunArgs) -> Result<()> {
    // Before the handlers, so the first signal that can arrive already has a pipe
    // to write to. Without it the driver still runs, just with the old lost-wakeup
    // window — a warning beats refusing to start.
    let wakeup = match ShutdownWakeup::install() {
        Ok(wakeup) => Some(wakeup),
        Err(e) => {
            warn!(
                "Could not create the shutdown wakeup pipe, so a signal arriving between two touches may not be noticed until the next one: {}",
                e
            );
            None
        }
    };
    install_signal_handlers();

    info!("Starting ASUS Touchpad Numpad Driver");
    info!("Model: {}", args.model);

    // Get layout
    let layout = get_layout(&args.model).context("Failed to load layout")?;
    info!("Using layout: {}", layout.name());

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

    let virtual_keys = layout.all_keys();

    // Initialize virtual keyboard
    let virtual_kb =
        VirtualKeyboard::new(&virtual_keys).context("Failed to create virtual keyboard")?;

    // Initialize LED controller (optional - warn and continue on failure)
    let led = try_create_led_controller(devices.touchpad.i2c_bus, devices.i2c_address);
    let keyboard_path = devices.keyboard.as_ref().map(|kb| kb.event_path.clone());

    // Create driver context
    let mut ctx = DriverContext {
        state: NumpadState::new(),
        virtual_kb,
        led,
        touchpad,
        layout: layout.as_ref(),
        bounds,
        pending_finger_event: None,
        pending_corner: None,
        contacts: ContactTracker::new(),
        keyboard_path,
        numlock_toggled_by_driver: false,
    };

    info!("Entering main event loop");
    notify_systemd(&[("READY", "1"), ("STATUS", "Driver running")]);

    let outcome = run_event_loop(&mut ctx, wakeup.as_ref());
    match &outcome {
        Ok(()) => info!("Shutdown requested, cleaning up driver state"),
        Err(e) => error!("Event loop stopped: {:#}", e),
    }

    notify_systemd(&[("STOPPING", "1"), ("STATUS", "Driver stopping")]);
    cleanup(&mut ctx);
    outcome
}

/// The read end of the self-pipe the signal handlers write to.
///
/// The shutdown atomic alone cannot wake a process that is about to park: a signal
/// delivered after the loop tested `SHUTDOWN_REQUESTED` but before the kernel call
/// begins sets the flag and interrupts nothing, and the loop then sleeps until the
/// user next touches the pad — `systemctl stop` hangs to its timeout and SIGKILLs,
/// so `cleanup()` never runs. A byte in a pipe is *state* rather than an edge: it
/// stays buffered, so the wait returns at once however late or early the signal was.
struct ShutdownWakeup {
    read_fd: RawFd,
}

impl ShutdownWakeup {
    fn install() -> io::Result<Self> {
        let mut fds = [-1 as libc::c_int; 2];
        // Non-blocking at both ends: a handler must never block whatever the
        // process was doing, and `drain` must never park the loop. Close-on-exec
        // so the subprocesses cli.rs spawns cannot inherit it.
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) } != 0 {
            return Err(io::Error::last_os_error());
        }

        // Published for the handler and never closed for the life of the process:
        // a signal can land at any instant, and writing a byte into a recycled fd
        // would be far worse than leaking one.
        WAKEUP_WRITE_FD.store(fds[1], Ordering::SeqCst);

        Ok(Self { read_fd: fds[0] })
    }

    fn read_fd(&self) -> RawFd {
        self.read_fd
    }

    /// Empties the pipe, so one signal does not leave `poll` returning readable forever.
    ///
    /// A short read means the pipe is empty; an error means it is empty (`EAGAIN`)
    /// or a signal landed mid-read, which will have queued its own byte — either
    /// way the next wait is still correct, so nothing here needs retrying.
    fn drain(&self) {
        let mut buf = [0u8; 64];
        loop {
            let read = unsafe { libc::read(self.read_fd, buf.as_mut_ptr().cast(), buf.len()) };
            if read != buf.len() as isize {
                break;
            }
        }
    }
}

/// What woke the event loop out of `poll`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wakeup {
    /// The touchpad fd has something to report: events, or an error the read surfaces.
    Touchpad,
    /// A signal handler ran, so the shutdown flag needs re-checking.
    Signal,
}

/// Reads a `poll` result into a `Wakeup`.
///
/// The pipe wins over the touchpad: with shutdown pending there is nothing worth
/// reading, and any events left queued die with the process anyway. Anything at all
/// on the touchpad fd counts as touchpad activity, `POLLERR`/`POLLNVAL` included —
/// the read that follows turns those into the `io::Error` the failure budget wants,
/// rather than this function having to classify device breakage itself.
fn classify_wakeup(touchpad_revents: libc::c_short, wakeup_revents: libc::c_short) -> Wakeup {
    if wakeup_revents != 0 || touchpad_revents == 0 {
        Wakeup::Signal
    } else {
        Wakeup::Touchpad
    }
}

/// Blocks until the touchpad has something to read or a signal handler pokes the pipe.
///
/// With no pipe (creation failed) this degrades to waiting on the touchpad alone —
/// `poll` ignores a negative fd — which is the pre-pipe behaviour, race included.
fn wait_for_wakeup(touchpad_fd: RawFd, wakeup: Option<&ShutdownWakeup>) -> io::Result<Wakeup> {
    let mut fds = [
        libc::pollfd {
            fd: touchpad_fd,
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: wakeup.map_or(-1, ShutdownWakeup::read_fd),
            events: libc::POLLIN,
            revents: 0,
        },
    ];

    // No timeout: the loop must block until there is real work, never spin.
    let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) };

    if ready < 0 {
        let error = io::Error::last_os_error();
        // The signal landed while we were already parked — the other half of the
        // same story, and not a failure. The caller re-checks the flag.
        if error.kind() == io::ErrorKind::Interrupted {
            return Ok(Wakeup::Signal);
        }
        return Err(error);
    }

    let wakeup_kind = classify_wakeup(fds[0].revents, fds[1].revents);
    if wakeup_kind == Wakeup::Signal {
        if let Some(wakeup) = wakeup {
            wakeup.drain();
        }
    }

    Ok(wakeup_kind)
}

/// Charges a failed wait or read to the shared budget, and sleeps the backoff.
///
/// `Err` once the budget is spent, which ends the loop and exits non-zero.
fn note_failure(consecutive: &mut u32, what: &str, error: &dyn std::fmt::Display) -> Result<()> {
    *consecutive += 1;
    error!(
        "Error {} touchpad events ({}/{}): {}",
        what, consecutive, MAX_CONSECUTIVE_READ_ERRORS, error
    );

    if *consecutive >= MAX_CONSECUTIVE_READ_ERRORS {
        bail!(
            "touchpad unreadable after {} consecutive failures",
            MAX_CONSECUTIVE_READ_ERRORS
        );
    }

    std::thread::sleep(READ_ERROR_BACKOFF);
    Ok(())
}

/// Reads touchpad events until shutdown is requested.
///
/// Returns `Err` once the touchpad stops being readable, rather than retrying
/// forever: the unit's `Restart=on-failure` then restarts the driver, which
/// re-runs detection. That matters after suspend/resume, where the touchpad can
/// come back as a different `/dev/input/eventN` and the old handle is dead for good.
fn run_event_loop(ctx: &mut DriverContext, wakeup: Option<&ShutdownWakeup>) -> Result<()> {
    let mut consecutive_read_errors = 0u32;

    // The loop now waits in `poll` and only reads once it says so, so the fd must
    // not be blocking: a wakeup that carries no events — the pipe firing, or a
    // readable report that a `SYN_DROPPED` resync has already consumed — would
    // otherwise park the process in `read()`, which is the stall being fixed here.
    // Failing that, the old blocking behaviour still works for every ordinary touch.
    if let Err(e) = ctx.touchpad.set_nonblocking(true) {
        warn!(
            "Could not make the touchpad fd non-blocking, so an empty wakeup may stall until the next touch: {}",
            e
        );
    }

    let touchpad_fd = ctx.touchpad.as_fd().as_raw_fd();

    while !SHUTDOWN_REQUESTED.load(Ordering::SeqCst) {
        match wait_for_wakeup(touchpad_fd, wakeup) {
            Ok(Wakeup::Touchpad) => {}
            // A handler ran. The loop condition above decides whether that means
            // shutdown; nothing else here does.
            Ok(Wakeup::Signal) => continue,
            // A broken fd fails `poll` as surely as it fails `read`, so both share
            // one budget and one backoff.
            Err(e) => {
                note_failure(&mut consecutive_read_errors, "waiting for", &e)?;
                continue;
            }
        }

        match ctx.touchpad.fetch_events() {
            Ok(events) => {
                consecutive_read_errors = 0;
                for event in events {
                    if let Err(e) = process_event(&event, ctx) {
                        error!("Error processing event: {}", e);
                    }
                }
            }
            // A signal handler ran and interrupted the read. The loop condition
            // above decides whether that means shutdown.
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            // Nothing queued after all. Harmless now that the fd is non-blocking:
            // go back to waiting rather than sleeping on a guess.
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
            Err(e) => note_failure(&mut consecutive_read_errors, "reading", &e)?,
        }
    }

    Ok(())
}

fn process_event(event: &evdev::InputEvent, ctx: &mut DriverContext) -> Result<()> {
    use evdev::EventType;

    match event.event_type() {
        EventType::ABSOLUTE => {
            let code = AbsoluteAxisCode(event.code());
            match code {
                // Multitouch protocol B reports a slot only when it changes, so
                // this stays sticky between reports.
                AbsoluteAxisCode::ABS_MT_SLOT => {
                    ctx.contacts.set_slot(event.value());
                }
                // Contact lifetimes. Slot reuse alone cannot tell a finger that is
                // still down from one that just left, and the pad reports no
                // coordinate for the finger it hands the contact over to.
                AbsoluteAxisCode::ABS_MT_TRACKING_ID => {
                    if ctx.contacts.track(event.value()) {
                        if let Some(position) = ctx.contacts.primary_position() {
                            debug!(
                                "Contact handed to slot {:?} at x={:.2}, y={:.2}",
                                ctx.contacts.primary_slot(),
                                position.x,
                                position.y
                            );
                            ctx.state.current_position = position;
                        }
                    }
                }
                // Every slot's coordinate is recorded, because a secondary finger's
                // is what ownership falls back to when the primary lifts. Only the
                // owning slot moves the position the driver acts on: without that,
                // resting a second finger drags it to that finger.
                AbsoluteAxisCode::ABS_MT_POSITION_X => {
                    let x = normalize_axis(event.value(), ctx.bounds.min_x, ctx.bounds.max_x);
                    ctx.contacts.update_x(x);
                    if ctx.contacts.current_slot_owns_position() {
                        ctx.state.current_position.x = x;
                    }
                }
                AbsoluteAxisCode::ABS_MT_POSITION_Y => {
                    let y = normalize_axis(event.value(), ctx.bounds.min_y, ctx.bounds.max_y);
                    ctx.contacts.update_y(y);
                    if ctx.contacts.current_slot_owns_position() {
                        ctx.state.current_position.y = y;
                    }
                }
                // Single-touch axes carry no slot and are always the primary contact.
                AbsoluteAxisCode::ABS_X => {
                    ctx.state
                        .update_x(event.value(), ctx.bounds.min_x, ctx.bounds.max_x);
                }
                AbsoluteAxisCode::ABS_Y => {
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
            let outcome = match ctx.pending_finger_event.take() {
                Some(value) => handle_finger_event(value, ctx),
                // The finger is still down and has not changed state; a corner it
                // is resting on may just have cleared the hold threshold.
                None => poll_pending_corner(ctx),
            };

            // The report has been acted on, so contacts that began in it stop
            // counting as new. Runs even on error, or a failed key press would
            // leave the contact eligible to fire again on the next report.
            ctx.contacts.end_report();
            outcome?;
        }
        _ => {}
    }

    Ok(())
}

/// Fires a held corner once it clears `CORNER_HOLD`, or drops it if the finger
/// wandered out of the zone.
///
/// Called on every sync report while a finger is down, so the action lands mid-hold
/// the way the hardware numpad does. A finger held perfectly still emits no further
/// reports, so `handle_finger_event` re-checks on release as a fallback — between
/// them, both a jittery and a motionless hold work.
///
/// Only this path *cancels*, because only it can tell "still in the zone, not yet
/// long enough" from "gone". The release path re-tests the position instead.
fn poll_pending_corner(ctx: &mut DriverContext) -> Result<()> {
    let Some(pending) = ctx.pending_corner else {
        return Ok(());
    };
    if pending.fired {
        return Ok(());
    }

    let corner_now = corner_at_position(ctx.layout, ctx.state.current_position);
    if corner_now != pending.corner {
        debug!(
            "Corner hold cancelled: finger left the {:?} zone",
            pending.corner
        );
        ctx.pending_corner = None;
        return Ok(());
    }

    if pending.should_fire(Instant::now(), corner_now) {
        if let Some(pending) = ctx.pending_corner.as_mut() {
            pending.fired = true;
        }
        activate_corner(pending.corner, ctx)?;
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

        // Fallback for a hold that emitted no further reports: a finger resting
        // perfectly still never reaches poll_pending_corner. The position is
        // re-tested here rather than trusting that path to have cancelled already —
        // a report carrying both the last movement and the lift comes straight
        // here, so this is the only check that sees the slide-out at all.
        if let Some(pending) = ctx.pending_corner.take() {
            let corner_now = corner_at_position(ctx.layout, ctx.state.current_position);
            if pending.should_fire(Instant::now(), corner_now) {
                activate_corner(pending.corner, ctx)?;
            } else if !pending.fired && corner_now != pending.corner {
                debug!(
                    "Corner hold dropped on release: finger left the {:?} zone",
                    pending.corner
                );
            }
        }

        release_pressed_key(ctx)?;
    } else if value == 1 {
        // Finger down - handle corner detection or key press
        debug!(
            "Finger down at x={:.2}, y={:.2}",
            ctx.state.current_position.x, ctx.state.current_position.y
        );

        // BTN_TOOL_FINGER also returns to 1 when the pad drops from two fingers
        // back to one, but the finger left behind was already resting there — the
        // user never tapped it. Neither a key nor a corner may fire for it, and
        // with nothing down at all there is only a stale coordinate to act on.
        if !ctx.contacts.primary_is_new() {
            debug!("Ignoring finger down: no new contact began in this report");
            return Ok(());
        }

        // A key is still held from an earlier touch; wait for its release.
        if ctx.state.pressed_key.is_some() {
            return Ok(());
        }

        ctx.pending_corner = None;

        let position = ctx.state.current_position;
        let corner = corner_at_position(ctx.layout, position);

        if corner != Corner::None {
            if ctx.state.enabled {
                // The pad is grabbed, so this touch is unambiguously ours and
                // nothing else can act on it. Respond immediately.
                activate_corner(corner, ctx)?;
            } else {
                debug!(
                    "Corner {:?} armed, needs a {}ms hold",
                    corner,
                    CORNER_HOLD.as_millis()
                );
                ctx.pending_corner = Some(PendingCorner {
                    corner,
                    since: Instant::now(),
                    fired: false,
                });
            }

            return Ok(());
        }

        // Numpad key press
        if ctx.state.enabled {
            if let Some(key) = ctx.layout.key_at_position(position.x, position.y) {
                debug!(
                    "Key press: {:?} at x={:.2}, y={:.2}",
                    key, position.x, position.y
                );

                ctx.virtual_kb.press_key(key)?;
                ctx.state.pressed_key = Some(key);
            }
        }
    }

    Ok(())
}

/// Runs the action bound to a corner, given the numpad's current state.
fn activate_corner(corner: Corner, ctx: &mut DriverContext) -> Result<()> {
    match corner {
        Corner::TopRight => {
            // Toggle numpad. `enabled` is owned by enable_numpad/disable_numpad
            // so it can never disagree with the grab.
            if !ctx.state.enabled {
                enable_numpad(ctx)?;
                info!("Numpad enabled");
            } else {
                disable_numpad(ctx)?;
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
        Corner::None => {}
    }

    Ok(())
}

fn corner_at_position(layout: &dyn NumpadLayout, position: TouchPosition) -> Corner {
    if layout.is_toggle_position(position.x, position.y) {
        Corner::TopRight
    } else if layout.is_calc_position(position.x, position.y) {
        Corner::TopLeft
    } else {
        Corner::None
    }
}

/// Which direction a numpad toggle is going, for `decide_numlock`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NumlockPhase {
    Enabling,
    Disabling,
}

/// What a numpad toggle owes NumLock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NumlockDecision {
    /// Whether to send a NumLock click.
    click: bool,
    /// What `numlock_toggled_by_driver` becomes once this is carried out — i.e.
    /// whether the driver still owes a restoring toggle afterwards.
    owed: bool,
}

/// Decides the NumLock move for a toggle, from a *fresh* `LED_NUML` reading and
/// the outstanding debt.
///
/// `numlock_on` is `None` when the keyboard could not be read at all, which is
/// emphatically not "NumLock is off" — the keyboard may be absent, or its event
/// node busy or renumbered after resume.
///
/// Kept free of `VirtualKeyboard` and `/dev/input` so the truth table can be
/// tested without hardware; the caller performs the click and the bookkeeping.
fn decide_numlock(phase: NumlockPhase, numlock_on: Option<bool>, owed: bool) -> NumlockDecision {
    match phase {
        // Only click when NumLock is known to be off. On an unreadable reading,
        // guessing wrong turns NumLock *off* — every key then emits its cursor
        // variant and the driver would later "restore" it back on. Doing nothing
        // still leaves the user their own NumLock key.
        NumlockPhase::Enabling => match numlock_on {
            Some(false) => NumlockDecision {
                click: true,
                owed: true,
            },
            _ => NumlockDecision { click: false, owed },
        },
        // The debt is settled either way: either the driver clicks NumLock back
        // off, or the user beat it to it and there is nothing left to restore.
        // Clicking on the flag alone is what left NumLock inverted before.
        // An unreadable reading falls back to the flag — the driver knows it
        // turned NumLock on, so restoring is the better guess.
        NumlockPhase::Disabling => NumlockDecision {
            click: owed && numlock_on != Some(false),
            owed: false,
        },
    }
}

/// Carries out a `NumlockDecision`, recording the debt only once the click has
/// actually landed — a failed click leaves NumLock untouched, so the flag must
/// keep describing reality.
fn apply_numlock_decision(ctx: &mut DriverContext, decision: NumlockDecision, action: &str) {
    if !decision.click {
        ctx.numlock_toggled_by_driver = decision.owed;
        return;
    }

    match ctx.virtual_kb.click_numlock() {
        Ok(()) => ctx.numlock_toggled_by_driver = decision.owed,
        Err(e) => warn!("Failed to {} NumLock: {}", action, e),
    }
}

fn enable_numpad(ctx: &mut DriverContext) -> Result<()> {
    ctx.touchpad.grab()?;

    // The pad is grabbed from here on, so record that before anything that can
    // fail. Otherwise a later error leaves the pointer dead while the driver
    // still believes the numpad is off, and only a toggle tap can recover it.
    ctx.state.enabled = true;

    // Re-read rather than trusting a value latched at startup: the user may have
    // toggled NumLock from the keyboard since, and acting on a stale reading
    // leaves it inverted once the numpad is switched back off.
    let numlock_on = read_numlock_state(ctx.keyboard_path.as_deref());
    if numlock_on.is_none() {
        warn!("NumLock state unknown, leaving it alone: press NumLock yourself if the numpad moves the cursor instead of typing digits");
    }

    // NumLock and the LED are cosmetic next to the grab: warn, but stay enabled.
    let decision = decide_numlock(
        NumlockPhase::Enabling,
        numlock_on,
        ctx.numlock_toggled_by_driver,
    );
    apply_numlock_decision(ctx, decision, "turn on");

    if let Some(ref mut led_ctrl) = ctx.led {
        if let Err(e) = led_ctrl.set_brightness(ctx.state.brightness) {
            warn!("Failed to set LED brightness: {}", e);
        }
    }

    Ok(())
}

fn disable_numpad(ctx: &mut DriverContext) -> Result<()> {
    // A stuck key must not stop us from releasing the grab.
    if let Err(e) = release_pressed_key(ctx) {
        warn!("Failed to release held key: {}", e);
    }

    let ungrab = ctx.touchpad.ungrab();
    if ungrab.is_ok() {
        ctx.state.enabled = false;
    }

    // Restore NumLock and the LED even if the ungrab failed, so a failure there
    // does not also leave the backlight on.
    //
    // Re-read for the same reason the enable path does: the user may have pressed
    // NumLock themselves since, and clicking on the flag alone would then leave it
    // stuck on with nothing else to undo it.
    let numlock_on = read_numlock_state(ctx.keyboard_path.as_deref());
    if numlock_on.is_none() && ctx.numlock_toggled_by_driver {
        warn!("NumLock state unknown, restoring it anyway since the driver turned it on");
    }
    let decision = decide_numlock(
        NumlockPhase::Disabling,
        numlock_on,
        ctx.numlock_toggled_by_driver,
    );
    apply_numlock_decision(ctx, decision, "restore");

    if let Some(ref mut led_ctrl) = ctx.led {
        if let Err(e) = led_ctrl.turn_off() {
            warn!("Failed to turn off LED: {}", e);
        }
    }

    ungrab.context("Failed to ungrab touchpad")
}

fn release_pressed_key(ctx: &mut DriverContext) -> Result<()> {
    if let Some(key) = ctx.state.pressed_key.take() {
        debug!("Releasing key: {:?}", key);
        ctx.virtual_kb.release_key(key)?;
    }
    Ok(())
}

fn cleanup(ctx: &mut DriverContext) {
    if let Err(e) = disable_numpad(ctx) {
        warn!("Failed to fully clean up driver state: {}", e);
    }
}

/// Reads the physical keyboard's `LED_NUML`.
///
/// `None` means the state could not be read, which callers must keep distinct
/// from `Some(false)`: acting on a guess is how NumLock ends up inverted.
fn read_numlock_state(keyboard_path: Option<&str>) -> Option<bool> {
    let Some(keyboard_path) = keyboard_path else {
        debug!("No keyboard device was detected, so NumLock state is unknown");
        return None;
    };

    match evdev::Device::open(keyboard_path).and_then(|device| device.get_led_state()) {
        Ok(leds) => {
            let numlock_on = leds.contains(LedCode::LED_NUML);
            debug!("NumLock reads as {}", numlock_on);
            Some(numlock_on)
        }
        Err(e) => {
            warn!("Could not read NumLock state from {}: {}", keyboard_path, e);
            None
        }
    }
}

fn install_signal_handlers() {
    unsafe extern "C" fn handle_signal(_: libc::c_int) {
        SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);

        // Everything below must stay async-signal-safe: an atomic load and a raw
        // one-byte write, no allocation, no logging, no formatting. errno is saved
        // and restored because the interrupted code may be about to read its own.
        let fd = WAKEUP_WRITE_FD.load(Ordering::SeqCst);
        if fd < 0 {
            return;
        }

        let errno = libc::__errno_location();
        let saved_errno = *errno;
        let byte = 1u8;
        // A one-byte write cannot land partially, so there is no partial case to
        // handle. EAGAIN means an earlier wakeup is still unread, which serves the
        // same purpose; only EINTR is worth retrying.
        while libc::write(fd, std::ptr::addr_of!(byte).cast(), 1) < 0 && *errno == libc::EINTR {}
        *errno = saved_errno;
    }

    // Installed via sigaction with sa_flags = 0 rather than libc::signal, which on
    // glibc implies SA_RESTART. The event loop blocks on the touchpad fd, and a
    // restarted wait would swallow the signal until the user next touched the pad —
    // leaving `systemctl stop` to hang until its timeout and SIGKILL, so cleanup()
    // never ran and the LED stayed lit. Without SA_RESTART the wait fails with
    // EINTR, the loop re-checks SHUTDOWN_REQUESTED, and shutdown is immediate.
    // The pipe byte covers the other half: a signal that lands just *before* the
    // wait begins has no call to interrupt, and only the buffered byte wakes it.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = handle_signal as *const () as libc::sighandler_t;
        action.sa_flags = 0;
        libc::sigemptyset(&mut action.sa_mask);

        for signal in [libc::SIGINT, libc::SIGTERM] {
            if libc::sigaction(signal, &action, std::ptr::null_mut()) != 0 {
                warn!(
                    "Failed to install handler for signal {}: {}",
                    signal,
                    std::io::Error::last_os_error()
                );
            }
        }
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
    fn g634jy_corner_detection_respects_layout_toggle_dead_zone() {
        let layout = layouts::G634jyLayout::new();

        assert_eq!(
            corner_at_position(&layout, TouchPosition { x: 0.32, y: 0.40 }),
            Corner::None
        );
        assert_eq!(
            corner_at_position(&layout, TouchPosition { x: 0.90, y: 0.20 }),
            Corner::TopRight
        );
        assert_eq!(
            corner_at_position(&layout, TouchPosition { x: 0.02, y: 0.02 }),
            Corner::TopLeft
        );
    }

    #[test]
    fn g634jy_calc_corner_no_longer_shadows_the_seven_key() {
        let layout = layouts::G634jyLayout::new();

        // (0.05, 0.05) is the top-left corner of the "7" band. The old hard-coded
        // 0.06 x 0.07 corner claimed it and cycled brightness instead of typing 7.
        let position = TouchPosition { x: 0.05, y: 0.05 };
        assert_eq!(corner_at_position(&layout, position), Corner::None);
        assert_eq!(
            layout.key_at_position(position.x, position.y),
            Some(KeyCode::KEY_KP7)
        );
    }

    fn pending(corner: Corner, fired: bool) -> (PendingCorner, Instant) {
        // Built forwards from `since` rather than backwards from `now`, so the
        // arithmetic cannot underflow the platform clock.
        let since = Instant::now();
        (
            PendingCorner {
                corner,
                since,
                fired,
            },
            since + CORNER_HOLD,
        )
    }

    /// The regression: when the finger's last movement out of the zone and its
    /// `BTN_TOOL_FINGER=0` land in the same report, `process_event` dispatches to
    /// `handle_finger_event` and `poll_pending_corner` never runs — so the release
    /// path is the only thing standing between a slide-out and an unwanted toggle.
    #[test]
    fn a_hold_that_ends_outside_the_zone_does_not_fire() {
        let (held, after_hold) = pending(Corner::TopLeft, false);

        assert!(
            !held.should_fire(after_hold, Corner::None),
            "a corner hold fired after the finger had already left the zone"
        );
        // Slid from one corner straight into the other: still not the corner that
        // was armed, so still nothing.
        assert!(!held.should_fire(after_hold, Corner::TopRight));
    }

    #[test]
    fn a_motionless_hold_still_fires_on_release() {
        let (held, after_hold) = pending(Corner::TopLeft, false);
        assert!(held.should_fire(after_hold, Corner::TopLeft));
    }

    #[test]
    fn a_hold_shorter_than_the_threshold_never_fires() {
        let (held, _) = pending(Corner::TopRight, false);
        assert!(!held.should_fire(held.since, Corner::TopRight));
        assert!(!held.should_fire(
            held.since + CORNER_HOLD - Duration::from_millis(1),
            Corner::TopRight
        ));
    }

    /// `poll_pending_corner` fires mid-hold and marks the pending corner; the
    /// release that follows must not run the action a second time.
    #[test]
    fn an_already_fired_hold_does_not_repeat_on_release() {
        let (held, after_hold) = pending(Corner::TopRight, true);
        assert!(!held.should_fire(after_hold, Corner::TopRight));
    }

    fn decision(click: bool, owed: bool) -> NumlockDecision {
        NumlockDecision { click, owed }
    }

    /// Enabling only acts on a reading it trusts: NumLock must be on or the keypad
    /// codes come out as cursor movement, but an unreadable state is not "off".
    #[test]
    fn enabling_clicks_numlock_only_when_it_is_known_to_be_off() {
        assert_eq!(
            decide_numlock(NumlockPhase::Enabling, Some(false), false),
            decision(true, true)
        );
        assert_eq!(
            decide_numlock(NumlockPhase::Enabling, Some(true), false),
            decision(false, false)
        );
        assert_eq!(
            decide_numlock(NumlockPhase::Enabling, None, false),
            decision(false, false)
        );
    }

    /// The stale flag must not veto the click: a failed restore leaves it set while
    /// NumLock is genuinely off, and skipping the toggle there was unrecoverable
    /// short of restarting the driver.
    #[test]
    fn enabling_ignores_a_stale_debt_flag_and_trusts_the_fresh_reading() {
        assert_eq!(
            decide_numlock(NumlockPhase::Enabling, Some(false), true),
            decision(true, true)
        );
        // Already on: nothing to do, but the debt still stands.
        assert_eq!(
            decide_numlock(NumlockPhase::Enabling, Some(true), true),
            decision(false, true)
        );
        assert_eq!(
            decide_numlock(NumlockPhase::Enabling, None, true),
            decision(false, true)
        );
    }

    #[test]
    fn disabling_restores_numlock_only_while_it_is_still_on() {
        assert_eq!(
            decide_numlock(NumlockPhase::Disabling, Some(true), true),
            decision(true, false)
        );
        // The user turned NumLock off themselves; clicking would switch it back on.
        assert_eq!(
            decide_numlock(NumlockPhase::Disabling, Some(false), true),
            decision(false, false)
        );
        // Unreadable: the driver knows it clicked, so restoring is the better guess.
        assert_eq!(
            decide_numlock(NumlockPhase::Disabling, None, true),
            decision(true, false)
        );
    }

    /// Without a debt the driver never touches NumLock on the way out — whatever
    /// state it is in is the user's own.
    #[test]
    fn disabling_leaves_numlock_alone_when_nothing_is_owed() {
        for reading in [Some(true), Some(false), None] {
            assert_eq!(
                decide_numlock(NumlockPhase::Disabling, reading, false),
                decision(false, false),
                "reading {:?} moved NumLock with no debt outstanding",
                reading
            );
        }
    }

    /// The pipe outranks the touchpad, so a shutdown is never made to wait behind a
    /// batch of events, and anything at all on the touchpad fd is left to the read
    /// to interpret.
    #[test]
    fn classifies_a_poll_result_by_what_needs_doing() {
        assert_eq!(classify_wakeup(libc::POLLIN, 0), Wakeup::Touchpad);
        assert_eq!(classify_wakeup(libc::POLLERR, 0), Wakeup::Touchpad);
        assert_eq!(classify_wakeup(0, libc::POLLIN), Wakeup::Signal);
        assert_eq!(classify_wakeup(libc::POLLIN, libc::POLLIN), Wakeup::Signal);
        // Nothing ready at all: harmless, and re-checking the flag is the safe move.
        assert_eq!(classify_wakeup(0, 0), Wakeup::Signal);
    }

    /// The lost-wakeup window itself: the signal is delivered *before* the wait
    /// begins, so there is no in-flight syscall for `EINTR` to interrupt and the
    /// buffered byte is the only thing that can end the wait. With a bare atomic
    /// and a blocking read this is precisely where the driver used to sleep until
    /// the next touch, and `systemctl stop` timed out into a SIGKILL.
    ///
    /// This installs the real handlers for the rest of the test binary and leaves
    /// `SHUTDOWN_REQUESTED` set. Nothing else in the suite reads either, and no
    /// other test raises a signal.
    #[test]
    fn wakes_a_wait_that_starts_after_the_signal_was_delivered() {
        let wakeup = ShutdownWakeup::install().expect("failed to create the wakeup pipe");
        install_signal_handlers();

        // Stands in for a touchpad nobody is touching: never readable, so only the
        // pipe can end the wait.
        let mut idle = [-1 as libc::c_int; 2];
        assert_eq!(
            unsafe { libc::pipe2(idle.as_mut_ptr(), libc::O_CLOEXEC) },
            0,
            "failed to create the idle stand-in pipe"
        );

        assert_eq!(unsafe { libc::raise(libc::SIGTERM) }, 0);
        assert!(
            SHUTDOWN_REQUESTED.load(Ordering::SeqCst),
            "the handler did not run"
        );

        // Waited on off-thread so a regression fails the test instead of hanging it.
        let (tx, rx) = std::sync::mpsc::channel();
        let touchpad_fd = idle[0];
        std::thread::spawn(move || {
            let _ = tx.send(wait_for_wakeup(touchpad_fd, Some(&wakeup)).map_err(|e| e.to_string()));
        });

        let outcome = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("wait_for_wakeup never returned: the buffered byte did not wake poll");
        assert_eq!(outcome, Ok(Wakeup::Signal));

        for fd in idle {
            unsafe { libc::close(fd) };
        }
    }

    /// Whatever a layout claims as a corner must not also be a key: corners are
    /// resolved first, so an overlap silently swallows the key.
    #[test]
    fn corner_zones_never_shadow_a_key() {
        let layout = layouts::G634jyLayout::new();

        for xi in 0..=200 {
            for yi in 0..=200 {
                let position = TouchPosition {
                    x: xi as f64 / 200.0,
                    y: yi as f64 / 200.0,
                };
                if corner_at_position(&layout, position) != Corner::None {
                    assert_eq!(
                        layout.key_at_position(position.x, position.y),
                        None,
                        "corner zone shadows a key at x={}, y={}",
                        position.x,
                        position.y
                    );
                }
            }
        }
    }
}
