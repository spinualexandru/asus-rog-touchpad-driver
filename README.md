# ASUS ROG Touchpad Numpad Driver

A Linux driver written in Rust that enables the illuminated numpad feature on ASUS ROG laptop touchpads.

## Features

- **Numpad Toggle**: Tap the top-right corner of the touchpad to enable/disable the numpad overlay
- **LED Backlight Control**: Native I2C communication to control the numpad LED backlight
- **Brightness Cycling**: Tap the top-left corner (when numpad is active) to cycle through brightness levels
- **Calculator Shortcut**: Tap the top-left corner (when numpad is off) to launch the calculator
- **Virtual Keyboard**: Injects numpad key events via uinput
- **Auto-restart**: Systemd service with automatic restart on failure
- **Low Resource Usage**: Efficient Rust implementation with minimal CPU overhead

## Supported Devices

| Model | Touchpad ID | Status |
|-------|-------------|--------|
| ROG Strix SCAR 16 G634JY | ASUF1416:00 2808:0108 | Fully supported |
| ROG Strix SCAR 16 G634JYR | ASUF1416:00 2808:0108 | Fully supported |

More ASUS models with numpad-enabled touchpads can be added by creating new layout configurations.

## Installation

### Arch Linux (PKGBUILD)

```bash
git clone https://github.com/spinualexandru/asus-rog-touchpad-driver.git
cd asus-rog-touchpad-driver
makepkg -si
```

This builds and installs a proper Arch package with:
- Automatic dependency resolution
- Clean uninstall via `pacman -R asus-rog-touchpad-numpad`
- Service auto-enabled on install

### Using Just (Any Distribution)

Requires [just](https://github.com/casey/just) command runner.

```bash
git clone https://github.com/spinualexandru/asus-rog-touchpad-driver.git
cd asus-rog-touchpad-driver
just install
```

This will:
1. Check cargo is configured
2. Build the release binary
3. Install the binary to `/usr/bin/`
4. Configure the i2c-dev kernel module
5. Install and start the systemd service

To see required dependencies for your distribution:
```bash
just deps
```

## Manual Installation

### Dependencies

**Arch Linux:**
```bash
sudo pacman -S just rust libevdev
```

**Debian/Ubuntu:**
```bash
sudo apt install just cargo libevdev-dev pkg-config
```

**Fedora:**
```bash
sudo dnf install just rust cargo libevdev-devel pkg-config
```

### Build

```bash
cargo build --release
```

### Install

```bash
# Install binary
sudo cp target/release/asus-rog-touchpad-numpad /usr/bin/
sudo chmod 755 /usr/bin/asus-rog-touchpad-numpad

# Load i2c-dev kernel module
sudo modprobe i2c-dev
echo "i2c-dev" | sudo tee /etc/modules-load.d/i2c-dev.conf

# Install systemd service
sudo cp asus-rog-touchpad.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now asus-rog-touchpad
```

## Usage

### Touchpad Gestures

| Gesture | Numpad Off | Numpad On |
|---------|------------|-----------|
| Tap top-right corner | Enable numpad | Disable numpad |
| Tap top-left corner | Launch calculator | Cycle brightness |
| Tap numpad area | Normal touchpad | Enter numpad key |

### Numpad Layout (G634JY)

```
┌─────┬─────┬─────┬─────┬─────┐
│  7  │  8  │  9  │  /  │ ⌫  │
├─────┼─────┼─────┼─────┼─────┤
│  4  │  5  │  6  │  *  │ ⌫  │
├─────┼─────┼─────┼─────┼─────┤
│  1  │  2  │  3  │  -  │ ⏎  │
├─────┼─────┼─────┼─────┼─────┤
│  0  │  0  │  .  │  +  │ ⏎  │
└─────┴─────┴─────┴─────┴─────┘
```

### Service Management

```bash
just status   # Check status
just restart  # Restart service
just stop     # Stop service
just logs     # View logs (follow)
just run      # Run manually with debug logging
```

## Configuration

### Command Line Arguments

```
asus-rog-touchpad-numpad [MODEL] [PERCENTAGE_KEY]

Arguments:
  MODEL           Layout model to use (default: g634jy)
  PERCENTAGE_KEY  Key code for % symbol (default: 6 for Qwerty, 40 for Azerty)
```

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `RUST_LOG` | Log level (error, warn, info, debug, trace) | info |

### Debug Mode

To run with verbose logging:

```bash
sudo RUST_LOG=debug /usr/bin/asus-rog-touchpad-numpad
```

## Troubleshooting

### Numpad not activating

1. Check if the service is running:
   ```bash
   systemctl status asus-rog-touchpad
   ```

2. Check if i2c-dev module is loaded:
   ```bash
   lsmod | grep i2c_dev
   ```

3. Verify touchpad is detected:
   ```bash
   grep -E "ASUE|ASUF|ELAN" /proc/bus/input/devices
   ```

### LED backlight not working

1. Check I2C bus detection:
   ```bash
   sudo i2cdetect -l | grep -i designware
   ```

2. The driver will log a warning if LED control fails but will continue to function for numpad input.

### Permission denied errors

Ensure you're running as root or the service is running with proper permissions:
```bash
sudo systemctl restart asus-rog-touchpad
```

### Service fails to start on boot

Some systems may need a delay before the touchpad is ready. Edit the service file:

```bash
sudo systemctl edit asus-rog-touchpad
```

Add:
```ini
[Service]
ExecStartPre=/bin/sleep 2
```

## Uninstall

**Arch Linux (if installed via PKGBUILD):**
```bash
sudo pacman -R asus-rog-touchpad-numpad
```

**Other distributions (if installed via just):**
```bash
just uninstall
```

## Development

### Project Structure

```
src/
├── main.rs           # Entry point, CLI, event loop
├── error.rs          # Custom error types
├── device/           # Device detection
├── i2c/              # I2C LED control
├── input/            # Touchpad & virtual keyboard
├── layouts/          # Numpad layout definitions
└── numpad/           # State machine
```

### Adding a New Layout

1. Create a new file in `src/layouts/` (e.g., `newmodel.rs`)
2. Implement the `NumpadLayout` trait
3. Add the layout to `src/layouts/mod.rs`
4. Rebuild and test

Example layout:

```rust
use super::NumpadLayout;
use evdev::KeyCode;

pub struct NewModelLayout {
    keys: [[KeyCode; 5]; 4],
}

impl NumpadLayout for NewModelLayout {
    fn name(&self) -> &'static str { "newmodel" }
    fn cols(&self) -> u32 { 5 }
    fn rows(&self) -> u32 { 4 }
    fn top_offset(&self) -> f64 { 0.10 }
    // ... implement remaining methods
}
```

### Building for Development

```bash
just build        # Release build
just build-debug  # Debug build
just run          # Build and run with debug logging
just run-debug    # Build debug and run with logging
just clean        # Clean build artifacts
```

Or with cargo directly:
```bash
cargo build --release
RUST_LOG=debug sudo -E ./target/release/asus-rog-touchpad-numpad
```

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.

When adding support for a new device:
1. Run with debug logging to capture touchpad coordinates
2. Create a new layout file
3. Test thoroughly before submitting

## License

This project is licensed under the GPL 2.0 - see the [LICENSE](LICENSE) file for details.
