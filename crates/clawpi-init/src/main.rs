use clawpi_core::{detect_mode, inspect_state, record_mode, Layout};
use std::env;
use std::io;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);

    match args.next().as_deref() {
        None | Some("--help") | Some("-h") => {
            print_usage();
            ExitCode::SUCCESS
        }
        Some("--version") | Some("-V") => {
            println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("mode") => print_mode(),
        Some("status") => print_status(),
        Some("activate") => activate_mode(),
        Some(arg) => {
            eprintln!("unsupported argument: {arg}");
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn print_usage() {
    println!("clawpi-init");
    println!("Phase 3 proving-ground boot and mode selection helper.");
    println!();
    println!("Available commands:");
    println!("  mode      print the selected ClawPi mode");
    println!("  status    print mode and setup contract status");
    println!("  activate  record the selected mode and start its systemd target");
}

fn print_mode() -> ExitCode {
    let layout = Layout::detect();

    match detect_mode(&layout) {
        Ok(mode) => {
            println!("{mode}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("failed to determine mode: {err}");
            ExitCode::from(1)
        }
    }
}

fn print_status() -> ExitCode {
    let layout = Layout::detect();

    match inspect_state(&layout) {
        Ok(state) => {
            println!("root={}", layout.root().display());
            println!("mode={}", state.mode);
            println!("target={}", state.mode.target_name());
            println!("config_path={}", layout.config_path().display());
            println!("config_status={}", state.config_status.label());

            if let Some(config) = state.config_status.as_config() {
                println!("device_name={}", config.device_name);
                println!("setup_state={}", config.setup_state);
                println!(
                    "setup_complete={}",
                    config.setup_state.as_str() == "complete"
                );
                println!("runtime_profile={}", config.runtime_profile);
            } else {
                println!("setup_complete=false");
            }

            if let Some(reason) = state.config_status.error() {
                println!("config_error={reason}");
            }

            println!(
                "recovery_requested={}",
                layout.recovery_requested_path().exists()
            );

            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("failed to read ClawPi state: {err}");
            ExitCode::from(1)
        }
    }
}

fn activate_mode() -> ExitCode {
    let layout = Layout::detect();
    let mode = match detect_mode(&layout) {
        Ok(mode) => mode,
        Err(err) => {
            eprintln!("failed to determine mode: {err}");
            return ExitCode::from(1);
        }
    };

    if let Err(err) = record_mode(&layout, mode) {
        eprintln!("failed to record selected mode: {err}");
        return ExitCode::from(1);
    }

    println!("selected_mode={mode}");
    println!("selected_target={}", mode.target_name());

    match start_target(mode.target_name()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("failed to start {}: {err}", mode.target_name());
            ExitCode::from(1)
        }
    }
}

fn start_target(target: &str) -> io::Result<()> {
    match Command::new("systemctl")
        .args(["--no-block", "start", target])
        .status()
    {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(io::Error::new(
            io::ErrorKind::Other,
            format!("systemctl exited with status {status}"),
        )),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}
