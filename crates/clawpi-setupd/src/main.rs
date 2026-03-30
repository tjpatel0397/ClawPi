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
    println!("Phase 3 proving-ground setup helper.");
    println!();
    println!("Available flags:");
    println!("  --once    seed or validate /etc/clawpi/config.toml once");
}

fn run_once() -> ExitCode {
    let layout = Layout::detect();

    match write_setup_state(&layout) {
        Ok(state) => {
            println!("setup_state_path={}", layout.setup_state_path().display());
            println!("config_path={}", layout.config_path().display());
            println!("config_created={}", state.config_created);
            println!("config_status={}", state.config_status.label());
            println!("mode={}", state.mode);

            if let Some(config) = state.config_status.as_config() {
                println!("device_name={}", config.device_name);
                println!("setup_state={}", config.setup_state);
                println!("runtime_profile={}", config.runtime_profile);
            }

            if let Some(reason) = state.config_status.error() {
                println!("config_error={reason}");
            }

            match state.mode {
                Mode::Setup => {
                    println!("status=pending");
                    println!(
                        "note=setup mode remains active until config.toml is valid and complete"
                    );
                }
                Mode::Normal => {
                    println!("status=complete");
                    println!("note=config.toml is valid and setup is complete");
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
