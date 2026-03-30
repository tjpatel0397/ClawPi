use clawpi_core::{
    clear_wifi_credentials, detect_mode, inspect_state, mark_setup_complete, read_optional_file,
    set_device_name, set_recovery_requested, set_wifi_credentials, Layout,
};
use std::env;
use std::process::ExitCode;

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
        Some("status") => print_status(),
        Some("set-device-name") => match args.next() {
            Some(device_name) => update_device_name(&device_name),
            None => {
                eprintln!("set-device-name requires a value");
                ExitCode::from(2)
            }
        },
        Some("set-wifi") => {
            let ssid = match args.next() {
                Some(value) => value,
                None => {
                    eprintln!("set-wifi requires SSID and passphrase");
                    return ExitCode::from(2);
                }
            };
            let passphrase = match args.next() {
                Some(value) => value,
                None => {
                    eprintln!("set-wifi requires SSID and passphrase");
                    return ExitCode::from(2);
                }
            };
            let country = args.next();
            update_wifi(&ssid, &passphrase, country.as_deref())
        }
        Some("clear-wifi") => clear_wifi(),
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
    println!("Phase 3 proving-ground device control tool.");
    println!();
    println!("Available commands:");
    println!("  status                    print current ClawPi mode and config state");
    println!("  set-device-name NAME      update the device name in /etc/clawpi/config.toml");
    println!("  set-wifi SSID PASS [CC]   store setup-mode Wi-Fi credentials");
    println!("  clear-wifi                remove stored Wi-Fi credentials");
    println!("  complete-setup            mark the setup contract as complete");
    println!("  require-setup             mark the setup contract as pending");
    println!("  request-recovery          force recovery mode on the next activation");
    println!("  clear-recovery            clear the recovery marker");
}

fn print_status() -> ExitCode {
    let layout = Layout::detect();

    match inspect_state(&layout) {
        Ok(state) => {
            println!("root={}", layout.root().display());
            println!("mode={}", state.mode);
            println!("target={}", state.mode.target_name());
            println!("config_dir={}", layout.etc_dir().display());
            println!("config_path={}", layout.config_path().display());
            println!("config_status={}", state.config_status.label());
            println!("state_dir={}", layout.state_dir().display());
            println!("run_dir={}", layout.run_dir().display());
            println!(
                "session_status_path={}",
                layout.session_status_path().display()
            );
            println!(
                "recovery_status_path={}",
                layout.recovery_status_path().display()
            );
            println!("wifi_status_path={}", layout.wifi_status_path().display());

            if let Some(config) = state.config_status.as_config() {
                println!("device_name={}", config.device_name);
                println!("setup_state={}", config.setup_state);
                println!(
                    "setup_complete={}",
                    config.setup_state.as_str() == "complete"
                );
                println!("runtime_profile={}", config.runtime_profile);
                println!("wifi_country={}", config.wifi_country);
                println!("wifi_configured={}", config.wifi_ssid.is_some());
                if let Some(ssid) = &config.wifi_ssid {
                    println!("wifi_ssid={ssid}");
                }
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

            match read_optional_file(&layout.session_status_path()) {
                Ok(Some(content)) => {
                    println!(
                        "session_status={}",
                        lookup_field(&content, "status").unwrap_or("unknown")
                    );
                    println!(
                        "session_mode={}",
                        lookup_field(&content, "mode").unwrap_or("unknown")
                    );
                    println!(
                        "session_heartbeat_unix={}",
                        lookup_field(&content, "heartbeat_unix").unwrap_or("unknown")
                    );
                }
                Ok(None) => println!("session_status=absent"),
                Err(err) => {
                    eprintln!("failed to read session status: {err}");
                    return ExitCode::from(1);
                }
            }

            match read_optional_file(&layout.recovery_status_path()) {
                Ok(Some(content)) => {
                    println!(
                        "recovery_status={}",
                        lookup_field(&content, "status").unwrap_or("unknown")
                    );
                    println!(
                        "recovery_mode={}",
                        lookup_field(&content, "mode").unwrap_or("unknown")
                    );
                }
                Ok(None) => println!("recovery_status=absent"),
                Err(err) => {
                    eprintln!("failed to read recovery status: {err}");
                    return ExitCode::from(1);
                }
            }

            match read_optional_file(&layout.wifi_status_path()) {
                Ok(Some(content)) => {
                    println!(
                        "wifi_status={}",
                        lookup_field(&content, "status").unwrap_or("unknown")
                    );
                    println!(
                        "wifi_reload={}",
                        lookup_field(&content, "reload").unwrap_or("unknown")
                    );
                }
                Ok(None) => println!("wifi_status=absent"),
                Err(err) => {
                    eprintln!("failed to read wifi status: {err}");
                    return ExitCode::from(1);
                }
            }

            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("failed to read ClawPi state: {err}");
            ExitCode::from(1)
        }
    }
}

fn update_device_name(device_name: &str) -> ExitCode {
    let layout = Layout::detect();

    match set_device_name(&layout, device_name) {
        Ok(config) => {
            println!("device_name={}", config.device_name);
            println!("setup_state={}", config.setup_state);
            println!("mode={}", mode_or_error(&layout));
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("failed to update config: {err}");
            ExitCode::from(1)
        }
    }
}

fn update_wifi(ssid: &str, passphrase: &str, country: Option<&str>) -> ExitCode {
    let layout = Layout::detect();

    match set_wifi_credentials(&layout, ssid, passphrase, country) {
        Ok(config) => {
            println!("wifi_ssid={}", config.wifi_ssid.as_deref().unwrap_or(""));
            println!("wifi_country={}", config.wifi_country);
            println!("mode={}", mode_or_error(&layout));
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("failed to update wifi config: {err}");
            ExitCode::from(1)
        }
    }
}

fn clear_wifi() -> ExitCode {
    let layout = Layout::detect();

    match clear_wifi_credentials(&layout) {
        Ok(config) => {
            println!("wifi_configured={}", config.wifi_ssid.is_some());
            println!("wifi_country={}", config.wifi_country);
            println!("mode={}", mode_or_error(&layout));
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("failed to clear wifi config: {err}");
            ExitCode::from(1)
        }
    }
}

fn toggle_setup_complete(complete: bool) -> ExitCode {
    let layout = Layout::detect();

    match mark_setup_complete(&layout, complete) {
        Ok(()) => {
            println!("setup_complete={}", if complete { "true" } else { "false" });
            println!("mode={}", mode_or_error(&layout));
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("failed to update setup state: {err}");
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
            println!("mode={}", mode_or_error(&layout));
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("failed to update recovery marker: {err}");
            ExitCode::from(1)
        }
    }
}

fn mode_or_error(layout: &Layout) -> String {
    match detect_mode(layout) {
        Ok(mode) => mode.to_string(),
        Err(err) => format!("error:{err}"),
    }
}

fn lookup_field<'a>(content: &'a str, key: &str) -> Option<&'a str> {
    content.lines().find_map(|line| {
        let (current_key, value) = line.split_once('=')?;
        if current_key == key {
            Some(value)
        } else {
            None
        }
    })
}
