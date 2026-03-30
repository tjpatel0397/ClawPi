use clawpi_core::{apply_wifi_config, inspect_state, Layout};
use std::process::ExitCode;

fn main() -> ExitCode {
    let layout = Layout::detect();

    match run(&layout) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("clawpi-wifid: {err}");
            ExitCode::from(1)
        }
    }
}

fn run(layout: &Layout) -> std::io::Result<()> {
    let state = inspect_state(layout)?;

    if state.mode.as_str() != "setup" {
        println!("wifi_status_path={}", layout.wifi_status_path().display());
        println!("status=skipped");
        println!("reason=mode-is-not-setup");
        return Ok(());
    }

    apply_wifi_config(layout)?;

    println!("wifi_status_path={}", layout.wifi_status_path().display());

    if let Some(content) = clawpi_core::read_optional_file(&layout.wifi_status_path())? {
        print!("{content}");
        if !content.ends_with('\n') {
            println!();
        }
    }

    Ok(())
}
