use std::env;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const CONFIG_VERSION: u32 = 1;
pub const RUNTIME_PROFILE: &str = "proving-ground";
pub const DEFAULT_WIFI_COUNTRY: &str = "US";

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Mode {
    Setup,
    Normal,
    Recovery,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::Normal => "normal",
            Self::Recovery => "recovery",
        }
    }

    pub fn target_name(self) -> &'static str {
        match self {
            Self::Setup => "clawpi-setup.target",
            Self::Normal => "clawpi.target",
            Self::Recovery => "clawpi-recovery.target",
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SetupState {
    Pending,
    Complete,
}

impl SetupState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Complete => "complete",
        }
    }

    fn from_str(value: &str) -> Result<Self, String> {
        match value {
            "pending" => Ok(Self::Pending),
            "complete" => Ok(Self::Complete),
            _ => Err(format!("unsupported setup_state: {value}")),
        }
    }
}

impl fmt::Display for SetupState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClawPiConfig {
    pub config_version: u32,
    pub device_name: String,
    pub setup_state: SetupState,
    pub runtime_profile: String,
    pub wifi_country: String,
    pub wifi_ssid: Option<String>,
    pub wifi_passphrase: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigStatus {
    Missing,
    Invalid(String),
    Valid(ClawPiConfig),
}

impl ConfigStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Invalid(_) => "invalid",
            Self::Valid(_) => "valid",
        }
    }

    pub fn as_config(&self) -> Option<&ClawPiConfig> {
        match self {
            Self::Valid(config) => Some(config),
            Self::Missing | Self::Invalid(_) => None,
        }
    }

    pub fn error(&self) -> Option<&str> {
        match self {
            Self::Invalid(reason) => Some(reason.as_str()),
            Self::Missing | Self::Valid(_) => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemState {
    pub mode: Mode,
    pub config_status: ConfigStatus,
    pub config_created: bool,
}

#[derive(Clone, Debug)]
pub struct Layout {
    root: PathBuf,
}

impl Layout {
    pub fn detect() -> Self {
        let root = env::var_os("CLAWPI_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));
        Self { root }
    }

    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn etc_dir(&self) -> PathBuf {
        self.root.join("etc").join("clawpi")
    }

    pub fn config_path(&self) -> PathBuf {
        self.etc_dir().join("config.toml")
    }

    pub fn hostname_path(&self) -> PathBuf {
        self.root.join("etc").join("hostname")
    }

    pub fn state_dir(&self) -> PathBuf {
        self.root.join("var").join("lib").join("clawpi")
    }

    pub fn run_dir(&self) -> PathBuf {
        self.root.join("run").join("clawpi")
    }

    pub fn session_status_path(&self) -> PathBuf {
        self.run_dir().join("sessiond.status")
    }

    pub fn recovery_status_path(&self) -> PathBuf {
        self.run_dir().join("recovery.status")
    }

    pub fn wifi_status_path(&self) -> PathBuf {
        self.run_dir().join("wifi.status")
    }

    pub fn wpa_supplicant_run_dir(&self) -> PathBuf {
        self.root.join("run").join("wpa_supplicant")
    }

    pub fn wpa_supplicant_control_path(&self) -> PathBuf {
        self.wpa_supplicant_run_dir().join("wlan0")
    }

    pub fn wpa_supplicant_dir(&self) -> PathBuf {
        self.root.join("etc").join("wpa_supplicant")
    }

    pub fn wpa_supplicant_config_path(&self) -> PathBuf {
        self.wpa_supplicant_dir().join("wpa_supplicant-wlan0.conf")
    }

    pub fn legacy_setup_complete_path(&self) -> PathBuf {
        self.state_dir().join("setup-complete")
    }

    pub fn recovery_requested_path(&self) -> PathBuf {
        self.state_dir().join("recovery-requested")
    }

    pub fn setup_state_path(&self) -> PathBuf {
        self.state_dir().join("setup-state")
    }

    pub fn active_mode_path(&self) -> PathBuf {
        self.run_dir().join("active-mode")
    }

    pub fn last_mode_path(&self) -> PathBuf {
        self.state_dir().join("last-mode")
    }

    pub fn ensure_dirs(&self) -> io::Result<()> {
        fs::create_dir_all(self.etc_dir())?;
        fs::create_dir_all(self.state_dir())?;
        fs::create_dir_all(self.run_dir())?;
        Ok(())
    }
}

pub fn detect_mode(layout: &Layout) -> io::Result<Mode> {
    Ok(inspect_state(layout)?.mode)
}

pub fn inspect_state(layout: &Layout) -> io::Result<SystemState> {
    inspect_state_inner(layout, false)
}

pub fn reconcile_state(layout: &Layout) -> io::Result<SystemState> {
    inspect_state_inner(layout, true)
}

pub fn mark_setup_complete(layout: &Layout, complete: bool) -> io::Result<()> {
    let setup_state = if complete {
        SetupState::Complete
    } else {
        SetupState::Pending
    };
    set_setup_state(layout, setup_state).map(|_| ())
}

pub fn set_setup_state(layout: &Layout, setup_state: SetupState) -> io::Result<ClawPiConfig> {
    layout.ensure_dirs()?;

    let mut config = config_for_update(layout)?;
    config.setup_state = setup_state;
    write_config(layout, &config)?;

    Ok(config)
}

pub fn set_device_name(layout: &Layout, device_name: &str) -> io::Result<ClawPiConfig> {
    layout.ensure_dirs()?;

    let trimmed = device_name.trim();
    if trimmed.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "device name must not be empty",
        ));
    }

    let mut config = config_for_update(layout)?;
    config.device_name = trimmed.to_string();
    validate_config(&config).map_err(invalid_data)?;
    write_config(layout, &config)?;

    Ok(config)
}

pub fn set_wifi_credentials(
    layout: &Layout,
    ssid: &str,
    passphrase: &str,
    country: Option<&str>,
) -> io::Result<ClawPiConfig> {
    layout.ensure_dirs()?;

    let ssid = ssid.trim();
    let passphrase = passphrase.trim();

    if ssid.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "wifi ssid must not be empty",
        ));
    }

    if passphrase.len() < 8 || passphrase.len() > 63 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "wifi passphrase must be 8 to 63 characters",
        ));
    }

    let mut config = config_for_update(layout)?;
    config.wifi_ssid = Some(ssid.to_string());
    config.wifi_passphrase = Some(passphrase.to_string());
    config.wifi_country = normalize_wifi_country(country.unwrap_or(DEFAULT_WIFI_COUNTRY))
        .map_err(|reason| io::Error::new(io::ErrorKind::InvalidInput, reason))?;
    validate_config(&config).map_err(invalid_data)?;
    write_config(layout, &config)?;

    Ok(config)
}

pub fn clear_wifi_credentials(layout: &Layout) -> io::Result<ClawPiConfig> {
    layout.ensure_dirs()?;

    let mut config = config_for_update(layout)?;
    config.wifi_ssid = None;
    config.wifi_passphrase = None;
    validate_config(&config).map_err(invalid_data)?;
    write_config(layout, &config)?;

    Ok(config)
}

pub fn set_recovery_requested(layout: &Layout, requested: bool) -> io::Result<()> {
    layout.ensure_dirs()?;

    if requested {
        fs::write(
            layout.recovery_requested_path(),
            b"phase=3\nstatus=requested\n",
        )?;
    } else {
        remove_if_exists(&layout.recovery_requested_path())?;
    }

    Ok(())
}

pub fn prepare_setup_fallback(layout: &Layout) -> io::Result<SystemState> {
    layout.ensure_dirs()?;
    remove_if_exists(&layout.recovery_requested_path())?;

    let state = match read_config_status(layout)? {
        ConfigStatus::Missing => {
            let config = default_config(layout)?;
            let pending_config = ClawPiConfig {
                setup_state: SetupState::Pending,
                ..config
            };
            write_config(layout, &pending_config)?;
            SystemState {
                mode: Mode::Setup,
                config_status: ConfigStatus::Valid(pending_config),
                config_created: true,
            }
        }
        ConfigStatus::Valid(config) => {
            let pending_config = ClawPiConfig {
                setup_state: SetupState::Pending,
                ..config
            };
            write_config(layout, &pending_config)?;
            SystemState {
                mode: Mode::Setup,
                config_status: ConfigStatus::Valid(pending_config),
                config_created: false,
            }
        }
        ConfigStatus::Invalid(reason) => SystemState {
            mode: Mode::Setup,
            config_status: ConfigStatus::Invalid(reason),
            config_created: false,
        },
    };

    record_mode(layout, Mode::Setup)?;

    let mut content = format!(
        "phase=5\nmode={}\nconfig_path={}\nconfig_status={}\n",
        state.mode.as_str(),
        layout.config_path().display(),
        state.config_status.label()
    );

    if let Some(config) = state.config_status.as_config() {
        content.push_str(&format!("device_name={}\n", config.device_name));
        content.push_str(&format!("setup_state={}\n", config.setup_state.as_str()));
    }

    if let Some(reason) = state.config_status.error() {
        content.push_str(&format!("config_error={reason}\n"));
    }

    content.push_str("status=redirected-to-setup\n");
    fs::write(layout.recovery_status_path(), content)?;

    Ok(state)
}

pub fn apply_wifi_config(layout: &Layout) -> io::Result<()> {
    layout.ensure_dirs()?;

    let config_status = read_config_status(layout)?;
    let status_content = match config_status {
        ConfigStatus::Missing => format!(
            "phase=5\nstatus=missing-config\nconfig_path={}\n",
            layout.config_path().display()
        ),
        ConfigStatus::Invalid(reason) => format!(
            "phase=5\nstatus=invalid-config\nconfig_path={}\nconfig_error={reason}\n",
            layout.config_path().display()
        ),
        ConfigStatus::Valid(config) => {
            let (ssid, passphrase) = match (&config.wifi_ssid, &config.wifi_passphrase) {
                (Some(ssid), Some(passphrase)) => (ssid, passphrase),
                _ => {
                    let content = format!(
                        "phase=5\nstatus=not-configured\nwifi_country={}\n",
                        config.wifi_country
                    );
                    fs::write(layout.wifi_status_path(), content)?;
                    return Ok(());
                }
            };

            fs::create_dir_all(layout.wpa_supplicant_dir())?;
            let file_content = format!(
                "ctrl_interface=DIR=/run/wpa_supplicant GROUP=netdev\nupdate_config=1\ncountry={}\n\nnetwork={{\n    ssid={}\n    psk={}\n    key_mgmt=WPA-PSK\n}}\n",
                config.wifi_country,
                format_string(ssid),
                format_string(passphrase),
            );
            fs::write(layout.wpa_supplicant_config_path(), file_content)?;

            let reload = try_reload_wifi(layout)?;
            format!(
                "phase=5\nstatus={}\nwifi_ssid={}\nwifi_country={}\nwpa_supplicant_path={}\nreload={}\n",
                reload.status,
                ssid,
                config.wifi_country,
                layout.wpa_supplicant_config_path().display(),
                reload.command,
            )
        }
    };

    fs::write(layout.wifi_status_path(), status_content)?;
    Ok(())
}

pub fn record_mode(layout: &Layout, mode: Mode) -> io::Result<()> {
    layout.ensure_dirs()?;
    fs::write(layout.active_mode_path(), format!("{}\n", mode.as_str()))?;
    fs::write(layout.last_mode_path(), format!("{}\n", mode.as_str()))?;
    Ok(())
}

pub fn write_setup_state(layout: &Layout) -> io::Result<SystemState> {
    let state = reconcile_state(layout)?;

    let mut content = format!(
        "phase=3\nmode={}\nconfig_path={}\nconfig_created={}\nconfig_status={}\n",
        state.mode.as_str(),
        layout.config_path().display(),
        state.config_created,
        state.config_status.label()
    );

    if let Some(config) = state.config_status.as_config() {
        content.push_str(&format!("device_name={}\n", config.device_name));
        content.push_str(&format!("setup_state={}\n", config.setup_state.as_str()));
        content.push_str(&format!("runtime_profile={}\n", config.runtime_profile));
        content.push_str(&format!("wifi_country={}\n", config.wifi_country));
        content.push_str(&format!("wifi_configured={}\n", config.wifi_ssid.is_some()));
    }

    if let Some(reason) = state.config_status.error() {
        content.push_str(&format!("config_error={reason}\n"));
    }

    let note = match state.mode {
        Mode::Recovery => "recovery mode requested",
        Mode::Normal => "config is complete and normal mode is allowed",
        Mode::Setup => match &state.config_status {
            ConfigStatus::Missing => "config is missing",
            ConfigStatus::Invalid(_) => "config is invalid",
            ConfigStatus::Valid(config) if state.config_created => {
                if config.setup_state == SetupState::Complete {
                    "config was created from legacy state and setup is complete"
                } else {
                    "config was created and setup is still pending"
                }
            }
            ConfigStatus::Valid(_) => "config is present but setup is still pending",
        },
    };
    content.push_str(&format!("note={note}\n"));

    fs::write(layout.setup_state_path(), content)?;

    Ok(state)
}

pub fn read_optional_file(path: &Path) -> io::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content.trim().to_string())),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

fn inspect_state_inner(layout: &Layout, create_if_missing: bool) -> io::Result<SystemState> {
    layout.ensure_dirs()?;

    let mut config_status = read_config_status(layout)?;
    let mut config_created = false;

    if create_if_missing && matches!(config_status, ConfigStatus::Missing) {
        let config = default_config(layout)?;
        write_config(layout, &config)?;
        config_status = ConfigStatus::Valid(config);
        config_created = true;
    }

    let mode = if layout.recovery_requested_path().exists() {
        Mode::Recovery
    } else {
        match &config_status {
            ConfigStatus::Valid(config) if config.setup_state == SetupState::Complete => {
                Mode::Normal
            }
            ConfigStatus::Missing | ConfigStatus::Invalid(_) | ConfigStatus::Valid(_) => {
                Mode::Setup
            }
        }
    };

    Ok(SystemState {
        mode,
        config_status,
        config_created,
    })
}

fn read_config_status(layout: &Layout) -> io::Result<ConfigStatus> {
    match fs::read_to_string(layout.config_path()) {
        Ok(content) => Ok(match parse_config(&content) {
            Ok(config) => ConfigStatus::Valid(config),
            Err(reason) => ConfigStatus::Invalid(reason),
        }),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(ConfigStatus::Missing),
        Err(err) => Err(err),
    }
}

fn config_for_update(layout: &Layout) -> io::Result<ClawPiConfig> {
    match read_config_status(layout)? {
        ConfigStatus::Valid(config) => Ok(config),
        ConfigStatus::Missing => default_config(layout),
        ConfigStatus::Invalid(reason) => Err(invalid_data(reason)),
    }
}

fn default_config(layout: &Layout) -> io::Result<ClawPiConfig> {
    let device_name = read_optional_file(&layout.hostname_path())?
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| String::from("clawpi"));
    let setup_state = if layout.legacy_setup_complete_path().exists() {
        SetupState::Complete
    } else {
        SetupState::Pending
    };

    let config = ClawPiConfig {
        config_version: CONFIG_VERSION,
        device_name,
        setup_state,
        runtime_profile: String::from(RUNTIME_PROFILE),
        wifi_country: String::from(DEFAULT_WIFI_COUNTRY),
        wifi_ssid: None,
        wifi_passphrase: None,
    };

    validate_config(&config).map_err(invalid_data)?;
    Ok(config)
}

fn write_config(layout: &Layout, config: &ClawPiConfig) -> io::Result<()> {
    layout.ensure_dirs()?;
    validate_config(config).map_err(invalid_data)?;

    let content = format!(
        "config_version = {}\ndevice_name = {}\nsetup_state = {}\nruntime_profile = {}\nwifi_country = {}\n{}{}",
        config.config_version,
        format_string(&config.device_name),
        format_string(config.setup_state.as_str()),
        format_string(&config.runtime_profile),
        format_string(&config.wifi_country),
        optional_config_line("wifi_ssid", config.wifi_ssid.as_deref()),
        optional_config_line("wifi_passphrase", config.wifi_passphrase.as_deref()),
    );
    fs::write(layout.config_path(), content)?;
    remove_if_exists(&layout.legacy_setup_complete_path())?;

    Ok(())
}

fn validate_config(config: &ClawPiConfig) -> Result<(), String> {
    if config.config_version != CONFIG_VERSION {
        return Err(format!(
            "unsupported config_version: {}",
            config.config_version
        ));
    }

    if config.device_name.trim().is_empty() {
        return Err(String::from("device_name must not be empty"));
    }

    if config.runtime_profile != RUNTIME_PROFILE {
        return Err(format!(
            "unsupported runtime_profile: {}",
            config.runtime_profile
        ));
    }

    normalize_wifi_country(&config.wifi_country)?;

    match (&config.wifi_ssid, &config.wifi_passphrase) {
        (Some(ssid), Some(passphrase)) => {
            if ssid.trim().is_empty() {
                return Err(String::from("wifi_ssid must not be empty"));
            }
            if passphrase.len() < 8 || passphrase.len() > 63 {
                return Err(String::from("wifi_passphrase must be 8 to 63 characters"));
            }
        }
        (None, None) => {}
        _ => {
            return Err(String::from(
                "wifi_ssid and wifi_passphrase must be set together",
            ))
        }
    }

    Ok(())
}

fn parse_config(content: &str) -> Result<ClawPiConfig, String> {
    let mut config_version = None;
    let mut device_name = None;
    let mut setup_state = None;
    let mut runtime_profile = None;
    let mut wifi_country = None;
    let mut wifi_ssid = None;
    let mut wifi_passphrase = None;

    for (index, raw_line) in content.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line.trim();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("line {line_number}: expected key = value"))?;
        let key = key.trim();
        let value = value.trim();

        match key {
            "config_version" => {
                config_version = Some(
                    value
                        .parse::<u32>()
                        .map_err(|_| format!("line {line_number}: invalid config_version"))?,
                );
            }
            "device_name" => {
                device_name = Some(parse_string(value, line_number)?);
            }
            "setup_state" => {
                let parsed = parse_string(value, line_number)?;
                setup_state = Some(SetupState::from_str(&parsed)?);
            }
            "runtime_profile" => {
                runtime_profile = Some(parse_string(value, line_number)?);
            }
            "wifi_country" => {
                wifi_country = Some(parse_string(value, line_number)?);
            }
            "wifi_ssid" => {
                wifi_ssid = Some(parse_string(value, line_number)?);
            }
            "wifi_passphrase" => {
                wifi_passphrase = Some(parse_string(value, line_number)?);
            }
            _ => return Err(format!("line {line_number}: unsupported key {key}")),
        }
    }

    let config = ClawPiConfig {
        config_version: config_version
            .ok_or_else(|| String::from("missing config_version in config.toml"))?,
        device_name: device_name
            .ok_or_else(|| String::from("missing device_name in config.toml"))?,
        setup_state: setup_state
            .ok_or_else(|| String::from("missing setup_state in config.toml"))?,
        runtime_profile: runtime_profile
            .ok_or_else(|| String::from("missing runtime_profile in config.toml"))?,
        wifi_country: wifi_country.unwrap_or_else(|| String::from(DEFAULT_WIFI_COUNTRY)),
        wifi_ssid,
        wifi_passphrase,
    };

    validate_config(&config)?;
    Ok(config)
}

fn parse_string(value: &str, line_number: usize) -> Result<String, String> {
    if !(value.starts_with('"') && value.ends_with('"') && value.len() >= 2) {
        return Err(format!("line {line_number}: expected quoted string"));
    }

    Ok(value[1..value.len() - 1]
        .replace("\\\"", "\"")
        .replace("\\\\", "\\"))
}

fn format_string(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn optional_config_line(key: &str, value: Option<&str>) -> String {
    match value {
        Some(value) => format!("{key} = {}\n", format_string(value)),
        None => String::new(),
    }
}

fn normalize_wifi_country(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.len() != 2 || !trimmed.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return Err(format!("invalid wifi_country: {value}"));
    }
    Ok(trimmed.to_ascii_uppercase())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WifiReloadOutcome {
    status: &'static str,
    command: String,
}

fn try_reload_wifi(layout: &Layout) -> io::Result<WifiReloadOutcome> {
    if layout.wpa_supplicant_control_path().exists() {
        for args in [
            ["-i", "wlan0", "reconfigure"],
            ["-i", "wlan0", "reassociate"],
        ] {
            if command_succeeds("wpa_cli", &args)? {
                return Ok(WifiReloadOutcome {
                    status: "configured",
                    command: format!("wpa_cli {}", args.join(" ")),
                });
            }
        }

        return Ok(WifiReloadOutcome {
            status: "staged",
            command: String::from("manual-reload-needed"),
        });
    }

    for unit in ["wpa_supplicant@wlan0.service", "wpa_supplicant.service"] {
        if systemd_unit_is_active(unit)?
            && command_succeeds("systemctl", &["reload-or-restart", unit])?
        {
            return Ok(WifiReloadOutcome {
                status: "configured",
                command: format!("systemctl reload-or-restart {unit}"),
            });
        }
    }

    Ok(WifiReloadOutcome {
        status: "staged",
        command: String::from("manual-reload-needed"),
    })
}

fn systemd_unit_is_active(unit: &str) -> io::Result<bool> {
    command_succeeds("systemctl", &["is-active", "--quiet", unit])
}

fn command_succeeds(program: &str, args: &[&str]) -> io::Result<bool> {
    match std::process::Command::new(program).args(args).status() {
        Ok(status) => Ok(status.success()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(_) => Ok(false),
    }
}

fn invalid_data(reason: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}

fn remove_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn inspect_state_defaults_to_setup_when_config_is_missing() {
        let root = unique_test_root();
        let layout = Layout::from_root(&root);

        let state = inspect_state(&layout).unwrap();

        assert_eq!(state.mode, Mode::Setup);
        assert_eq!(state.config_status, ConfigStatus::Missing);

        cleanup_test_root(&root);
    }

    #[test]
    fn reconcile_state_creates_pending_config() {
        let root = unique_test_root();
        let layout = Layout::from_root(&root);

        let state = reconcile_state(&layout).unwrap();

        assert_eq!(state.mode, Mode::Setup);
        assert!(state.config_created);

        let config = match state.config_status {
            ConfigStatus::Valid(config) => config,
            ConfigStatus::Missing | ConfigStatus::Invalid(_) => panic!("expected config"),
        };
        assert_eq!(config.device_name, "clawpi");
        assert_eq!(config.setup_state, SetupState::Pending);
        assert!(layout.config_path().exists());

        cleanup_test_root(&root);
    }

    #[test]
    fn legacy_setup_marker_becomes_complete_config() {
        let root = unique_test_root();
        let layout = Layout::from_root(&root);

        fs::create_dir_all(layout.state_dir()).unwrap();
        fs::write(
            layout.legacy_setup_complete_path(),
            "phase=2\nstatus=complete\n",
        )
        .unwrap();

        let state = reconcile_state(&layout).unwrap();

        assert_eq!(state.mode, Mode::Normal);
        let config = match state.config_status {
            ConfigStatus::Valid(config) => config,
            ConfigStatus::Missing | ConfigStatus::Invalid(_) => panic!("expected config"),
        };
        assert_eq!(config.setup_state, SetupState::Complete);
        assert!(!layout.legacy_setup_complete_path().exists());

        cleanup_test_root(&root);
    }

    #[test]
    fn set_device_name_updates_valid_config() {
        let root = unique_test_root();
        let layout = Layout::from_root(&root);

        set_device_name(&layout, "clawpi-cm5").unwrap();

        let state = inspect_state(&layout).unwrap();
        let config = match state.config_status {
            ConfigStatus::Valid(config) => config,
            ConfigStatus::Missing | ConfigStatus::Invalid(_) => panic!("expected config"),
        };
        assert_eq!(config.device_name, "clawpi-cm5");
        assert_eq!(config.setup_state, SetupState::Pending);
        assert_eq!(config.wifi_country, DEFAULT_WIFI_COUNTRY);

        cleanup_test_root(&root);
    }

    #[test]
    fn set_wifi_credentials_writes_optional_wifi_fields() {
        let root = unique_test_root();
        let layout = Layout::from_root(&root);

        set_wifi_credentials(&layout, "ClawNet", "verysecret", Some("us")).unwrap();

        let state = inspect_state(&layout).unwrap();
        let config = match state.config_status {
            ConfigStatus::Valid(config) => config,
            ConfigStatus::Missing | ConfigStatus::Invalid(_) => panic!("expected config"),
        };
        assert_eq!(config.wifi_country, "US");
        assert_eq!(config.wifi_ssid.as_deref(), Some("ClawNet"));
        assert_eq!(config.wifi_passphrase.as_deref(), Some("verysecret"));

        cleanup_test_root(&root);
    }

    #[test]
    fn invalid_config_forces_setup_mode() {
        let root = unique_test_root();
        let layout = Layout::from_root(&root);

        layout.ensure_dirs().unwrap();
        fs::write(
            layout.config_path(),
            "config_version = 1\nsetup_state = \"complete\"\nruntime_profile = \"proving-ground\"\n",
        )
        .unwrap();

        let state = inspect_state(&layout).unwrap();

        assert_eq!(state.mode, Mode::Setup);
        assert!(matches!(state.config_status, ConfigStatus::Invalid(_)));

        cleanup_test_root(&root);
    }

    #[test]
    fn record_mode_writes_runtime_files() {
        let root = unique_test_root();
        let layout = Layout::from_root(&root);

        record_mode(&layout, Mode::Normal).unwrap();

        assert_eq!(
            read_optional_file(&layout.active_mode_path()).unwrap(),
            Some(String::from("normal"))
        );
        assert_eq!(
            read_optional_file(&layout.last_mode_path()).unwrap(),
            Some(String::from("normal"))
        );

        cleanup_test_root(&root);
    }

    #[test]
    fn prepare_setup_fallback_clears_recovery_and_sets_pending() {
        let root = unique_test_root();
        let layout = Layout::from_root(&root);

        set_device_name(&layout, "clawpi-cm5").unwrap();
        mark_setup_complete(&layout, true).unwrap();
        set_recovery_requested(&layout, true).unwrap();

        let state = prepare_setup_fallback(&layout).unwrap();

        assert_eq!(state.mode, Mode::Setup);
        let config = match state.config_status {
            ConfigStatus::Valid(config) => config,
            ConfigStatus::Missing | ConfigStatus::Invalid(_) => panic!("expected config"),
        };
        assert_eq!(config.setup_state, SetupState::Pending);
        assert!(!layout.recovery_requested_path().exists());
        assert_eq!(
            read_optional_file(&layout.active_mode_path()).unwrap(),
            Some(String::from("setup"))
        );

        cleanup_test_root(&root);
    }

    fn unique_test_root() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = env::temp_dir().join(format!("clawpi-core-test-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn cleanup_test_root(root: &Path) {
        let _ = fs::remove_dir_all(root);
    }
}
