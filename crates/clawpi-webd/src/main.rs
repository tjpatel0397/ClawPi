use clawpi_core::{
    ai_configured, device_hostname_label, inspect_state, local_url_for_device_name,
    read_optional_file, set_ai_profile, ClawPiConfig, Layout, Mode, DEFAULT_AI_MODEL,
    DEFAULT_AI_PROVIDER,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::ExitCode;
use std::time::Duration;

const OPENAI_RESPONSES_URL: &str = "https://api.openai.com/v1/responses";

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
            "false",
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

    write_status_file(
        layout,
        "serving",
        &hostname,
        &local_url,
        web_note(config),
        if ai_configured(config) {
            "true"
        } else {
            "false"
        },
    )?;

    for stream in listener.incoming() {
        let state = inspect_state(layout)?;
        if state.mode != Mode::Normal {
            write_status_file(
                layout,
                "stopped",
                &hostname,
                &local_url,
                "mode-changed-from-normal",
                "false",
            )?;
            return Ok(());
        }

        match stream {
            Ok(mut stream) => handle_connection(layout, &mut stream)?,
            Err(err) => return Err(err),
        }
    }

    Ok(())
}

fn handle_connection(layout: &Layout, stream: &mut TcpStream) -> io::Result<()> {
    let request = match read_http_request(stream) {
        Ok(request) => request,
        Err(err) => {
            write_http_response(
                stream,
                "400 Bad Request",
                "text/plain; charset=utf-8",
                format!("invalid request: {err}\n"),
            )?;
            return Ok(());
        }
    };

    let state = inspect_state(layout)?;
    if state.mode != Mode::Normal {
        write_http_response(
            stream,
            "409 Conflict",
            "text/plain; charset=utf-8",
            String::from("clawpi-webd only serves in normal mode\n"),
        )?;
        return Ok(());
    }

    let config = state.config_status.as_config().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "normal mode requires a valid config",
        )
    })?;

    match (request.method.as_str(), request.normalized_path().as_str()) {
        ("GET", "/health") => {
            write_http_response(
                stream,
                "204 No Content",
                "text/plain; charset=utf-8",
                String::new(),
            )?;
        }
        ("GET", "/status") => {
            write_http_response(
                stream,
                "200 OK",
                "text/plain; charset=utf-8",
                render_status_text(layout, config)?,
            )?;
        }
        ("POST", "/configure-ai") => {
            let fields = parse_form_urlencoded(&String::from_utf8_lossy(&request.body));
            let provider = fields
                .get("provider")
                .map(String::as_str)
                .unwrap_or(DEFAULT_AI_PROVIDER);
            let model = fields.get("model").map(String::as_str);
            let api_key = fields.get("api_key").map(String::as_str).unwrap_or("");

            match set_ai_profile(layout, provider, model, api_key) {
                Ok(_) => {
                    let updated_state = inspect_state(layout)?;
                    let updated_config =
                        updated_state.config_status.as_config().ok_or_else(|| {
                            io::Error::new(
                                io::ErrorKind::InvalidData,
                                "expected valid config after AI update",
                            )
                        })?;
                    write_runtime_status(layout, updated_config)?;
                    write_http_response(
                        stream,
                        "200 OK",
                        "text/html; charset=utf-8",
                        render_home_page(
                            layout,
                            updated_config,
                            Some("Claw AI is configured. You can start talking to the device now."),
                            None,
                            None,
                            None,
                        )?,
                    )?;
                }
                Err(err) => {
                    write_http_response(
                        stream,
                        "422 Unprocessable Entity",
                        "text/html; charset=utf-8",
                        render_home_page(
                            layout,
                            config,
                            None,
                            Some(&format!("failed to store AI settings: {err}")),
                            None,
                            None,
                        )?,
                    )?;
                }
            }
        }
        ("POST", "/prompt") => {
            let fields = parse_form_urlencoded(&String::from_utf8_lossy(&request.body));
            let prompt = fields.get("prompt").map(|value| value.trim()).unwrap_or("");

            if prompt.is_empty() {
                write_http_response(
                    stream,
                    "422 Unprocessable Entity",
                    "text/html; charset=utf-8",
                    render_home_page(
                        layout,
                        config,
                        None,
                        Some("prompt must not be empty"),
                        None,
                        None,
                    )?,
                )?;
                return Ok(());
            }

            match prompt_claw(config, prompt) {
                Ok(reply) => {
                    write_http_response(
                        stream,
                        "200 OK",
                        "text/html; charset=utf-8",
                        render_home_page(layout, config, None, None, Some(prompt), Some(&reply))?,
                    )?;
                }
                Err(err) => {
                    write_http_response(
                        stream,
                        "502 Bad Gateway",
                        "text/html; charset=utf-8",
                        render_home_page(
                            layout,
                            config,
                            None,
                            Some(&format!("Claw could not answer yet: {err}")),
                            Some(prompt),
                            None,
                        )?,
                    )?;
                }
            }
        }
        _ => {
            write_http_response(
                stream,
                "200 OK",
                "text/html; charset=utf-8",
                render_home_page(layout, config, None, None, None, None)?,
            )?;
        }
    }

    Ok(())
}

fn render_status_text(layout: &Layout, config: &ClawPiConfig) -> io::Result<String> {
    let hostname = device_hostname_label(&config.device_name);
    let local_url = local_url_for_device_name(&config.device_name);
    let session_status = read_optional_file(&layout.session_status_path())?
        .and_then(|content| lookup_field(&content, "status").map(String::from))
        .unwrap_or_else(|| String::from("absent"));

    Ok(format!(
        "status=ready\ndevice_name={}\nhostname={hostname}\nlocal_url={local_url}\nsession_status={session_status}\nai_configured={}\nai_provider={}\nai_model={}\n",
        config.device_name,
        ai_configured(config),
        config.ai_provider.as_deref().unwrap_or("unset"),
        config.ai_model.as_deref().unwrap_or("unset"),
    ))
}

fn render_home_page(
    layout: &Layout,
    config: &ClawPiConfig,
    notice: Option<&str>,
    error: Option<&str>,
    prompt: Option<&str>,
    answer: Option<&str>,
) -> io::Result<String> {
    let hostname = device_hostname_label(&config.device_name);
    let local_url = local_url_for_device_name(&config.device_name);
    let ai_ready = ai_configured(config);
    let session_summary = read_optional_file(&layout.session_status_path())?;
    let session_status = session_summary
        .as_deref()
        .and_then(|content| lookup_field(content, "status"))
        .unwrap_or("absent");
    let session_mode = session_summary
        .as_deref()
        .and_then(|content| lookup_field(content, "mode"))
        .unwrap_or("unknown");
    let heartbeat = session_summary
        .as_deref()
        .and_then(|content| lookup_field(content, "heartbeat_unix"))
        .unwrap_or("unknown");
    let wifi_ssid = config.wifi_ssid.as_deref().unwrap_or("unset");

    let notice_html = notice
        .map(|value| format!("<p class=\"notice notice-ok\">{}</p>", escape_html(value)))
        .unwrap_or_default();
    let error_html = error
        .map(|value| {
            format!(
                "<p class=\"notice notice-error\">{}</p>",
                escape_html(value)
            )
        })
        .unwrap_or_default();

    let gateway_panel = if ai_ready {
        format!(
            "<section class=\"panel\">\
               <h2>Claw Gateway</h2>\
               <p>Claw is configured and ready to answer through this local page.</p>\
               <form method=\"post\" action=\"/prompt\">\
                 <label for=\"prompt\">Ask Claw</label>\
                 <textarea id=\"prompt\" name=\"prompt\" rows=\"5\" placeholder=\"What should this device help with today?\">{}</textarea>\
                 <button type=\"submit\">Send To Claw</button>\
               </form>\
             </section>",
            escape_html(prompt.unwrap_or(""))
        )
    } else {
        format!(
            "<section class=\"panel\">\
               <h2>Finish Claw</h2>\
               <p>Wi-Fi is done. The next OS step is giving this device its AI runtime credentials so it can become a real local Claw gateway.</p>\
             </section>",
        )
    };

    let ai_settings_panel = format!(
        "<section class=\"panel\">\
           <h2>AI Settings</h2>\
           <p>{settings_copy}</p>\
           <form method=\"post\" action=\"/configure-ai\">\
             <input type=\"hidden\" name=\"provider\" value=\"{provider}\">\
             <label for=\"model\">AI model</label>\
             <input id=\"model\" name=\"model\" value=\"{model}\" placeholder=\"{model}\">\
             <label for=\"api_key\">OpenAI API key</label>\
             <input id=\"api_key\" name=\"api_key\" type=\"password\" placeholder=\"sk-...\" autocomplete=\"off\" required>\
             <button type=\"submit\">{button_label}</button>\
           </form>\
         </section>",
        settings_copy = if ai_ready {
            "Replace the model or API key used by the local Claw gateway."
        } else {
            "Store the local AI credentials that will wake Claw after first boot."
        },
        provider = DEFAULT_AI_PROVIDER,
        model = escape_html(config.ai_model.as_deref().unwrap_or(DEFAULT_AI_MODEL)),
        button_label = if ai_ready { "Update AI Settings" } else { "Wake Claw" },
    );

    let exchange_html = match (prompt, answer) {
        (Some(prompt), Some(answer)) => format!(
            "<section class=\"panel exchange\">\
               <h2>Latest Exchange</h2>\
               <div class=\"bubble bubble-user\"><strong>You</strong><br>{}</div>\
               <div class=\"bubble bubble-claw\"><strong>Claw</strong><br>{}</div>\
             </section>",
            escape_html(prompt),
            escape_html(answer)
        ),
        (Some(prompt), None) => format!(
            "<section class=\"panel exchange\">\
               <h2>Latest Exchange</h2>\
               <div class=\"bubble bubble-user\"><strong>You</strong><br>{}</div>\
             </section>",
            escape_html(prompt)
        ),
        (None, _) => String::new(),
    };

    Ok(format!(
        "<!doctype html>\
<html lang=\"en\">\
<head>\
  <meta charset=\"utf-8\">\
  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
  <title>{device_name} · ClawPi</title>\
  <style>\
    :root {{ color-scheme: light; font-family: \"Avenir Next\", \"Segoe UI\", sans-serif; }}\
    * {{ box-sizing: border-box; }}\
    body {{ margin: 0; color: #13211a; background: radial-gradient(circle at top left, #f3e4cf 0%, #eef3df 42%, #d5e6da 100%); }}\
    main {{ max-width: 64rem; margin: 0 auto; padding: 2rem 1.1rem 3rem; }}\
    h1 {{ margin: 0 0 0.35rem; font-size: 2.6rem; line-height: 1; letter-spacing: -0.03em; }}\
    h2 {{ margin: 0 0 0.65rem; font-size: 1.2rem; }}\
    p {{ line-height: 1.55; }}\
    .intro {{ display: grid; gap: 1rem; grid-template-columns: 1.2fr 0.8fr; align-items: start; }}\
    .hero, .panel {{ background: rgba(255,255,255,0.82); border: 1px solid rgba(19,33,26,0.08); border-radius: 1.2rem; box-shadow: 0 0.6rem 2rem rgba(19,33,26,0.08); }}\
    .hero {{ padding: 1.4rem; }}\
    .stack {{ display: grid; gap: 1rem; }}\
    .panel {{ padding: 1.15rem; }}\
    .status-grid {{ display: grid; gap: 0.75rem; grid-template-columns: repeat(2, minmax(0, 1fr)); margin-top: 1rem; }}\
    .stat {{ padding: 0.85rem 0.9rem; border-radius: 0.9rem; background: rgba(19,33,26,0.05); }}\
    .label {{ display: block; font-size: 0.76rem; text-transform: uppercase; letter-spacing: 0.08em; color: #51635b; }}\
    .value {{ display: block; margin-top: 0.2rem; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 0.95rem; }}\
    form {{ display: grid; gap: 0.85rem; margin-top: 1rem; }}\
    label {{ display: block; font-weight: 700; font-size: 0.95rem; }}\
    input, textarea {{ width: 100%; border: 1px solid #b7cbbf; border-radius: 0.95rem; background: rgba(255,255,255,0.94); color: #13211a; padding: 0.9rem 1rem; font: inherit; }}\
    textarea {{ resize: vertical; min-height: 8rem; }}\
    button {{ border: 0; border-radius: 999px; background: #184f37; color: #fff; padding: 0.95rem 1.15rem; font: inherit; font-weight: 700; cursor: pointer; }}\
    .notice {{ margin: 1rem 0 0; padding: 0.9rem 1rem; border-radius: 0.95rem; }}\
    .notice-ok {{ background: #dff5e8; color: #11482e; }}\
    .notice-error {{ background: #ffe5dd; color: #7b2413; }}\
    .bubble {{ padding: 0.95rem 1rem; border-radius: 1rem; margin-top: 0.8rem; line-height: 1.55; white-space: pre-wrap; }}\
    .bubble-user {{ background: #e6efe7; }}\
    .bubble-claw {{ background: #fff7db; }}\
    .subtle {{ color: #485b53; }}\
    @media (max-width: 760px) {{ .intro, .status-grid {{ grid-template-columns: 1fr; }} h1 {{ font-size: 2.1rem; }} }}\
  </style>\
</head>\
<body>\
  <main>\
    <section class=\"intro\">\
      <div class=\"hero\">\
        <p class=\"subtle\">Agentic OS gateway</p>\
        <h1>{device_name}</h1>\
        <p>ClawPi is on your network. This is the local control surface for finishing AI setup and talking to the device without touching SSH.</p>\
        {notice_html}\
        {error_html}\
      </div>\
      <div class=\"stack\">\
        <section class=\"panel\">\
          <h2>Device Status</h2>\
          <div class=\"status-grid\">\
            <div class=\"stat\"><span class=\"label\">Local URL</span><span class=\"value\">{local_url}</span></div>\
            <div class=\"stat\"><span class=\"label\">Hostname</span><span class=\"value\">{hostname}.local</span></div>\
            <div class=\"stat\"><span class=\"label\">Wi-Fi</span><span class=\"value\">{wifi_ssid}</span></div>\
            <div class=\"stat\"><span class=\"label\">Session</span><span class=\"value\">{session_status}</span></div>\
            <div class=\"stat\"><span class=\"label\">Mode</span><span class=\"value\">{session_mode}</span></div>\
            <div class=\"stat\"><span class=\"label\">Heartbeat</span><span class=\"value\">{heartbeat}</span></div>\
          </div>\
        </section>\
        <section class=\"panel\">\
          <h2>AI Runtime</h2>\
          <div class=\"status-grid\">\
            <div class=\"stat\"><span class=\"label\">Configured</span><span class=\"value\">{ai_ready}</span></div>\
            <div class=\"stat\"><span class=\"label\">Provider</span><span class=\"value\">{ai_provider}</span></div>\
            <div class=\"stat\"><span class=\"label\">Model</span><span class=\"value\">{ai_model}</span></div>\
            <div class=\"stat\"><span class=\"label\">Profile</span><span class=\"value\">local gateway</span></div>\
          </div>\
        </section>\
      </div>\
    </section>\
    {gateway_panel}\
    {ai_settings_panel}\
    {exchange_html}\
  </main>\
</body>\
</html>",
        device_name = escape_html(&config.device_name),
        local_url = escape_html(&local_url),
        hostname = escape_html(&hostname),
        wifi_ssid = escape_html(wifi_ssid),
        session_status = escape_html(session_status),
        session_mode = escape_html(session_mode),
        heartbeat = escape_html(heartbeat),
        ai_ready = if ai_ready { "true" } else { "false" },
        ai_provider = escape_html(config.ai_provider.as_deref().unwrap_or("unset")),
        ai_model = escape_html(config.ai_model.as_deref().unwrap_or(DEFAULT_AI_MODEL)),
        notice_html = notice_html,
        error_html = error_html,
        gateway_panel = gateway_panel,
        ai_settings_panel = ai_settings_panel,
        exchange_html = exchange_html,
    ))
}

fn prompt_claw(config: &ClawPiConfig, prompt: &str) -> io::Result<String> {
    if !ai_configured(config) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "AI is not configured yet",
        ));
    }

    let provider = config.ai_provider.as_deref().unwrap_or(DEFAULT_AI_PROVIDER);
    match provider {
        DEFAULT_AI_PROVIDER => prompt_openai(config, prompt),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unsupported ai provider: {provider}"),
        )),
    }
}

fn prompt_openai(config: &ClawPiConfig, prompt: &str) -> io::Result<String> {
    let api_key = config
        .ai_api_key
        .as_deref()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing ai_api_key"))?;
    let model = config.ai_model.as_deref().unwrap_or(DEFAULT_AI_MODEL);
    let instructions = format!(
        "You are Claw, the local operating-system companion for a Raspberry Pi device named {}. Be concise, practical, and device-oriented. If the user asks you to take actions that are not wired into this local gateway yet, say so directly and suggest the next step.",
        config.device_name
    );

    let payload = json!({
        "model": model,
        "instructions": instructions,
        "input": prompt,
    });

    let response = ureq::post(OPENAI_RESPONSES_URL)
        .set("Authorization", &format!("Bearer {api_key}"))
        .set("Content-Type", "application/json")
        .set("Accept", "application/json")
        .timeout(Duration::from_secs(60))
        .send_json(payload);

    let value: Value = match response {
        Ok(response) => response
            .into_json()
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?,
        Err(ureq::Error::Status(_, response)) => {
            let body = response
                .into_string()
                .unwrap_or_else(|_| String::from("request failed"));
            let message = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|value| {
                    value
                        .get("error")
                        .and_then(|error| error.get("message"))
                        .and_then(Value::as_str)
                        .map(String::from)
                })
                .unwrap_or(body);
            return Err(io::Error::new(io::ErrorKind::Other, message));
        }
        Err(ureq::Error::Transport(err)) => {
            return Err(io::Error::new(io::ErrorKind::Other, err.to_string()))
        }
    };

    extract_response_text(&value).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Other,
            "OpenAI response did not include assistant text",
        )
    })
}

fn extract_response_text(value: &Value) -> Option<String> {
    if let Some(text) = value.get("output_text").and_then(Value::as_str) {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    let mut parts = Vec::new();
    for output in value.get("output")?.as_array()? {
        for content in output.get("content")?.as_array()? {
            if let Some(text) = content.get("text").and_then(Value::as_str) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    parts.push(trimmed.to_string());
                }
            }
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}

fn web_note(config: &ClawPiConfig) -> &'static str {
    if ai_configured(config) {
        "gateway-ready"
    } else {
        "awaiting-ai-setup"
    }
}

fn write_runtime_status(layout: &Layout, config: &ClawPiConfig) -> io::Result<()> {
    let hostname = device_hostname_label(&config.device_name);
    let local_url = local_url_for_device_name(&config.device_name);
    write_status_file(
        layout,
        "serving",
        &hostname,
        &local_url,
        web_note(config),
        if ai_configured(config) {
            "true"
        } else {
            "false"
        },
    )
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

    let mut body = buffer[header_end..].to_vec();
    while body.len() < content_length {
        let bytes_read = stream.read(&mut chunk)?;
        if bytes_read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..bytes_read]);
    }
    body.truncate(content_length);

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

fn parse_form_urlencoded(value: &str) -> HashMap<String, String> {
    value
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (decode_form_component(key), decode_form_component(value))
        })
        .collect()
}

fn decode_form_component(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut result = String::with_capacity(value.len());
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                result.push(' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let hex = &value[index + 1..index + 3];
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        result.push(byte as char);
                        index += 3;
                    }
                    Err(_) => {
                        result.push('%');
                        index += 1;
                    }
                }
            }
            byte => {
                result.push(byte as char);
                index += 1;
            }
        }
    }

    result
}

fn lookup_field<'a>(content: &'a str, field: &str) -> Option<&'a str> {
    let prefix = format!("{field}=");
    content.lines().find_map(|line| line.strip_prefix(&prefix))
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
    ai_configured: &str,
) -> io::Result<()> {
    std::fs::write(
        layout.web_status_path(),
        format!(
            "phase=6\nservice=clawpi-webd\nstatus={status}\nhostname={hostname}\nlocal_url={local_url}\nai_configured={ai_configured}\nnote={note}\n"
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
