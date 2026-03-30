use clawpi_core::{detect_mode, record_mode, Layout};
use std::env;
use std::io;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    match env::args().nth(1).as_deref() {
        None | Some("--help") | Some("-h") => {
            print_usage();
            ExitCode::SUCCESS
        }
        Some("--version") | Some("-V") => {
            println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("mode") => {
            let layout = Layout::detect();
            println!("{}", detect_mode(&layout));
            ExitCode::SUCCESS
        }
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
    println!("Phase 2 proving-ground boot and mode selection helper.");
    println!();
    println!("Available commands:");
    println!("  mode      print the selected ClawPi mode");
    println!("  status    print mode and proving-ground state paths");
    println!("  activate  record the selected mode and start its systemd target");
}

fn print_status() -> ExitCode {
    let layout = Layout::detect();
    let mode = detect_mode(&layout);

    println!("root={}", layout.root().display());
    println!("mode={mode}");
    println!("target={}", mode.target_name());
    println!("setup_complete={}", layout.setup_complete_path().exists());
    println!(
        "recovery_requested={}",
        layout.recovery_requested_path().exists()
    );

    ExitCode::SUCCESS
}

fn activate_mode() -> ExitCode {
    let layout = Layout::detect();
    let mode = detect_mode(&layout);

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
