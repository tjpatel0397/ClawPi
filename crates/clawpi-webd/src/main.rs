use clawpi_core::{
    ai_configured, device_hostname_label, inspect_state, local_url_for_device_name,
    read_optional_file, set_ai_profile, AgentPromptRequest, AgentPromptResponse, ClawPiConfig,
    Layout, Mode, DEFAULT_AI_MODEL, DEFAULT_AI_PROVIDER,
};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;
use std::time::Duration;

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
            let model = fields
                .get("model")
                .map(|value| value.trim())
                .filter(|value| !value.is_empty());
            let submitted_api_key = fields
                .get("api_key")
                .map(|value| value.trim())
                .unwrap_or("");
            let api_key = if submitted_api_key.is_empty() && ai_configured(config) {
                config.ai_api_key.as_deref().unwrap_or("")
            } else {
                submitted_api_key
            };

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
                            Some("Claw is configured. You can start talking to the device now."),
                            None,
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
                        None,
                    )?,
                )?;
                return Ok(());
            }

            match prompt_claw(layout, prompt) {
                Ok(reply) => {
                    write_http_response(
                        stream,
                        "200 OK",
                        "text/html; charset=utf-8",
                        render_home_page(
                            layout,
                            config,
                            None,
                            None,
                            None,
                            Some(prompt),
                            Some(&reply),
                        )?,
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
                render_home_page(layout, config, None, None, None, None, None)?,
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
    let agent_status = read_optional_file(&layout.agent_status_path())?
        .and_then(|content| lookup_field(&content, "status").map(String::from))
        .unwrap_or_else(|| String::from("absent"));

    Ok(format!(
        "status=ready\ndevice_name={}\nhostname={hostname}\nlocal_url={local_url}\nsession_status={session_status}\nagent_status={agent_status}\nai_configured={}\nai_provider={}\nai_model={}\n",
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
    draft_prompt: Option<&str>,
    last_prompt: Option<&str>,
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
    let wifi_ssid = config.wifi_ssid.as_deref().unwrap_or("unset");
    let ai_provider = config.ai_provider.as_deref().unwrap_or(DEFAULT_AI_PROVIDER);
    let ai_model = config.ai_model.as_deref().unwrap_or(DEFAULT_AI_MODEL);
    let ai_provider_label = if ai_provider.eq_ignore_ascii_case(DEFAULT_AI_PROVIDER) {
        "OpenAI"
    } else {
        ai_provider
    };

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

    let body_html = if ai_ready {
        render_chat_view(
            config,
            &hostname,
            &local_url,
            wifi_ssid,
            ai_provider_label,
            ai_model,
            session_status,
            session_mode,
            &notice_html,
            &error_html,
            draft_prompt,
            last_prompt,
            answer,
        )
    } else {
        render_setup_view(
            config,
            &hostname,
            &local_url,
            wifi_ssid,
            &notice_html,
            &error_html,
        )
    };

    Ok(render_document(&config.device_name, &body_html))
}

fn render_setup_view(
    config: &ClawPiConfig,
    hostname: &str,
    local_url: &str,
    wifi_ssid: &str,
    notice_html: &str,
    error_html: &str,
) -> String {
    let ai_form = render_ai_form(
        config,
        "Save AI Configuration",
        "sk-...",
        true,
        true,
        "This API key stays on the device and is used by the local Claw gateway.",
    );

    format!(
        "<section class=\"shell\">\
           <header class=\"shell-header\">\
             <div class=\"brand-row\">\
               <div>\
                 <p class=\"eyebrow\">ClawPi local console</p>\
                 <h1>{device_name}</h1>\
                 <p class=\"lede\">Wi-Fi is done. The next OS step is attaching the AI runtime that powers this device's local Claw console.</p>\
               </div>\
             </div>\
             <div class=\"chips\">\
               <span class=\"chip\">{hostname}.local</span>\
               <span class=\"chip\">{wifi_ssid}</span>\
               <span class=\"chip\">setup handoff</span>\
             </div>\
           </header>\
           <div class=\"surface\">\
             {notice_html}\
             {error_html}\
             <div class=\"setup-grid\">\
               <section class=\"panel intro-panel\">\
                 <p class=\"eyebrow\">Finish onboarding</p>\
                 <h2>Configure Claw</h2>\
                 <p>This browser handoff should stay narrow: configure the model, store the key, then switch into a single prompt surface at <strong>{local_url}</strong>.</p>\
                 <div class=\"meta-stack\">\
                   <div class=\"meta-row\"><span>Device</span><strong>{device_name}</strong></div>\
                   <div class=\"meta-row\"><span>Hostname</span><strong>{hostname}.local</strong></div>\
                   <div class=\"meta-row\"><span>Wi-Fi</span><strong>{wifi_ssid}</strong></div>\
                 </div>\
                 <p class=\"supporting-copy\">Richer on-device tool execution and OS actions can layer into this console later, but the surface should already feel simple and native from day one.</p>\
               </section>\
               <section class=\"panel form-panel\">\
                 <p class=\"eyebrow\">AI runtime</p>\
                 <h2>Connect the model</h2>\
                 <p>ClawPi currently speaks through OpenAI. Model selection stays editable after setup from inside the local console.</p>\
                 {ai_form}\
               </section>\
             </div>\
           </div>\
         </section>",
        device_name = escape_html(&config.device_name),
        hostname = escape_html(hostname),
        local_url = escape_html(local_url),
        wifi_ssid = escape_html(wifi_ssid),
        notice_html = notice_html,
        error_html = error_html,
        ai_form = ai_form,
    )
}

fn render_chat_view(
    config: &ClawPiConfig,
    hostname: &str,
    local_url: &str,
    wifi_ssid: &str,
    ai_provider: &str,
    ai_model: &str,
    session_status: &str,
    session_mode: &str,
    notice_html: &str,
    error_html: &str,
    draft_prompt: Option<&str>,
    last_prompt: Option<&str>,
    answer: Option<&str>,
) -> String {
    let ai_form = render_ai_form(
        config,
        "Update AI Configuration",
        "Leave blank to keep the current key",
        false,
        false,
        "Leave the key blank to keep the current secret. Re-enter it only when rotating credentials.",
    );
    let transcript_html = render_transcript(last_prompt, answer);

    format!(
        "<section class=\"shell\">\
           <header class=\"shell-header\">\
             <div class=\"brand-row\">\
               <div>\
                 <p class=\"eyebrow\">ClawPi local console</p>\
                 <h1>{device_name}</h1>\
                 <p class=\"lede\">A focused local prompt surface for talking to this device without a dashboard wrapped around it.</p>\
               </div>\
             </div>\
             <div class=\"chips\">\
               <span class=\"chip\">{hostname}.local</span>\
               <span class=\"chip\">{wifi_ssid}</span>\
               <span class=\"chip\">{ai_provider} / {ai_model}</span>\
             </div>\
           </header>\
           <div class=\"console-body\">\
             {notice_html}\
             {error_html}\
             <details class=\"details-toggle\">\
               <summary>\
                 <span>AI settings and device details</span>\
                 <span class=\"summary-copy\">edit the model, rotate the key, or inspect the local handoff state</span>\
               </summary>\
               <div class=\"details-content\">\
                 <section class=\"panel details-panel\">\
                   <p class=\"eyebrow\">AI runtime</p>\
                   <h2>Adjust the console</h2>\
                   <p>These settings stay tucked away so the main surface can behave like a normal agent console.</p>\
                   {ai_form}\
                 </section>\
                 <section class=\"panel details-panel\">\
                   <p class=\"eyebrow\">Device</p>\
                   <h2>Local context</h2>\
                   <div class=\"meta-stack\">\
                     <div class=\"meta-row\"><span>Local URL</span><strong>{local_url}</strong></div>\
                     <div class=\"meta-row\"><span>Hostname</span><strong>{hostname}.local</strong></div>\
                     <div class=\"meta-row\"><span>Wi-Fi</span><strong>{wifi_ssid}</strong></div>\
                     <div class=\"meta-row\"><span>Runtime</span><strong>{session_status}</strong></div>\
                     <div class=\"meta-row\"><span>Mode</span><strong>{session_mode}</strong></div>\
                   </div>\
                 </section>\
               </div>\
             </details>\
             <section class=\"transcript-shell\">\
               <div class=\"panel-heading\">\
                 <div>\
                   <p class=\"eyebrow\">Agent session</p>\
                   <h2>Ask Claw</h2>\
                 </div>\
                 <p class=\"panel-note\">One question in, one answer back. Keep the surface small while the deeper runtime comes online.</p>\
               </div>\
               <div class=\"transcript\">{transcript_html}</div>\
               <form method=\"post\" action=\"/prompt\" class=\"composer\">\
                 <label class=\"visually-hidden\" for=\"prompt\">Message Claw</label>\
                 <textarea id=\"prompt\" name=\"prompt\" rows=\"4\" class=\"composer-box\" placeholder=\"Ask a question about this device or what you want it to do next.\" autofocus>{draft_prompt}</textarea>\
                 <div class=\"composer-row\">\
                   <p class=\"composer-note\">Claw replies through the local agent service running on this device.</p>\
                   <button type=\"submit\">Send</button>\
                 </div>\
               </form>\
             </section>\
           </div>\
         </section>",
        device_name = escape_html(&config.device_name),
        hostname = escape_html(hostname),
        local_url = escape_html(local_url),
        wifi_ssid = escape_html(wifi_ssid),
        ai_provider = escape_html(ai_provider),
        ai_model = escape_html(ai_model),
        session_status = escape_html(session_status),
        session_mode = escape_html(session_mode),
        notice_html = notice_html,
        error_html = error_html,
        ai_form = ai_form,
        transcript_html = transcript_html,
        draft_prompt = escape_html(draft_prompt.unwrap_or("")),
    )
}

fn render_ai_form(
    config: &ClawPiConfig,
    submit_label: &str,
    api_key_placeholder: &str,
    api_key_required: bool,
    autofocus_api_key: bool,
    helper_copy: &str,
) -> String {
    let required_attr = if api_key_required { " required" } else { "" };
    let autofocus_attr = if autofocus_api_key { " autofocus" } else { "" };

    format!(
        "<form method=\"post\" action=\"/configure-ai\" class=\"config-form\">\
           <input type=\"hidden\" name=\"provider\" value=\"{provider}\">\
           <label>Provider</label>\
           <div class=\"provider-card\">\
             <div>\
               <div class=\"provider-name\">OpenAI</div>\
               <p class=\"provider-note\">First supported runtime for the local Claw gateway.</p>\
             </div>\
             <span class=\"chip chip-quiet\">active</span>\
           </div>\
           <label for=\"model\">Model</label>\
           <input id=\"model\" name=\"model\" value=\"{model}\" placeholder=\"{default_model}\" spellcheck=\"false\">\
           <label for=\"api_key\">API key</label>\
           <input id=\"api_key\" name=\"api_key\" type=\"password\" placeholder=\"{api_key_placeholder}\" autocomplete=\"off\" spellcheck=\"false\"{required_attr}{autofocus_attr}>\
           <p class=\"form-note\">{helper_copy}</p>\
           <button type=\"submit\">{submit_label}</button>\
         </form>",
        provider = DEFAULT_AI_PROVIDER,
        model = escape_html(config.ai_model.as_deref().unwrap_or(DEFAULT_AI_MODEL)),
        default_model = DEFAULT_AI_MODEL,
        api_key_placeholder = escape_html(api_key_placeholder),
        required_attr = required_attr,
        autofocus_attr = autofocus_attr,
        helper_copy = escape_html(helper_copy),
        submit_label = escape_html(submit_label),
    )
}

fn render_transcript(last_prompt: Option<&str>, answer: Option<&str>) -> String {
    match (last_prompt, answer) {
        (Some(prompt), Some(answer)) => format!(
            "<article class=\"message message-user\">\
               <span class=\"message-label\">You</span>\
               {prompt}\
             </article>\
             <article class=\"message message-assistant\">\
               <span class=\"message-label\">Claw</span>\
               {answer}\
             </article>",
            prompt = escape_html(prompt),
            answer = escape_html(answer),
        ),
        (Some(prompt), None) => format!(
            "<article class=\"message message-user\">\
               <span class=\"message-label\">You</span>\
               {prompt}\
             </article>",
            prompt = escape_html(prompt),
        ),
        (None, _) => String::from(
            "<article class=\"message message-assistant message-empty\">\
               <span class=\"message-label\">Claw</span>\
               Claw is configured on this device. Ask what you want explained, what task you want help with, or what this Pi should do next.\
             </article>",
        ),
    }
}

fn render_document(device_name: &str, body_html: &str) -> String {
    format!(
        "<!doctype html>\
<html lang=\"en\">\
<head>\
  <meta charset=\"utf-8\">\
  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
  <title>{device_name} · ClawPi</title>\
  <style>\
    :root {{\
      color-scheme: light;\
      --bg-0: #f3ead7;\
      --bg-1: #dfe8db;\
      --ink: #17211c;\
      --muted: #536159;\
      --line: rgba(23, 33, 28, 0.11);\
      --surface: rgba(248, 244, 236, 0.74);\
      --surface-strong: rgba(255, 255, 255, 0.82);\
      --chip: rgba(23, 33, 28, 0.06);\
      --accent: #1f5b42;\
      --accent-strong: #163d2d;\
      font-family: \"Sohne\", \"IBM Plex Sans\", \"Avenir Next\", \"Segoe UI Variable\", sans-serif;\
    }}\
    * {{ box-sizing: border-box; }}\
    body {{\
      margin: 0;\
      min-height: 100vh;\
      color: var(--ink);\
      background: radial-gradient(circle at top left, rgba(255, 241, 213, 0.92), transparent 32%), radial-gradient(circle at top right, rgba(202, 228, 210, 0.88), transparent 36%), linear-gradient(180deg, var(--bg-0) 0%, var(--bg-1) 100%);\
    }}\
    body::before {{\
      content: \"\";\
      position: fixed;\
      inset: 0;\
      pointer-events: none;\
      background: linear-gradient(180deg, rgba(255,255,255,0.18), rgba(255,255,255,0)), repeating-linear-gradient(135deg, rgba(23,33,28,0.03) 0, rgba(23,33,28,0.03) 1px, transparent 1px, transparent 14px);\
      opacity: 0.45;\
    }}\
    main {{\
      position: relative;\
      min-height: 100vh;\
      max-width: 72rem;\
      margin: 0 auto;\
      padding: 1.5rem 1rem 2rem;\
      display: grid;\
    }}\
    h1 {{ margin: 0; font-size: clamp(2.2rem, 5vw, 3.4rem); line-height: 0.98; letter-spacing: -0.05em; }}\
    h2 {{ margin: 0; font-size: 1.4rem; line-height: 1.1; letter-spacing: -0.03em; }}\
    p {{ margin: 0; line-height: 1.6; }}\
    strong {{ font-weight: 700; }}\
    .shell {{\
      align-self: stretch;\
      border: 1px solid var(--line);\
      border-radius: 1.85rem;\
      background: var(--surface);\
      backdrop-filter: blur(18px);\
      box-shadow: 0 1.5rem 4rem rgba(15, 23, 18, 0.14);\
      overflow: hidden;\
      animation: rise 0.42s ease;\
    }}\
    .shell-header {{\
      padding: 1.35rem 1.4rem 1.15rem;\
      background: linear-gradient(180deg, rgba(255,255,255,0.48), rgba(255,255,255,0.14));\
      border-bottom: 1px solid var(--line);\
    }}\
    .surface, .console-body {{ padding: 1.2rem 1.4rem 1.4rem; }}\
    .brand-row {{ display: flex; justify-content: space-between; gap: 1rem; align-items: start; }}\
    .chips {{ display: flex; flex-wrap: wrap; gap: 0.55rem; margin-top: 1rem; }}\
    .chip {{\
      display: inline-flex;\
      align-items: center;\
      gap: 0.35rem;\
      padding: 0.48rem 0.72rem;\
      border-radius: 999px;\
      background: var(--chip);\
      border: 1px solid rgba(23, 33, 28, 0.06);\
      font: 600 0.8rem/1 \"IBM Plex Mono\", \"SFMono-Regular\", Menlo, monospace;\
      color: var(--muted);\
    }}\
    .chip-quiet {{ background: rgba(23, 33, 28, 0.04); }}\
    .eyebrow {{\
      margin-bottom: 0.55rem;\
      font: 700 0.78rem/1 \"IBM Plex Mono\", \"SFMono-Regular\", Menlo, monospace;\
      text-transform: uppercase;\
      letter-spacing: 0.14em;\
      color: var(--muted);\
    }}\
    .lede {{ max-width: 42rem; margin-top: 0.7rem; color: var(--muted); }}\
    .setup-grid {{ display: grid; gap: 1rem; grid-template-columns: minmax(0, 1.08fr) minmax(0, 0.92fr); margin-top: 1rem; }}\
    .panel {{\
      padding: 1.2rem;\
      border-radius: 1.35rem;\
      background: var(--surface-strong);\
      border: 1px solid rgba(23, 33, 28, 0.08);\
      box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.72);\
    }}\
    .intro-panel, .details-panel {{ display: grid; gap: 0.9rem; align-content: start; }}\
    .supporting-copy, .panel-note, .form-note, .composer-note, .summary-copy, .provider-note {{ color: var(--muted); }}\
    .panel-note {{ max-width: 28rem; text-align: right; font-size: 0.93rem; }}\
    .meta-stack {{ display: grid; gap: 0.7rem; }}\
    .meta-row {{\
      display: flex;\
      justify-content: space-between;\
      align-items: center;\
      gap: 1rem;\
      padding: 0.85rem 0.95rem;\
      border-radius: 1rem;\
      background: rgba(23, 33, 28, 0.045);\
    }}\
    .meta-row span {{ color: var(--muted); }}\
    .meta-row strong {{\
      font: 600 0.92rem/1.4 \"IBM Plex Mono\", \"SFMono-Regular\", Menlo, monospace;\
      text-align: right;\
      word-break: break-word;\
    }}\
    .config-form, .composer {{ display: grid; gap: 0.88rem; margin-top: 1rem; }}\
    label {{\
      display: block;\
      font: 700 0.78rem/1 \"IBM Plex Mono\", \"SFMono-Regular\", Menlo, monospace;\
      text-transform: uppercase;\
      letter-spacing: 0.12em;\
      color: var(--muted);\
    }}\
    input, textarea {{\
      width: 100%;\
      border: 1px solid #c3cfc3;\
      border-radius: 1rem;\
      background: rgba(255, 255, 255, 0.92);\
      color: var(--ink);\
      padding: 0.95rem 1rem;\
      font: inherit;\
      box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.75);\
    }}\
    input:focus, textarea:focus {{\
      outline: 2px solid rgba(31, 91, 66, 0.18);\
      outline-offset: 2px;\
      border-color: rgba(31, 91, 66, 0.35);\
    }}\
    textarea {{ resize: vertical; min-height: 6.8rem; }}\
    .composer-box {{ min-height: 7rem; }}\
    .provider-card {{\
      display: flex;\
      justify-content: space-between;\
      gap: 1rem;\
      align-items: center;\
      padding: 0.9rem 1rem;\
      border-radius: 1rem;\
      border: 1px solid rgba(23, 33, 28, 0.08);\
      background: rgba(255, 255, 255, 0.68);\
    }}\
    .provider-name {{ font-weight: 700; }}\
    button {{\
      border: 0;\
      border-radius: 999px;\
      background: var(--accent);\
      color: #fff;\
      padding: 0.98rem 1.2rem;\
      font: inherit;\
      font-weight: 700;\
      cursor: pointer;\
      transition: background 140ms ease, transform 140ms ease;\
    }}\
    button:hover {{ background: var(--accent-strong); transform: translateY(-1px); }}\
    .notice {{ margin-top: 1rem; padding: 0.95rem 1rem; border-radius: 1rem; border: 1px solid transparent; }}\
    .notice-ok {{ background: rgba(220, 243, 228, 0.95); color: #12482f; border-color: rgba(18, 72, 47, 0.12); }}\
    .notice-error {{ background: rgba(255, 230, 220, 0.96); color: #7a2a19; border-color: rgba(122, 42, 25, 0.12); }}\
    .details-toggle {{\
      border-radius: 1.2rem;\
      background: rgba(255, 255, 255, 0.42);\
      border: 1px solid rgba(23, 33, 28, 0.08);\
      overflow: hidden;\
    }}\
    .details-toggle summary {{\
      list-style: none;\
      cursor: pointer;\
      display: flex;\
      justify-content: space-between;\
      align-items: center;\
      gap: 1rem;\
      padding: 1rem 1.1rem;\
      font-weight: 700;\
    }}\
    .details-toggle summary::-webkit-details-marker {{ display: none; }}\
    .details-toggle[open] summary {{ border-bottom: 1px solid rgba(23, 33, 28, 0.08); }}\
    .details-content {{ display: grid; gap: 1rem; grid-template-columns: minmax(0, 1fr) minmax(0, 0.92fr); padding: 1rem; }}\
    .transcript-shell {{ display: grid; gap: 1rem; }}\
    .panel-heading {{ display: flex; justify-content: space-between; gap: 1rem; align-items: end; }}\
    .transcript {{\
      display: grid;\
      gap: 0.85rem;\
      min-height: 20rem;\
      padding: 1.05rem;\
      border-radius: 1.35rem;\
      border: 1px solid rgba(23, 33, 28, 0.08);\
      background: rgba(23, 33, 28, 0.045);\
    }}\
    .message {{\
      max-width: 50rem;\
      padding: 1rem 1.05rem;\
      border-radius: 1.15rem;\
      line-height: 1.65;\
      white-space: pre-wrap;\
      overflow-wrap: anywhere;\
    }}\
    .message-user {{ margin-left: auto; background: #e4ede6; }}\
    .message-assistant {{ background: #fff3d7; border: 1px solid rgba(125, 93, 26, 0.1); }}\
    .message-empty {{ max-width: none; background: rgba(255, 255, 255, 0.82); border-style: dashed; }}\
    .message-label {{\
      display: block;\
      margin-bottom: 0.4rem;\
      font: 700 0.72rem/1 \"IBM Plex Mono\", \"SFMono-Regular\", Menlo, monospace;\
      text-transform: uppercase;\
      letter-spacing: 0.12em;\
      color: var(--muted);\
    }}\
    .composer-row {{ display: flex; justify-content: space-between; gap: 1rem; align-items: center; }}\
    .visually-hidden {{\
      position: absolute;\
      width: 1px;\
      height: 1px;\
      padding: 0;\
      margin: -1px;\
      overflow: hidden;\
      clip: rect(0, 0, 0, 0);\
      white-space: nowrap;\
      border: 0;\
    }}\
    @keyframes rise {{ from {{ opacity: 0; transform: translateY(16px); }} to {{ opacity: 1; transform: none; }} }}\
    @media (max-width: 900px) {{\
      .setup-grid, .details-content {{ grid-template-columns: 1fr; }}\
      .panel-heading {{ align-items: start; flex-direction: column; }}\
      .panel-note {{ text-align: left; max-width: none; }}\
    }}\
    @media (max-width: 760px) {{\
      main {{ padding: 0.85rem 0.75rem 1.25rem; }}\
      .shell-header, .surface, .console-body {{ padding: 1rem; }}\
      .brand-row, .composer-row, .details-toggle summary {{ flex-direction: column; align-items: start; }}\
      .meta-row {{ flex-direction: column; align-items: start; }}\
      .meta-row strong {{ text-align: left; }}\
      button {{ width: 100%; }}\
      .message {{ max-width: none; }}\
    }}\
  </style>\
</head>\
<body>\
  <main>{body_html}</main>\
</body>\
</html>",
        device_name = escape_html(device_name),
        body_html = body_html,
    )
}

fn prompt_claw(layout: &Layout, prompt: &str) -> io::Result<String> {
    let mut stream = match UnixStream::connect(layout.agent_socket_path()) {
        Ok(stream) => stream,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "local Claw agent is not running yet",
            ))
        }
        Err(err) if err.kind() == io::ErrorKind::ConnectionRefused => {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionRefused,
                "local Claw agent is unavailable",
            ))
        }
        Err(err) => return Err(err),
    };

    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let request = AgentPromptRequest {
        prompt: prompt.to_string(),
    };
    let body = serde_json::to_vec(&request)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;
    stream.write_all(&body)?;
    stream.shutdown(Shutdown::Write)?;

    stream.set_read_timeout(Some(Duration::from_secs(70)))?;
    let mut response_body = Vec::new();
    stream.read_to_end(&mut response_body)?;
    if response_body.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "local Claw agent returned no response",
        ));
    }

    let response: AgentPromptResponse = serde_json::from_slice(&response_body)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;

    match (response.reply, response.error) {
        (Some(reply), None) => Ok(reply),
        (None, Some(error)) => Err(io::Error::new(io::ErrorKind::Other, error)),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "local Claw agent returned an invalid response",
        )),
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

#[cfg(test)]
mod tests {
    use super::*;
    use clawpi_core::{SetupState, CONFIG_VERSION, DEFAULT_WIFI_COUNTRY, RUNTIME_PROFILE};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn render_home_page_shows_setup_shell_before_ai_configuration() {
        let layout = test_layout("setup");
        let html = render_home_page(&layout, &base_config(), None, None, None, None, None).unwrap();

        assert!(html.contains("Configure Claw"));
        assert!(html.contains("Save AI Configuration"));
        assert!(!html.contains("AI settings and device details"));
    }

    #[test]
    fn render_home_page_shows_console_after_ai_configuration() {
        let layout = test_layout("console");
        let mut config = base_config();
        config.ai_provider = Some(String::from(DEFAULT_AI_PROVIDER));
        config.ai_model = Some(String::from(DEFAULT_AI_MODEL));
        config.ai_api_key = Some(String::from("sk-test-secret"));

        let html = render_home_page(&layout, &config, None, None, None, None, None).unwrap();

        assert!(html.contains("Ask Claw"));
        assert!(html.contains("AI settings and device details"));
        assert!(html.contains("OpenAI / gpt-5.4"));
        assert!(!html.contains("Connect the model"));
    }

    fn base_config() -> ClawPiConfig {
        ClawPiConfig {
            config_version: CONFIG_VERSION,
            device_name: String::from("clawpi"),
            setup_state: SetupState::Complete,
            runtime_profile: String::from(RUNTIME_PROFILE),
            wifi_country: String::from(DEFAULT_WIFI_COUNTRY),
            wifi_ssid: Some(String::from("Lab WiFi")),
            wifi_passphrase: None,
            ai_provider: None,
            ai_model: None,
            ai_api_key: None,
        }
    }

    fn test_layout(label: &str) -> Layout {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Layout::from_root(std::env::temp_dir().join(format!("clawpi-webd-{label}-{unique}")))
    }
}
