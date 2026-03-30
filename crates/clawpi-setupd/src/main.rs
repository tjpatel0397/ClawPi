use clawpi_core::{write_setup_state, Layout, Mode};
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
        Some("--once") => run_once(),
        Some(arg) => {
            eprintln!("unsupported argument: {arg}");
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn print_usage() {
    println!("clawpi-setupd");
    println!("Phase 2 proving-ground setup helper.");
    println!();
    println!("Available flags:");
    println!("  --once    update proving-ground setup state once");
}

fn run_once() -> ExitCode {
    let layout = Layout::detect();

    match write_setup_state(&layout) {
        Ok(mode) => {
            println!("setup_state_path={}", layout.setup_state_path().display());
            println!("mode={mode}");

            match mode {
                Mode::Setup => {
                    println!("status=pending");
                    println!(
                        "note=setup target is active but the real first-boot flow is not built yet"
                    );
                }
                Mode::Normal => {
                    println!("status=complete");
                    println!("note=setup-complete marker is present");
                }
                Mode::Recovery => {
                    println!("status=recovery");
                    println!("note=recovery was requested before setupd ran");
                }
            }

            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("failed to write setup state: {err}");
            ExitCode::from(1)
        }
    }
}
