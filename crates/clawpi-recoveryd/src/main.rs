use clawpi_core::{prepare_setup_fallback, Layout};
use std::io;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let layout = Layout::detect();

    match run(&layout) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("clawpi-recoveryd: {err}");
            ExitCode::from(1)
        }
    }
}

fn run(layout: &Layout) -> io::Result<()> {
    let state = prepare_setup_fallback(layout)?;

    println!(
        "recovery_status_path={}",
        layout.recovery_status_path().display()
    );
    println!("mode={}", state.mode);
    println!("config_path={}", layout.config_path().display());
    println!("config_status={}", state.config_status.label());

    if let Some(config) = state.config_status.as_config() {
        println!("device_name={}", config.device_name);
        println!("setup_state={}", config.setup_state);
    }

    if let Some(reason) = state.config_status.error() {
        println!("config_error={reason}");
    }

    start_target("clawpi-setup.target")?;

    Ok(())
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
