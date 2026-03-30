use clawpi_core::{
    detect_mode, mark_setup_complete, read_optional_file, set_recovery_requested, Layout,
};
use std::env;
use std::process::ExitCode;

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
        Some("status") => print_status(),
        Some("complete-setup") => toggle_setup_complete(true),
        Some("require-setup") => toggle_setup_complete(false),
        Some("request-recovery") => toggle_recovery(true),
        Some("clear-recovery") => toggle_recovery(false),
        Some(arg) => {
            eprintln!("unsupported subcommand: {arg}");
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn print_usage() {
    println!("clawpi-ctl");
    println!("Phase 2 proving-ground device control tool.");
    println!();
    println!("Available commands:");
    println!("  status             print current ClawPi mode and state");
    println!("  complete-setup     mark the proving-ground device as setup complete");
    println!("  require-setup      clear the setup-complete marker");
    println!("  request-recovery   force recovery mode on the next activation");
    println!("  clear-recovery     clear the recovery marker");
}

fn print_status() -> ExitCode {
    let layout = Layout::detect();
    let mode = detect_mode(&layout);

    println!("root={}", layout.root().display());
    println!("mode={mode}");
    println!("target={}", mode.target_name());
    println!("config_dir={}", layout.etc_dir().display());
    println!("state_dir={}", layout.state_dir().display());
    println!("run_dir={}", layout.run_dir().display());
    println!("setup_complete={}", layout.setup_complete_path().exists());
    println!(
        "recovery_requested={}",
        layout.recovery_requested_path().exists()
    );

    match read_optional_file(&layout.last_mode_path()) {
        Ok(Some(value)) => println!("last_mode={value}"),
        Ok(None) => println!("last_mode=unknown"),
        Err(err) => {
            eprintln!("failed to read last mode: {err}");
            return ExitCode::from(1);
        }
    }

    match read_optional_file(&layout.active_mode_path()) {
        Ok(Some(value)) => println!("active_mode={value}"),
        Ok(None) => println!("active_mode=unknown"),
        Err(err) => {
            eprintln!("failed to read active mode: {err}");
            return ExitCode::from(1);
        }
    }

    ExitCode::SUCCESS
}

fn toggle_setup_complete(complete: bool) -> ExitCode {
    let layout = Layout::detect();

    match mark_setup_complete(&layout, complete) {
        Ok(()) => {
            println!("setup_complete={}", if complete { "true" } else { "false" });
            println!("mode={}", detect_mode(&layout));
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("failed to update setup marker: {err}");
            ExitCode::from(1)
        }
    }
}

fn toggle_recovery(requested: bool) -> ExitCode {
    let layout = Layout::detect();

    match set_recovery_requested(&layout, requested) {
        Ok(()) => {
            println!(
                "recovery_requested={}",
                if requested { "true" } else { "false" }
            );
            println!("mode={}", detect_mode(&layout));
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("failed to update recovery marker: {err}");
            ExitCode::from(1)
        }
    }
}
