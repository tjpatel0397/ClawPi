use clawpi_core::{device_hostname_label, inspect_state, local_url_for_device_name, Layout, Mode};
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::ExitCode;

fn main() -> ExitCode {
    let layout = Layout::detect();

    match run(&layout) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("clawpi-webd: {err}");
            ExitCode::from(1)
        }
    }
}

fn run(layout: &Layout) -> io::Result<()> {
    layout.ensure_dirs()?;

    let state = inspect_state(layout)?;
    if state.mode != Mode::Normal {
        write_status_file(
            layout,
            "skipped",
            "unknown",
            "unknown",
            "mode-is-not-normal",
        )?;
        return Ok(());
    }

    let config = state.config_status.as_config().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "normal mode requires a valid config",
        )
    })?;
    let hostname = device_hostname_label(&config.device_name);
    let local_url = local_url_for_device_name(&config.device_name);
    let listener = TcpListener::bind("0.0.0.0:80")?;

    write_status_file(layout, "serving", &hostname, &local_url, "ready")?;

    for stream in listener.incoming() {
        let state = inspect_state(layout)?;
        if state.mode != Mode::Normal {
            write_status_file(
                layout,
                "stopped",
                &hostname,
                &local_url,
                "mode-changed-from-normal",
            )?;
            return Ok(());
        }

        match stream {
            Ok(mut stream) => {
                handle_connection(&mut stream, &config.device_name, &hostname, &local_url)?
            }
            Err(err) => return Err(err),
        }
    }

    Ok(())
}

fn handle_connection(
    stream: &mut TcpStream,
    device_name: &str,
    hostname: &str,
    local_url: &str,
) -> io::Result<()> {
    let request_line = read_request_line(stream)?;
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .split('?')
        .next()
        .unwrap_or("/");

    match path {
        "/health" => write_http_response(stream, "204 No Content", "text/plain; charset=utf-8", String::new()),
        "/status" => write_http_response(
            stream,
            "200 OK",
            "text/plain; charset=utf-8",
            format!(
                "status=ready\ndevice_name={device_name}\nhostname={hostname}\nlocal_url={local_url}\n",
            ),
        ),
        _ => write_http_response(
            stream,
            "200 OK",
            "text/html; charset=utf-8",
            render_home_page(device_name, hostname, local_url),
        ),
    }
}

fn render_home_page(device_name: &str, hostname: &str, local_url: &str) -> String {
    format!(
        "<!doctype html>\
<html lang=\"en\">\
<head>\
  <meta charset=\"utf-8\">\
  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
  <title>{device_name} · ClawPi</title>\
  <style>\
    :root {{ color-scheme: light; font-family: ui-sans-serif, system-ui, sans-serif; }}\
    body {{ margin: 0; background: linear-gradient(160deg, #f3efe4, #d9efe3); color: #14211c; }}\
    main {{ max-width: 42rem; margin: 0 auto; padding: 2rem 1.25rem 3rem; }}\
    h1 {{ margin-bottom: 0.4rem; font-size: 2.2rem; }}\
    p {{ line-height: 1.55; }}\
    .card {{ margin-top: 1.25rem; padding: 1.15rem 1.2rem; background: rgba(255,255,255,0.85); border-radius: 1rem; box-shadow: 0 0.5rem 2rem rgba(20,33,28,0.08); }}\
    .meta {{ font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 0.95rem; }}\
  </style>\
</head>\
<body>\
  <main>\
    <h1>{device_name}</h1>\
    <p>ClawPi is ready on your network. This local page is the non-technical handoff after first-boot setup.</p>\
    <div class=\"card\">\
      <strong>Open this device locally</strong><br>\
      <div class=\"meta\">{local_url}</div>\
    </div>\
    <div class=\"card\">\
      <strong>Device details</strong><br>\
      <div class=\"meta\">hostname={hostname}.local</div>\
      <div class=\"meta\">status=ready</div>\
    </div>\
  </main>\
</body>\
</html>",
        device_name = escape_html(device_name),
        hostname = escape_html(hostname),
        local_url = escape_html(local_url),
    )
}

fn read_request_line(stream: &mut TcpStream) -> io::Result<String> {
    let mut buffer = [0_u8; 2048];
    let bytes_read = stream.read(&mut buffer)?;
    if bytes_read == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "request ended before request line",
        ));
    }

    let request = String::from_utf8_lossy(&buffer[..bytes_read]);
    request
        .lines()
        .next()
        .map(|line| line.to_string())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing request line"))
}

fn write_http_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: String,
) -> io::Result<()> {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body,
    );
    stream.write_all(response.as_bytes())
}

fn write_status_file(
    layout: &Layout,
    status: &str,
    hostname: &str,
    local_url: &str,
    note: &str,
) -> io::Result<()> {
    std::fs::write(
        layout.web_status_path(),
        format!(
            "phase=6\nservice=clawpi-webd\nstatus={status}\nhostname={hostname}\nlocal_url={local_url}\nnote={note}\n"
        ),
    )
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
