use clawpi_core::{inspect_state, Layout, Mode};
use std::io;
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

fn main() -> ExitCode {
    let layout = Layout::detect();

    match run(&layout) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("clawpi-sessiond: {err}");
            ExitCode::from(1)
        }
    }
}

fn run(layout: &Layout) -> io::Result<()> {
    layout.ensure_dirs()?;
    let pid = std::process::id();

    println!("clawpi-sessiond: starting pid={pid}");

    loop {
        let state = inspect_state(layout)?;

        match state.mode {
            Mode::Normal => {
                let config = state.config_status.as_config().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "normal mode requires a valid config",
                    )
                })?;

                write_status(
                    layout,
                    pid,
                    "running",
                    state.mode,
                    &config.device_name,
                    None,
                )?;
            }
            mode => {
                let device_name = state
                    .config_status
                    .as_config()
                    .map(|config| config.device_name.as_str())
                    .unwrap_or("unknown");
                let reason = format!("mode changed to {}", mode.as_str());

                write_status(layout, pid, "stopped", mode, device_name, Some(&reason))?;
                println!("clawpi-sessiond: exiting because {reason}");
                return Ok(());
            }
        }

        thread::sleep(HEARTBEAT_INTERVAL);
    }
}

fn write_status(
    layout: &Layout,
    pid: u32,
    status: &str,
    mode: Mode,
    device_name: &str,
    reason: Option<&str>,
) -> io::Result<()> {
    let heartbeat_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?
        .as_secs();

    let mut content = format!(
        "phase=4\nservice=clawpi-sessiond\nstatus={status}\npid={pid}\nmode={}\ndevice_name={device_name}\nheartbeat_unix={heartbeat_unix}\n",
        mode.as_str()
    );

    if let Some(reason) = reason {
        content.push_str(&format!("reason={reason}\n"));
    }

    std::fs::write(layout.session_status_path(), content)
}
