use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const BINARY_NAME: &str = "asus-rog-touchpad-numpad";
const SERVICE_NAME: &str = "asus-rog-touchpad";
const INSTALL_DIR: &str = "/usr/bin";
const SERVICE_DIR: &str = "/etc/systemd/system";
const MODULES_DIR: &str = "/etc/modules-load.d";

#[derive(Debug, Parser)]
#[command(author, version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<CliCommand>,
}

#[derive(Debug, Subcommand)]
pub enum CliCommand {
    /// Run the touchpad numpad driver.
    Run(RunArgs),

    /// Build the release binary.
    Build,

    /// Build the debug binary.
    BuildDebug,

    /// Build and run the debug binary with debug logging.
    RunDebug(RunArgs),

    /// Show distro-specific build/runtime dependencies.
    Deps,

    /// Check that Cargo is available.
    CheckCargo,

    /// Install the existing release binary to /usr/bin.
    InstallBinary,

    /// Load and persist the i2c-dev kernel module.
    SetupI2c,

    /// Install and enable the systemd service.
    InstallService,

    /// Start the systemd service.
    Start,

    /// Stop the systemd service.
    Stop,

    /// Restart the systemd service.
    Restart,

    /// Show systemd service status.
    Status,

    /// Follow systemd journal logs for the service.
    Logs,

    /// Install, configure, enable, and start the service.
    Install,

    /// Stop/disable/remove the service and installed binary.
    Uninstall,

    /// Stop, disable, and remove the systemd service.
    RemoveService,

    /// Remove the installed binary.
    RemoveBinary,

    /// Clean Cargo build artifacts.
    Clean,
}

#[derive(Args, Clone, Debug)]
pub struct RunArgs {
    /// Layout model to use.
    #[arg(default_value = "g634jy")]
    pub model: String,
}

impl Default for RunArgs {
    fn default() -> Self {
        Self {
            model: "g634jy".to_string(),
        }
    }
}

pub fn parse_cli() -> Cli {
    Cli::parse_from(args_with_legacy_run_subcommand(env::args_os().collect()))
}

fn args_with_legacy_run_subcommand(args: Vec<OsString>) -> Vec<OsString> {
    if args.len() > 1 {
        let first = args[1].to_string_lossy();
        if !first.starts_with('-') && !is_known_subcommand(&first) {
            let mut rewritten = Vec::with_capacity(args.len() + 1);
            rewritten.push(args[0].clone());
            rewritten.push(OsString::from("run"));
            rewritten.extend(args.iter().skip(1).cloned());
            return rewritten;
        }
    }

    args
}

pub fn execute_command(command: CliCommand) -> Result<()> {
    match command {
        CliCommand::Run(_) => unreachable!("driver run is handled by main"),
        CliCommand::Build => build_release(),
        CliCommand::BuildDebug => build_debug(),
        CliCommand::RunDebug(args) => run_debug(args),
        CliCommand::Deps => {
            print_deps();
            Ok(())
        }
        CliCommand::CheckCargo => check_cargo(),
        CliCommand::InstallBinary => install_binary(),
        CliCommand::SetupI2c => setup_i2c(),
        CliCommand::InstallService => install_service(),
        CliCommand::Start => start_service(),
        CliCommand::Stop => stop_service(),
        CliCommand::Restart => restart_service(),
        CliCommand::Status => systemctl(&["status", SERVICE_NAME]),
        CliCommand::Logs => journalctl_logs(),
        CliCommand::Install => install_all(),
        CliCommand::Uninstall => uninstall_all(),
        CliCommand::RemoveService => remove_service(),
        CliCommand::RemoveBinary => remove_binary(),
        CliCommand::Clean => run_command(Command::new("cargo").arg("clean")),
    }
}

fn is_known_subcommand(value: &str) -> bool {
    matches!(
        value,
        "run"
            | "build"
            | "build-debug"
            | "run-debug"
            | "deps"
            | "check-cargo"
            | "install-binary"
            | "setup-i2c"
            | "install-service"
            | "start"
            | "stop"
            | "restart"
            | "status"
            | "logs"
            | "install"
            | "uninstall"
            | "remove-service"
            | "remove-binary"
            | "clean"
    )
}

fn build_release() -> Result<()> {
    info("Building release binary...");
    run_command(Command::new("cargo").args(["build", "--release"]))?;

    let binary = release_binary_path();
    if !binary.is_file() {
        bail!("build failed: {} was not produced", binary.display());
    }

    info("Build successful");
    Ok(())
}

fn build_debug() -> Result<()> {
    info("Building debug binary...");
    run_command(Command::new("cargo").arg("build"))
}

fn run_debug(args: RunArgs) -> Result<()> {
    build_debug()?;
    let binary = Path::new("target").join("debug").join(BINARY_NAME);
    let mut command = Command::new(binary);
    command.env(
        "RUST_LOG",
        env::var("RUST_LOG").unwrap_or_else(|_| "debug".to_string()),
    );
    command.arg(args.model);
    run_command(&mut command)
}

fn print_deps() {
    info("Required dependencies:");
    if command_exists("pacman") {
        println!("  pacman -S --needed rust");
    } else if command_exists("apt") {
        println!("  apt install cargo");
    } else if command_exists("dnf") {
        println!("  dnf install rust cargo");
    } else if command_exists("zypper") {
        println!("  zypper install rust cargo");
    } else {
        println!("  - Rust/Cargo (https://rustup.rs)");
    }
}

fn check_cargo() -> Result<()> {
    let output = Command::new("cargo")
        .arg("--version")
        .output()
        .context("cargo not found. Please install Rust first.")?;

    if !output.status.success() {
        bail!("cargo exists but `cargo --version` failed");
    }

    let version = String::from_utf8_lossy(&output.stdout);
    info(format!("Found {}", version.trim()));
    Ok(())
}

fn install_binary() -> Result<()> {
    info(format!("Installing binary to {INSTALL_DIR}..."));
    let binary = release_binary_path();
    if !binary.is_file() {
        bail!(
            "release binary not found at {}; run `cargo build --release` first",
            binary.display()
        );
    }

    fs::create_dir_all(INSTALL_DIR).context("failed to create install directory")?;

    let destination = Path::new(INSTALL_DIR).join(BINARY_NAME);
    fs::copy(&binary, &destination)
        .with_context(|| format!("failed to copy binary to {}", destination.display()))?;
    fs::set_permissions(&destination, fs::Permissions::from_mode(0o755))
        .with_context(|| format!("failed to chmod {}", destination.display()))?;

    info("Binary installed");
    Ok(())
}

fn setup_i2c() -> Result<()> {
    info("Setting up i2c-dev kernel module...");
    if let Err(err) = run_command(Command::new("modprobe").arg("i2c-dev")) {
        warn(format!("Could not load i2c-dev module: {err}"));
    }

    fs::create_dir_all(MODULES_DIR).context("failed to create modules-load directory")?;
    let config = Path::new(MODULES_DIR).join("i2c-dev.conf");
    if config.exists() {
        info("i2c-dev already configured for boot");
    } else {
        fs::write(&config, "i2c-dev\n")
            .with_context(|| format!("failed to write {}", config.display()))?;
        info("Added i2c-dev to load at boot");
    }

    Ok(())
}

fn install_service() -> Result<()> {
    ensure_systemd_booted()?;
    info("Installing systemd service...");
    fs::create_dir_all(SERVICE_DIR).context("failed to create systemd service directory")?;

    let source = PathBuf::from(format!("{SERVICE_NAME}.service"));
    let destination = Path::new(SERVICE_DIR).join(format!("{SERVICE_NAME}.service"));
    fs::copy(&source, &destination).with_context(|| {
        format!(
            "failed to copy {} to {}",
            source.display(),
            destination.display()
        )
    })?;

    systemctl(&["daemon-reload"])?;
    systemctl(&["enable", SERVICE_NAME])?;
    info("Service installed and enabled");
    Ok(())
}

fn start_service() -> Result<()> {
    ensure_systemd_booted()?;
    info("Starting service...");
    systemctl(&["start", SERVICE_NAME])?;

    std::thread::sleep(std::time::Duration::from_secs(1));
    if systemctl_quiet(&["is-active", "--quiet", SERVICE_NAME]) {
        info("Service started successfully");
    } else {
        warn("Service may not have started correctly");
        warn(format!("Check status with: {BINARY_NAME} status"));
        warn(format!("Check logs with: {BINARY_NAME} logs"));
    }

    Ok(())
}

fn stop_service() -> Result<()> {
    ensure_systemd_booted()?;
    info("Stopping service...");
    if let Err(err) = systemctl(&["stop", SERVICE_NAME]) {
        warn(format!("Could not stop service: {err}"));
    }
    info("Service stopped");
    Ok(())
}

fn restart_service() -> Result<()> {
    ensure_systemd_booted()?;
    info("Restarting service...");
    systemctl(&["restart", SERVICE_NAME])?;
    info("Service restarted");
    Ok(())
}

fn journalctl_logs() -> Result<()> {
    run_command(Command::new("journalctl").args(["-u", SERVICE_NAME, "-f"]))
}

fn install_all() -> Result<()> {
    install_binary()?;
    setup_i2c()?;
    install_service()?;
    start_service()?;

    println!();
    println!("========================================");
    println!(" Installation complete!");
    println!("========================================");
    println!();
    println!("Usage:");
    println!("  - Tap top-right corner of touchpad to toggle numpad");
    println!("  - Tap top-left corner to cycle brightness (numpad on)");
    println!("  - Tap top-left corner to open calculator (numpad off)");
    println!();
    println!("Commands:");
    println!("  {BINARY_NAME} status    - Check service status");
    println!("  {BINARY_NAME} restart   - Restart service");
    println!("  {BINARY_NAME} logs      - View logs");
    println!("  {BINARY_NAME} uninstall - Uninstall");
    println!();

    Ok(())
}

fn uninstall_all() -> Result<()> {
    remove_service()?;
    remove_binary()?;

    println!();
    println!("========================================");
    println!(" Uninstall complete!");
    println!("========================================");
    println!();
    println!("Note: i2c-dev.conf was kept in {MODULES_DIR}/");
    println!("Remove manually if no longer needed.");
    println!();

    Ok(())
}

fn remove_service() -> Result<()> {
    ensure_systemd_booted()?;
    info("Stopping service...");
    if systemctl_quiet(&["is-active", "--quiet", SERVICE_NAME]) {
        systemctl(&["stop", SERVICE_NAME])?;
        info("Service stopped");
    } else {
        info("Service was not running");
    }

    info("Disabling service...");
    if systemctl_quiet(&["is-enabled", "--quiet", SERVICE_NAME]) {
        systemctl(&["disable", SERVICE_NAME])?;
        info("Service disabled");
    } else {
        info("Service was not enabled");
    }

    info("Removing service file...");
    let service_path = Path::new(SERVICE_DIR).join(format!("{SERVICE_NAME}.service"));
    if service_path.exists() {
        fs::remove_file(&service_path)
            .with_context(|| format!("failed to remove {}", service_path.display()))?;
        systemctl(&["daemon-reload"])?;
        info("Service file removed");
    } else {
        info("Service file not found");
    }

    Ok(())
}

fn remove_binary() -> Result<()> {
    info("Removing binary...");
    let binary = Path::new(INSTALL_DIR).join(BINARY_NAME);
    if binary.exists() {
        fs::remove_file(&binary)
            .with_context(|| format!("failed to remove {}", binary.display()))?;
        info("Binary removed");
    } else {
        info("Binary not found");
    }

    Ok(())
}

fn release_binary_path() -> PathBuf {
    Path::new("target").join("release").join(BINARY_NAME)
}

fn systemctl(args: &[&str]) -> Result<()> {
    ensure_systemd_booted()?;
    run_command(Command::new("systemctl").args(args))
}

fn systemctl_quiet(args: &[&str]) -> bool {
    Command::new("systemctl")
        .args(args)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn run_command(command: &mut Command) -> Result<()> {
    let description = format!("{command:?}");
    let status = command
        .status()
        .with_context(|| format!("failed to run {description}"))?;

    if !status.success() {
        bail!("{description} exited with {status}");
    }

    Ok(())
}

fn ensure_systemd_booted() -> Result<()> {
    if systemd_booted_raw() {
        Ok(())
    } else {
        bail!("systemd does not appear to be the active init system")
    }
}

fn systemd_booted_raw() -> bool {
    unsafe { libsystemd_sys::daemon::sd_booted() > 0 }
}

fn command_exists(program: &str) -> bool {
    let Some(paths) = env::var_os("PATH") else {
        return false;
    };

    env::split_paths(&paths).any(|path| path.join(program).is_file())
}

fn info(message: impl AsRef<str>) {
    let _ = writeln!(io::stdout(), "[INFO] {}", message.as_ref());
}

fn warn(message: impl AsRef<str>) {
    let _ = writeln!(io::stderr(), "[WARN] {}", message.as_ref());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_subcommand_accepts_driver_arguments() {
        let cli = Cli::parse_from([BINARY_NAME, "run", "g634jyr"]);

        assert!(matches!(cli.command, Some(CliCommand::Run(_))));
    }

    #[test]
    fn legacy_driver_arguments_are_rewritten_to_run() {
        let args = args_with_legacy_run_subcommand(vec![
            OsString::from(BINARY_NAME),
            OsString::from("g634jyr"),
        ]);
        let cli = Cli::parse_from(args);

        match cli.command {
            Some(CliCommand::Run(args)) => {
                assert_eq!(args.model, "g634jyr");
            }
            other => panic!("expected run command, got {other:?}"),
        }
    }

    #[test]
    fn recognizes_all_replacement_subcommands() {
        for command in [
            "run",
            "build",
            "build-debug",
            "run-debug",
            "deps",
            "check-cargo",
            "install-binary",
            "setup-i2c",
            "install-service",
            "start",
            "stop",
            "restart",
            "status",
            "logs",
            "install",
            "uninstall",
            "remove-service",
            "remove-binary",
            "clean",
        ] {
            assert!(is_known_subcommand(command), "{command}");
        }
    }
}
