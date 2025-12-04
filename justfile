# ASUS ROG Touchpad Numpad Driver

set shell := ["bash", "-c"]

binary_name := "asus-rog-touchpad-numpad"
service_name := "asus-rog-touchpad"
install_dir := "/usr/bin"
service_dir := "/etc/systemd/system"
modules_dir := "/etc/modules-load.d"

# Colors
green := '\033[0;32m'
yellow := '\033[1;33m'
red := '\033[0;31m'
nc := '\033[0m'

# Default: show available recipes
default:
    @just --list

# Build release binary
build:
    @echo -e "{{green}}[INFO]{{nc}} Building release binary..."
    cargo build --release
    @test -f "target/release/{{binary_name}}" || (echo -e "{{red}}[ERROR]{{nc}} Build failed - binary not found" && exit 1)
    @echo -e "{{green}}[INFO]{{nc}} Build successful"

# Build debug binary
build-debug:
    @echo -e "{{green}}[INFO]{{nc}} Building debug binary..."
    cargo build

# Run with debug logging (requires sudo)
run: build
    sudo RUST_LOG=debug ./target/release/{{binary_name}}

# Run debug build with logging
run-debug: build-debug
    sudo RUST_LOG=debug ./target/debug/{{binary_name}}

# Show required dependencies for your distribution
deps:
    #!/usr/bin/bash
    echo -e "{{green}}[INFO]{{nc}} Required dependencies:"
    if command -v pacman &> /dev/null; then
        echo "  pacman -S --needed rust libevdev"
    elif command -v apt &> /dev/null; then
        echo "  apt install cargo libevdev-dev pkg-config"
    elif command -v dnf &> /dev/null; then
        echo "  dnf install rust cargo libevdev-devel pkg-config"
    elif command -v zypper &> /dev/null; then
        echo "  zypper install rust cargo libevdev-devel pkg-config"
    else
        echo "  - Rust/Cargo (https://rustup.rs)"
        echo "  - libevdev development files"
        echo "  - pkg-config"
    fi

# Check cargo is properly configured
check-cargo:
    #!/usr/bin/bash
    set -e
    if ! command -v cargo &> /dev/null; then
        echo -e "{{red}}[ERROR]{{nc}} cargo not found. Please install Rust first."
        exit 1
    fi
    echo -e "{{green}}[INFO]{{nc}} Found $(cargo --version)"

# Install binary to system (requires sudo)
install-binary: build
    @echo -e "{{green}}[INFO]{{nc}} Installing binary to {{install_dir}}..."
    sudo cp "target/release/{{binary_name}}" "{{install_dir}}/"
    sudo chmod 755 "{{install_dir}}/{{binary_name}}"
    @echo -e "{{green}}[INFO]{{nc}} Binary installed"

# Setup i2c-dev kernel module (requires sudo)
setup-i2c:
    #!/usr/bin/bash
    set -e
    echo -e "{{green}}[INFO]{{nc}} Setting up i2c-dev kernel module..."
    sudo modprobe i2c-dev || echo -e "{{yellow}}[WARN]{{nc}} Could not load i2c-dev module"
    if [[ ! -f "{{modules_dir}}/i2c-dev.conf" ]]; then
        echo "i2c-dev" | sudo tee "{{modules_dir}}/i2c-dev.conf" > /dev/null
        echo -e "{{green}}[INFO]{{nc}} Added i2c-dev to load at boot"
    else
        echo -e "{{green}}[INFO]{{nc}} i2c-dev already configured for boot"
    fi

# Install systemd service (requires sudo)
install-service:
    @echo -e "{{green}}[INFO]{{nc}} Installing systemd service..."
    sudo cp "{{service_name}}.service" "{{service_dir}}/"
    sudo systemctl daemon-reload
    sudo systemctl enable "{{service_name}}"
    @echo -e "{{green}}[INFO]{{nc}} Service installed and enabled"

# Start the service (requires sudo)
start:
    #!/usr/bin/bash
    echo -e "{{green}}[INFO]{{nc}} Starting service..."
    sudo systemctl start "{{service_name}}"
    sleep 1
    if systemctl is-active --quiet "{{service_name}}"; then
        echo -e "{{green}}[INFO]{{nc}} Service started successfully"
    else
        echo -e "{{yellow}}[WARN]{{nc}} Service may not have started correctly"
        echo -e "{{yellow}}[WARN]{{nc}} Check status with: just status"
        echo -e "{{yellow}}[WARN]{{nc}} Check logs with: just logs"
    fi

# Stop the service (requires sudo)
stop:
    @echo -e "{{green}}[INFO]{{nc}} Stopping service..."
    -sudo systemctl stop "{{service_name}}"
    @echo -e "{{green}}[INFO]{{nc}} Service stopped"

# Restart the service (requires sudo)
restart:
    @echo -e "{{green}}[INFO]{{nc}} Restarting service..."
    sudo systemctl restart "{{service_name}}"
    @echo -e "{{green}}[INFO]{{nc}} Service restarted"

# Show service status
status:
    systemctl status "{{service_name}}"

# Follow service logs
logs:
    journalctl -u "{{service_name}}" -f

# Full install: build, install binary, setup i2c, install and start service
install: check-cargo build install-binary setup-i2c install-service start
    @echo ""
    @echo "========================================"
    @echo -e " {{green}}Installation complete!{{nc}}"
    @echo "========================================"
    @echo ""
    @echo "Usage:"
    @echo "  - Tap top-right corner of touchpad to toggle numpad"
    @echo "  - Tap top-left corner to cycle brightness (numpad on)"
    @echo "  - Tap top-left corner to open calculator (numpad off)"
    @echo ""
    @echo "Commands:"
    @echo "  just status   - Check service status"
    @echo "  just restart  - Restart service"
    @echo "  just logs     - View logs"
    @echo "  just uninstall - Uninstall"
    @echo ""

# Remove service (requires sudo)
remove-service:
    #!/usr/bin/bash
    echo -e "{{green}}[INFO]{{nc}} Stopping service..."
    if systemctl is-active --quiet "{{service_name}}" 2>/dev/null; then
        sudo systemctl stop "{{service_name}}"
        echo -e "{{green}}[INFO]{{nc}} Service stopped"
    else
        echo -e "{{green}}[INFO]{{nc}} Service was not running"
    fi

    echo -e "{{green}}[INFO]{{nc}} Disabling service..."
    if systemctl is-enabled --quiet "{{service_name}}" 2>/dev/null; then
        sudo systemctl disable "{{service_name}}"
        echo -e "{{green}}[INFO]{{nc}} Service disabled"
    else
        echo -e "{{green}}[INFO]{{nc}} Service was not enabled"
    fi

    echo -e "{{green}}[INFO]{{nc}} Removing service file..."
    if [[ -f "{{service_dir}}/{{service_name}}.service" ]]; then
        sudo rm -f "{{service_dir}}/{{service_name}}.service"
        sudo systemctl daemon-reload
        echo -e "{{green}}[INFO]{{nc}} Service file removed"
    else
        echo -e "{{green}}[INFO]{{nc}} Service file not found"
    fi

# Remove binary (requires sudo)
remove-binary:
    #!/usr/bin/bash
    echo -e "{{green}}[INFO]{{nc}} Removing binary..."
    if [[ -f "{{install_dir}}/{{binary_name}}" ]]; then
        sudo rm -f "{{install_dir}}/{{binary_name}}"
        echo -e "{{green}}[INFO]{{nc}} Binary removed"
    else
        echo -e "{{green}}[INFO]{{nc}} Binary not found"
    fi

# Full uninstall: remove service and binary
uninstall: remove-service remove-binary
    @echo ""
    @echo "========================================"
    @echo -e " {{green}}Uninstall complete!{{nc}}"
    @echo "========================================"
    @echo ""
    @echo "Note: i2c-dev.conf was kept in {{modules_dir}}/"
    @echo "Remove manually if no longer needed."
    @echo ""

# Clean build artifacts
clean:
    cargo clean
