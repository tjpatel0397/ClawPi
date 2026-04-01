use clawpi_core::{
    apply_wifi_config, inspect_state, mark_setup_complete, read_optional_file, record_mode,
    set_device_name, set_wifi_credentials, ConfigStatus, Layout, Mode, DEFAULT_WIFI_COUNTRY,
};
use signal_hook::consts::signal::{SIGINT, SIGTERM};
use signal_hook::flag as signal_flag;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const AP_INTERFACE: &str = "wlan0";
const AP_GATEWAY: &str = "192.168.64.1";
const AP_CIDR: &str = "192.168.64.1/24";
const AP_ADDRESS_RANGE: &str = "192.168.64.50,192.168.64.150,255.255.255.0,12h";
const AP_PORTAL_URL: &str = "http://setup.clawpi/";
const AP_PORTAL_IP_URL: &str = "http://192.168.64.1/";
const AP_CAPPORT_API_PATH: &str = "/.well-known/captive-portal";
const AP_CAPPORT_API_URL: &str = "http://192.168.64.1/.well-known/captive-portal";
const AP_CHANNEL: &str = "6";
const JOIN_TIMEOUT: Duration = Duration::from_secs(30);
const RESTORE_TIMEOUT: Duration = Duration::from_secs(20);
const IFUPDOWN_WLAN_UNIT: &str = "ifup@wlan0.service";

fn main() -> ExitCode {
    let layout = Layout::detect();

    match run(&layout) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("clawpi-portald: {err}");
            ExitCode::from(1)
        }
    }
}

fn run(layout: &Layout) -> io::Result<()> {
    let state = inspect_state(layout)?;
    if state.mode != Mode::Setup {
        write_status_file(
            layout,
            &format!(
                "phase=6\nstatus=skipped\nmode={}\nreason=mode-is-not-setup\n",
                state.mode.as_str()
            ),
        )?;
        return Ok(());
    }

    let shutdown_requested = Arc::new(AtomicBool::new(false));
    signal_flag::register(SIGTERM, Arc::clone(&shutdown_requested))
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;
    signal_flag::register(SIGINT, Arc::clone(&shutdown_requested))
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;

    let listener = TcpListener::bind("0.0.0.0:80")?;
    listener.set_nonblocking(true)?;
    let setup_ssid = build_setup_ssid();
    let mut runtime = PortalRuntime {
        layout: layout.clone(),
        setup_ssid,
        portal_dir: layout.run_dir().join("portal"),
        listener,
        shutdown_requested,
        hostapd_child: None,
        dnsmasq_child: None,
        ap_active: false,
        last_error: None,
        should_exit: false,
    };

    match runtime.start_setup_network() {
        Ok(()) => runtime.serve(),
        Err(err) => {
            let reason = err.to_string();
            let _ = runtime.restore_managed_wifi();
            let _ = runtime.write_failed_status(&reason);
            Err(io::Error::new(err.kind(), reason))
        }
    }
}

struct PortalRuntime {
    layout: Layout,
    setup_ssid: String,
    portal_dir: PathBuf,
    listener: TcpListener,
    shutdown_requested: Arc<AtomicBool>,
    hostapd_child: Option<Child>,
    dnsmasq_child: Option<Child>,
    ap_active: bool,
    last_error: Option<String>,
    should_exit: bool,
}

impl PortalRuntime {
    fn serve(&mut self) -> io::Result<()> {
        while !self.should_exit && !self.shutdown_requested.load(Ordering::Relaxed) {
            match self.listener.accept() {
                Ok((stream, _)) => self.handle_connection(stream)?,
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(200));
                }
                Err(err) => return Err(err),
            }
        }

        Ok(())
    }

    fn handle_connection(&mut self, mut stream: TcpStream) -> io::Result<()> {
        let request = match read_http_request(&mut stream) {
            Ok(request) => request,
            Err(err) => {
                write_http_response(
                    &mut stream,
                    "400 Bad Request",
                    "text/plain; charset=utf-8",
                    format!("invalid request: {err}\n"),
                )?;
                return Ok(());
            }
        };

        match (request.method.as_str(), request.normalized_path().as_str()) {
            ("GET", AP_CAPPORT_API_PATH) => {
                write_http_response(
                    &mut stream,
                    "200 OK",
                    "application/captive+json",
                    format!(
                        "{{\"captive\":true,\"user-portal-url\":\"{}\"}}",
                        AP_PORTAL_IP_URL
                    ),
                )?;
            }
            ("GET", "/status") => {
                let body = read_optional_file(&self.layout.portal_status_path())?
                    .unwrap_or_else(|| String::from("status=unknown\n"));
                write_http_response(&mut stream, "200 OK", "text/plain; charset=utf-8", body)?;
            }
            ("GET", "/hotspot-detect.html")
            | ("GET", "/generate_204")
            | ("GET", "/gen_204")
            | ("GET", "/connecttest.txt")
            | ("GET", "/ncsi.txt")
            | ("GET", "/success.txt")
            | ("GET", "/library/test/success.html") => {
                write_http_redirect(&mut stream, AP_PORTAL_IP_URL)?;
            }
            ("POST", "/configure") => {
                if let Err(err) = self.apply_form(&request.body) {
                    self.last_error = Some(err.to_string());
                    let body = self.render_setup_page()?;
                    write_http_response(
                        &mut stream,
                        "422 Unprocessable Entity",
                        "text/html; charset=utf-8",
                        body,
                    )?;
                    return Ok(());
                }

                write_http_response(
                    &mut stream,
                    "200 OK",
                    "text/html; charset=utf-8",
                    self.render_transition_page(),
                )?;
                stream.flush()?;
                thread::sleep(Duration::from_millis(750));

                let desired_ssid = self.current_wifi_ssid()?.unwrap_or_default();
                match self.transition_to_home_wifi(&desired_ssid) {
                    Ok(()) => {
                        self.should_exit = true;
                    }
                    Err(err) => {
                        self.last_error = Some(err.to_string());
                        self.start_setup_network()?;
                    }
                }
            }
            _ => {
                let body = self.render_setup_page()?;
                write_http_response(&mut stream, "200 OK", "text/html; charset=utf-8", body)?;
            }
        }

        Ok(())
    }

    fn apply_form(&mut self, body: &[u8]) -> io::Result<()> {
        let fields = parse_form_urlencoded(&String::from_utf8_lossy(body));

        if let Some(device_name) = fields.get("device_name") {
            if !device_name.trim().is_empty() {
                set_device_name(&self.layout, device_name)?;
            }
        }

        let ssid = fields.get("ssid").map(String::as_str).unwrap_or("");
        let passphrase = fields.get("passphrase").map(String::as_str).unwrap_or("");
        let country = fields
            .get("country")
            .map(|value| value.trim())
            .filter(|value| !value.is_empty());

        set_wifi_credentials(&self.layout, ssid, passphrase, country)?;
        self.last_error = None;

        Ok(())
    }

    fn transition_to_home_wifi(&mut self, expected_ssid: &str) -> io::Result<()> {
        self.write_transition_status(expected_ssid)?;
        self.activate_managed_wifi()?;

        if !wait_for_wifi_connection(expected_ssid)? {
            self.write_failed_status(&format!(
                "timed out joining home wifi for ssid {}",
                expected_ssid
            ))?;
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("timed out joining home wifi for ssid {expected_ssid}"),
            ));
        }

        mark_setup_complete(&self.layout, true)?;
        record_mode(&self.layout, Mode::Normal)?;
        self.write_connected_status(expected_ssid)?;
        start_unit(Mode::Normal.target_name())?;

        Ok(())
    }

    fn start_setup_network(&mut self) -> io::Result<()> {
        self.stop_setup_network()?;
        fs::create_dir_all(&self.portal_dir)?;
        self.write_starting_status()?;

        self.write_hostapd_config()?;
        self.write_dnsmasq_config()?;

        let country = self.current_wifi_country()?;
        let _ = command_succeeds("rfkill", &["unblock", "wlan"]);
        let _ = stop_unit_if_loaded(IFUPDOWN_WLAN_UNIT);
        let _ = command_succeeds("ifdown", &["--force", AP_INTERFACE]);
        let _ = command_succeeds("dhclient", &["-r", AP_INTERFACE]);
        stop_unit("wpa_supplicant@wlan0.service");
        stop_unit("wpa_supplicant.service");

        run_command("ip", &["link", "set", AP_INTERFACE, "down"])?;
        let _ = command_succeeds("ip", &["addr", "flush", "dev", AP_INTERFACE]);
        run_command("ip", &["link", "set", AP_INTERFACE, "up"])?;
        run_command("ip", &["addr", "add", AP_CIDR, "dev", AP_INTERFACE])?;

        let mut hostapd = Command::new("hostapd")
            .arg(self.hostapd_config_path())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;
        confirm_child_started(&mut hostapd, "hostapd")?;

        let mut dnsmasq = Command::new("dnsmasq")
            .arg("--keep-in-foreground")
            .arg(format!(
                "--conf-file={}",
                self.dnsmasq_config_path().display()
            ))
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;
        confirm_child_started(&mut dnsmasq, "dnsmasq")?;

        self.hostapd_child = Some(hostapd);
        self.dnsmasq_child = Some(dnsmasq);
        self.ap_active = true;
        self.write_setup_network_status(&country)?;

        Ok(())
    }

    fn restore_managed_wifi(&mut self) -> io::Result<()> {
        if let Err(err) = self.activate_managed_wifi() {
            self.last_error = Some(format!("portal rollback failed: {err}"));
            return Err(err);
        }

        if wait_for_ipv4_address(AP_INTERFACE, RESTORE_TIMEOUT)? {
            self.write_restored_status()?;
        } else {
            self.write_restore_failed_status("timed out waiting for wlan0 IPv4 after portal stop")?;
        }

        Ok(())
    }

    fn activate_managed_wifi(&mut self) -> io::Result<()> {
        let _ = self.stop_setup_network();
        let _ = command_succeeds("ip", &["addr", "flush", "dev", AP_INTERFACE]);
        let _ = run_command("ip", &["link", "set", AP_INTERFACE, "up"]);
        self.remove_stale_wpa_state()?;

        let uses_ifupdown = unit_is_loaded(IFUPDOWN_WLAN_UNIT)?;

        if uses_ifupdown {
            let _ = start_unit_if_loaded(IFUPDOWN_WLAN_UNIT);
        } else {
            for unit in ["wpa_supplicant@wlan0.service", "wpa_supplicant.service"] {
                let _ = start_unit_if_loaded(unit);
            }
        }

        apply_wifi_config(&self.layout)?;

        if uses_ifupdown {
            // Avoid racing the systemd-managed ifup invocation on DietPi.
        } else {
            let _ = command_succeeds("wpa_cli", &["-i", AP_INTERFACE, "reconnect"]);

            for unit in [
                "dhcpcd.service",
                "systemd-networkd.service",
                "NetworkManager.service",
            ] {
                let _ = start_unit_if_loaded(unit);
            }
        }

        if !has_ipv4_address(AP_INTERFACE)? {
            if uses_ifupdown {
                let _ = command_succeeds("ifup", &["--force", AP_INTERFACE]);
            } else {
                for (program, args) in [
                    ("dhcpcd", vec!["-n", AP_INTERFACE]),
                    ("dhclient", vec!["-1", AP_INTERFACE]),
                    ("udhcpc", vec!["-n", "-q", "-i", AP_INTERFACE]),
                ] {
                    if command_succeeds(program, &args) {
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    fn remove_stale_wpa_state(&self) -> io::Result<()> {
        remove_file_if_exists(&self.layout.wpa_supplicant_control_path())?;
        remove_file_if_exists(
            self.layout
                .root()
                .join("run")
                .join("wpa_supplicant.wlan0.pid")
                .as_path(),
        )?;
        Ok(())
    }

    fn stop_setup_network(&mut self) -> io::Result<()> {
        if let Some(mut child) = self.hostapd_child.take() {
            stop_child(&mut child);
        }
        if let Some(mut child) = self.dnsmasq_child.take() {
            stop_child(&mut child);
        }
        if self.ap_active {
            let _ = command_succeeds("ip", &["addr", "flush", "dev", AP_INTERFACE]);
            let _ = command_succeeds("ip", &["link", "set", AP_INTERFACE, "down"]);
        }
        self.ap_active = false;
        Ok(())
    }

    fn current_form_defaults(&self) -> io::Result<FormDefaults> {
        let state = inspect_state(&self.layout)?;
        let defaults = match state.config_status {
            ConfigStatus::Valid(config) => FormDefaults {
                device_name: config.device_name,
                wifi_ssid: config.wifi_ssid.unwrap_or_default(),
                wifi_country: config.wifi_country,
            },
            ConfigStatus::Missing | ConfigStatus::Invalid(_) => FormDefaults {
                device_name: String::from("clawpi"),
                wifi_ssid: String::new(),
                wifi_country: String::from(DEFAULT_WIFI_COUNTRY),
            },
        };

        Ok(defaults)
    }

    fn current_wifi_country(&self) -> io::Result<String> {
        Ok(self.current_form_defaults()?.wifi_country)
    }

    fn current_wifi_ssid(&self) -> io::Result<Option<String>> {
        let state = inspect_state(&self.layout)?;
        Ok(match state.config_status {
            ConfigStatus::Valid(config) => config.wifi_ssid,
            ConfigStatus::Missing | ConfigStatus::Invalid(_) => None,
        })
    }

    fn write_setup_network_status(&self, country: &str) -> io::Result<()> {
        let mut content = format!(
            "phase=6\nstatus=setup-network-active\nmode=setup\nsetup_ssid={}\nportal_url={}\nportal_fallback_url={}\nap_address={}\nwifi_country={}\n",
            self.setup_ssid, AP_PORTAL_URL, AP_PORTAL_IP_URL, AP_GATEWAY, country
        );

        if let Some(error) = &self.last_error {
            content.push_str(&format!("last_error={}\n", sanitize_status_line(error)));
        }

        write_status_file(&self.layout, &content)
    }

    fn write_transition_status(&self, home_wifi_ssid: &str) -> io::Result<()> {
        write_status_file(
            &self.layout,
            &format!(
                "phase=6\nstatus=joining-home-wifi\nmode=setup\nsetup_ssid={}\nportal_url={}\nportal_fallback_url={}\nhome_wifi_ssid={}\n",
                self.setup_ssid, AP_PORTAL_URL, AP_PORTAL_IP_URL, home_wifi_ssid
            ),
        )
    }

    fn write_starting_status(&self) -> io::Result<()> {
        write_status_file(
            &self.layout,
            &format!(
                "phase=6\nstatus=starting-setup-network\nmode=setup\nsetup_ssid={}\nportal_url={}\nportal_fallback_url={}\n",
                self.setup_ssid, AP_PORTAL_URL, AP_PORTAL_IP_URL
            ),
        )
    }

    fn write_failed_status(&self, error: &str) -> io::Result<()> {
        write_status_file(
            &self.layout,
            &format!(
                "phase=6\nstatus=setup-network-active\nmode=setup\nsetup_ssid={}\nportal_url={}\nportal_fallback_url={}\nap_address={}\nlast_error={}\n",
                self.setup_ssid,
                AP_PORTAL_URL,
                AP_PORTAL_IP_URL,
                AP_GATEWAY,
                sanitize_status_line(error)
            ),
        )
    }

    fn write_connected_status(&self, home_wifi_ssid: &str) -> io::Result<()> {
        write_status_file(
            &self.layout,
            &format!(
                "phase=6\nstatus=connected\nmode=normal\nhome_wifi_ssid={}\ntarget={}\n",
                home_wifi_ssid,
                Mode::Normal.target_name()
            ),
        )
    }

    fn write_restored_status(&self) -> io::Result<()> {
        write_status_file(
            &self.layout,
            &format!(
                "phase=6\nstatus=restored-managed-wifi\nmode=setup\nportal_url={}\nportal_fallback_url={}\n",
                AP_PORTAL_URL, AP_PORTAL_IP_URL
            ),
        )
    }

    fn write_restore_failed_status(&self, error: &str) -> io::Result<()> {
        write_status_file(
            &self.layout,
            &format!(
                "phase=6\nstatus=restore-failed\nmode=setup\nportal_url={}\nportal_fallback_url={}\nlast_error={}\n",
                AP_PORTAL_URL,
                AP_PORTAL_IP_URL,
                sanitize_status_line(error)
            ),
        )
    }

    fn render_setup_page(&self) -> io::Result<String> {
        let defaults = self.current_form_defaults()?;
        let error_html = self
            .last_error
            .as_deref()
            .map(|error| {
                format!(
                    "<p class=\"notice notice-error\">{}</p>",
                    escape_html(error)
                )
            })
            .unwrap_or_default();

        Ok(format!(
            "<!doctype html>\
<html lang=\"en\">\
<head>\
  <meta charset=\"utf-8\">\
  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
  <title>ClawPi Setup</title>\
  <style>\
    :root {{ color-scheme: light; font-family: ui-sans-serif, system-ui, sans-serif; }}\
    body {{ margin: 0; background: linear-gradient(160deg, #f3efe4, #d9efe3); color: #14211c; }}\
    main {{ max-width: 34rem; margin: 0 auto; padding: 2rem 1.25rem 3rem; }}\
    h1 {{ margin-bottom: 0.4rem; font-size: 2rem; }}\
    p {{ line-height: 1.5; }}\
    form {{ margin-top: 1.5rem; padding: 1.25rem; background: rgba(255,255,255,0.82); border-radius: 1rem; box-shadow: 0 0.5rem 2rem rgba(20,33,28,0.08); }}\
    label {{ display: block; margin-top: 1rem; font-weight: 600; }}\
    input {{ width: 100%; box-sizing: border-box; margin-top: 0.35rem; padding: 0.85rem 0.9rem; border: 1px solid #b7cbbf; border-radius: 0.8rem; font: inherit; }}\
    button {{ width: 100%; margin-top: 1.25rem; padding: 0.95rem 1rem; border: 0; border-radius: 999px; background: #184f37; color: #fff; font: inherit; font-weight: 700; }}\
    .meta {{ margin-top: 1rem; padding: 0.9rem 1rem; border-radius: 0.8rem; background: rgba(20,33,28,0.07); }}\
    .notice {{ margin-top: 1rem; padding: 0.9rem 1rem; border-radius: 0.8rem; }}\
    .notice-error {{ background: #ffe3dc; color: #7a2410; }}\
  </style>\
</head>\
<body>\
  <main>\
    <h1>ClawPi Setup</h1>\
    <p>Finish first boot from your phone. ClawPi is broadcasting <strong>{setup_ssid}</strong>. If this page did not open automatically, visit <strong>{portal_url}</strong>.</p>\
    <div class=\"meta\">\
      <strong>What happens next</strong><br>\
      ClawPi will leave this setup network, join your home Wi-Fi, and continue booting. If the setup network comes back after a short pause, check the password and try again.\
    </div>\
    {error_html}\
    <form method=\"post\" action=\"/configure\">\
      <label for=\"ssid\">Home Wi-Fi name</label>\
      <input id=\"ssid\" name=\"ssid\" autocomplete=\"ssid\" value=\"{wifi_ssid}\" required>\
      <label for=\"passphrase\">Home Wi-Fi password</label>\
      <input id=\"passphrase\" name=\"passphrase\" type=\"password\" autocomplete=\"current-password\" minlength=\"8\" maxlength=\"63\" required>\
      <label for=\"device_name\">Device name</label>\
      <input id=\"device_name\" name=\"device_name\" value=\"{device_name}\" placeholder=\"clawpi-cm5\">\
      <label for=\"country\">Wi-Fi country code</label>\
      <input id=\"country\" name=\"country\" value=\"{wifi_country}\" maxlength=\"2\" placeholder=\"US\">\
      <button type=\"submit\">Connect ClawPi</button>\
    </form>\
  </main>\
</body>\
</html>",
            setup_ssid = escape_html(&self.setup_ssid),
            portal_url = escape_html(AP_PORTAL_URL),
            wifi_ssid = escape_html(&defaults.wifi_ssid),
            device_name = escape_html(&defaults.device_name),
            wifi_country = escape_html(&defaults.wifi_country),
            error_html = error_html,
        ))
    }

    fn render_transition_page(&self) -> String {
        format!(
            "<!doctype html>\
<html lang=\"en\">\
<head>\
  <meta charset=\"utf-8\">\
  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
  <title>ClawPi Connecting</title>\
  <style>\
    body {{ margin: 0; background: #14211c; color: #f5f3ea; font-family: ui-sans-serif, system-ui, sans-serif; }}\
    main {{ max-width: 32rem; margin: 0 auto; padding: 3rem 1.25rem; }}\
    h1 {{ font-size: 2rem; margin-bottom: 0.5rem; }}\
    p {{ line-height: 1.6; }}\
  </style>\
</head>\
<body>\
  <main>\
    <h1>ClawPi is switching networks</h1>\
    <p>This setup network will disappear while ClawPi joins your home Wi-Fi.</p>\
    <p>If the <strong>{setup_ssid}</strong> network comes back after a short pause, rejoin it and check the password you entered.</p>\
  </main>\
</body>\
</html>",
            setup_ssid = escape_html(&self.setup_ssid),
        )
    }

    fn hostapd_config_path(&self) -> PathBuf {
        self.portal_dir.join("hostapd.conf")
    }

    fn dnsmasq_config_path(&self) -> PathBuf {
        self.portal_dir.join("dnsmasq.conf")
    }

    fn dnsmasq_leases_path(&self) -> PathBuf {
        self.portal_dir.join("dnsmasq.leases")
    }

    fn write_hostapd_config(&self) -> io::Result<()> {
        let country = self.current_wifi_country()?;
        let content = format!(
            "interface={interface}\ndriver=nl80211\nssid={ssid}\nhw_mode=g\nchannel={channel}\nauth_algs=1\nwpa=0\nignore_broadcast_ssid=0\ncountry_code={country}\nieee80211d=1\nwmm_enabled=1\nlogger_stdout=-1\nlogger_stdout_level=0\n",
            interface = AP_INTERFACE,
            ssid = self.setup_ssid,
            channel = AP_CHANNEL,
            country = country,
        );
        fs::write(self.hostapd_config_path(), content)
    }

    fn write_dnsmasq_config(&self) -> io::Result<()> {
        let content = format!(
            "interface={interface}\nbind-interfaces\nlisten-address={gateway}\ndhcp-authoritative\ndhcp-broadcast\ndhcp-range={range}\ndhcp-option=3,{gateway}\ndhcp-option=6,{gateway}\ndhcp-option-force=114,{capport_api}\naddress=/#/{gateway}\nno-resolv\ndomain-needed\nbogus-priv\nlog-dhcp\ndhcp-leasefile={leases}\n",
            interface = AP_INTERFACE,
            range = AP_ADDRESS_RANGE,
            gateway = AP_GATEWAY,
            capport_api = AP_CAPPORT_API_URL,
            leases = self.dnsmasq_leases_path().display(),
        );
        fs::write(self.dnsmasq_config_path(), content)
    }
}

impl Drop for PortalRuntime {
    fn drop(&mut self) {
        if self.ap_active {
            let _ = self.restore_managed_wifi();
        } else {
            let _ = self.stop_setup_network();
        }
    }
}

struct Request {
    method: String,
    path: String,
    body: Vec<u8>,
}

impl Request {
    fn normalized_path(&self) -> String {
        self.path
            .split('?')
            .next()
            .unwrap_or("/")
            .trim()
            .to_string()
    }
}

struct FormDefaults {
    device_name: String,
    wifi_ssid: String,
    wifi_country: String,
}

fn read_http_request(stream: &mut TcpStream) -> io::Result<Request> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;

    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let bytes_read = stream.read(&mut chunk)?;
        if bytes_read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "request ended before headers completed",
            ));
        }
        buffer.extend_from_slice(&chunk[..bytes_read]);
        if let Some(index) = find_subsequence(&buffer, b"\r\n\r\n") {
            break index + 4;
        }
        if buffer.len() > 64 * 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request headers too large",
            ));
        }
    };

    let header_text = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let mut lines = header_text.lines();
    let request_line = lines
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing request line"))?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing method"))?;
    let path = request_parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing path"))?;

    let content_length = lines
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);

    while buffer.len() < header_end + content_length {
        let bytes_read = stream.read(&mut chunk)?;
        if bytes_read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..bytes_read]);
    }

    let body = buffer[header_end..buffer.len().min(header_end + content_length)].to_vec();

    Ok(Request {
        method: method.to_string(),
        path: path.to_string(),
        body,
    })
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn write_http_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: String,
) -> io::Result<()> {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
        body.as_bytes().len(),
        body
    );
    stream.write_all(response.as_bytes())
}

fn write_http_redirect(stream: &mut TcpStream, location: &str) -> io::Result<()> {
    let response = format!(
        "HTTP/1.1 302 Found\r\nLocation: {location}\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
    );
    stream.write_all(response.as_bytes())
}

fn parse_form_urlencoded(body: &str) -> HashMap<String, String> {
    let mut fields = HashMap::new();

    for pair in body.split('&') {
        if pair.is_empty() {
            continue;
        }

        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        fields.insert(percent_decode(name), percent_decode(value));
    }

    fields
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let upper = decode_hex(bytes[index + 1]);
                let lower = decode_hex(bytes[index + 2]);
                if let (Some(upper), Some(lower)) = (upper, lower) {
                    decoded.push((upper << 4) | lower);
                    index += 3;
                } else {
                    decoded.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }

    String::from_utf8_lossy(&decoded).into_owned()
}

fn decode_hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());

    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }

    escaped
}

fn build_setup_ssid() -> String {
    let suffix = read_optional_file(PathBuf::from("/sys/class/net/wlan0/address").as_path())
        .ok()
        .flatten()
        .map(|value| value.replace(':', "").to_uppercase())
        .filter(|value| value.len() >= 4)
        .map(|value| value[value.len() - 4..].to_string())
        .unwrap_or_else(|| String::from("CLAW"));

    format!("ClawPi Setup {suffix}")
}

fn write_status_file(layout: &Layout, content: &str) -> io::Result<()> {
    layout.ensure_dirs()?;
    fs::write(layout.portal_status_path(), content)
}

fn run_command(program: &str, args: &[&str]) -> io::Result<()> {
    match Command::new(program).args(args).status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(io::Error::new(
            io::ErrorKind::Other,
            format!("{program} exited with status {status}"),
        )),
        Err(err) => Err(err),
    }
}

fn command_succeeds(program: &str, args: &[&str]) -> bool {
    matches!(
        Command::new(program).args(args).status(),
        Ok(status) if status.success()
    )
}

fn stop_unit(unit: &str) {
    let _ = Command::new("systemctl").args(["stop", unit]).status();
}

fn stop_unit_if_loaded(unit: &str) -> io::Result<bool> {
    if !unit_is_loaded(unit)? {
        return Ok(false);
    }

    stop_unit(unit);
    Ok(true)
}

fn confirm_child_started(child: &mut Child, label: &str) -> io::Result<()> {
    thread::sleep(Duration::from_millis(500));
    if let Some(status) = child.try_wait()? {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("{label} exited early with status {status}"),
        ));
    }

    Ok(())
}

fn stop_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn remove_file_if_exists(path: &std::path::Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

fn has_ipv4_address(interface: &str) -> io::Result<bool> {
    let output = match Command::new("ip")
        .args(["-4", "addr", "show", "dev", interface])
        .output()
    {
        Ok(output) if output.status.success() => output,
        Ok(_) => return Ok(false),
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err),
    };

    Ok(String::from_utf8_lossy(&output.stdout).contains("inet "))
}

fn wait_for_ipv4_address(interface: &str, timeout: Duration) -> io::Result<bool> {
    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        if has_ipv4_address(interface)? {
            return Ok(true);
        }
        thread::sleep(Duration::from_secs(1));
    }

    Ok(false)
}

fn wait_for_wifi_connection(expected_ssid: &str) -> io::Result<bool> {
    let deadline = Instant::now() + JOIN_TIMEOUT;

    while Instant::now() < deadline {
        if wifi_joined(expected_ssid)? {
            return Ok(true);
        }
        thread::sleep(Duration::from_secs(1));
    }

    Ok(false)
}

fn wifi_joined(expected_ssid: &str) -> io::Result<bool> {
    let output = match Command::new("wpa_cli")
        .args(["-i", AP_INTERFACE, "status"])
        .output()
    {
        Ok(output) if output.status.success() => output,
        Ok(_) => return Ok(false),
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err),
    };

    let content = String::from_utf8_lossy(&output.stdout);
    let state = lookup_field(&content, "wpa_state");
    let ssid = lookup_field(&content, "ssid");
    let ip_address = lookup_field(&content, "ip_address");

    if state != Some("COMPLETED") || ssid != Some(expected_ssid) {
        return Ok(false);
    }

    if ip_address.is_some() {
        return Ok(true);
    }

    let ip_output = match Command::new("ip")
        .args(["-4", "addr", "show", "dev", AP_INTERFACE])
        .output()
    {
        Ok(output) if output.status.success() => output,
        Ok(_) => return Ok(false),
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err),
    };

    Ok(String::from_utf8_lossy(&ip_output.stdout).contains("inet "))
}

fn lookup_field<'a>(content: &'a str, field: &str) -> Option<&'a str> {
    let prefix = format!("{field}=");
    content.lines().find_map(|line| line.strip_prefix(&prefix))
}

fn sanitize_status_line(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch == '\n' || ch == '\r' { ' ' } else { ch })
        .collect()
}

fn start_unit(unit: &str) -> io::Result<()> {
    match Command::new("systemctl")
        .args(["--no-block", "start", unit])
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

fn start_unit_if_loaded(unit: &str) -> io::Result<bool> {
    if !unit_is_loaded(unit)? {
        return Ok(false);
    }

    start_unit(unit)?;
    Ok(true)
}

fn unit_is_loaded(unit: &str) -> io::Result<bool> {
    let output = match Command::new("systemctl")
        .args(["show", "-p", "LoadState", "--value", unit])
        .output()
    {
        Ok(output) if output.status.success() => output,
        Ok(_) => return Ok(false),
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err),
    };

    Ok(String::from_utf8_lossy(&output.stdout).trim() != "not-found")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_form_decodes_spaces_and_hex() {
        let fields = parse_form_urlencoded("ssid=ClawPi+Wifi&device_name=cm5%2D01");

        assert_eq!(fields.get("ssid"), Some(&String::from("ClawPi Wifi")));
        assert_eq!(fields.get("device_name"), Some(&String::from("cm5-01")));
    }

    #[test]
    fn escape_html_replaces_special_characters() {
        assert_eq!(escape_html("<wifi>&\"'"), "&lt;wifi&gt;&amp;&quot;&#39;");
    }
}
