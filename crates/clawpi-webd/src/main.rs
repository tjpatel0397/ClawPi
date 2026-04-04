use clawpi_core::{
    ai_configured, device_hostname_label, inspect_state, local_url_for_device_name,
    read_optional_file, set_ai_profile, AgentPromptRequest, AgentPromptResponse, ClawPiConfig,
    Layout, Mode, DEFAULT_AI_MODEL, DEFAULT_AI_PROVIDER,
};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;
use std::time::Duration;

const MODEL_CUSTOM_ID: &str = "__custom__";
const AUTH_MODE_API_KEY: &str = "api_key";
const AUTH_MODE_DEVICE_LOGIN: &str = "device_login";
const AUTH_MODE_LOCAL: &str = "local";
const AUTH_MODE_NO_KEY: &str = "no_key";

#[derive(Clone, Copy, Serialize)]
struct UiAuthOption {
    id: &'static str,
    label: &'static str,
    secret_label: Option<&'static str>,
    secret_placeholder: Option<&'static str>,
    requires_secret: bool,
}

#[derive(Clone, Copy, Serialize)]
struct UiModelOption {
    id: &'static str,
    label: &'static str,
}

#[derive(Clone, Copy, Serialize)]
struct UiProviderPreset {
    id: &'static str,
    label: &'static str,
    hint: &'static str,
    route_editable: bool,
    route_placeholder: &'static str,
    default_model: &'static str,
    default_auth: &'static str,
    auth_options: &'static [UiAuthOption],
    models: &'static [UiModelOption],
}

const AUTH_API_KEY: UiAuthOption = UiAuthOption {
    id: AUTH_MODE_API_KEY,
    label: "API key",
    secret_label: Some("API key"),
    secret_placeholder: Some("sk-..."),
    requires_secret: true,
};

const AUTH_ANTHROPIC_KEY: UiAuthOption = UiAuthOption {
    id: AUTH_MODE_API_KEY,
    label: "Key or token",
    secret_label: Some("API key or setup token"),
    secret_placeholder: Some("sk-ant-..."),
    requires_secret: true,
};

const AUTH_DEVICE_LOGIN: UiAuthOption = UiAuthOption {
    id: AUTH_MODE_DEVICE_LOGIN,
    label: "Device login",
    secret_label: None,
    secret_placeholder: None,
    requires_secret: false,
};

const AUTH_LOCAL: UiAuthOption = UiAuthOption {
    id: AUTH_MODE_LOCAL,
    label: "Local",
    secret_label: None,
    secret_placeholder: None,
    requires_secret: false,
};

const OPENROUTER_MODELS: &[UiModelOption] = &[
    UiModelOption {
        id: "anthropic/claude-opus-4-6",
        label: "Claude Opus 4.6",
    },
    UiModelOption {
        id: "anthropic/claude-sonnet-4-6",
        label: "Claude Sonnet 4.6",
    },
    UiModelOption {
        id: "openai/gpt-5.4",
        label: "GPT-5.4",
    },
    UiModelOption {
        id: "openai/gpt-5.2",
        label: "GPT-5.2",
    },
    UiModelOption {
        id: "google/gemini-3.1-pro",
        label: "Gemini 3.1 Pro",
    },
    UiModelOption {
        id: "google/gemini-3.1-flash",
        label: "Gemini 3.1 Flash",
    },
    UiModelOption {
        id: "meta-llama/llama-4-maverick",
        label: "Llama 4 Maverick",
    },
    UiModelOption {
        id: "meta-llama/llama-4-scout",
        label: "Llama 4 Scout",
    },
    UiModelOption {
        id: "deepseek/deepseek-v4",
        label: "DeepSeek V4",
    },
    UiModelOption {
        id: "deepseek/deepseek-r1",
        label: "DeepSeek R1",
    },
    UiModelOption {
        id: "mistralai/mistral-large-3",
        label: "Mistral Large 3",
    },
    UiModelOption {
        id: "moonshot/kimi-k2.5",
        label: "Kimi K2.5",
    },
];

const ANTHROPIC_MODELS: &[UiModelOption] = &[
    UiModelOption {
        id: "claude-opus-4-6",
        label: "Claude Opus 4.6",
    },
    UiModelOption {
        id: "claude-sonnet-4-6",
        label: "Claude Sonnet 4.6",
    },
];

const OPENAI_MODELS: &[UiModelOption] = &[
    UiModelOption {
        id: "gpt-5.4",
        label: "GPT-5.4",
    },
    UiModelOption {
        id: "gpt-5.2",
        label: "GPT-5.2",
    },
    UiModelOption {
        id: "o4-mini",
        label: "o4-mini",
    },
];

const OPENAI_CODEX_MODELS: &[UiModelOption] = &[
    UiModelOption {
        id: "gpt-5.3-codex",
        label: "GPT-5.3 Codex",
    },
    UiModelOption {
        id: "gpt-5.2-codex",
        label: "GPT-5.2 Codex",
    },
];

const GEMINI_MODELS: &[UiModelOption] = &[
    UiModelOption {
        id: "gemini-3.1-pro",
        label: "Gemini 3.1 Pro",
    },
    UiModelOption {
        id: "gemini-3.1-flash",
        label: "Gemini 3.1 Flash",
    },
];

const GROQ_MODELS: &[UiModelOption] = &[
    UiModelOption {
        id: "llama-4-maverick-17b-128e",
        label: "Llama 4 Maverick",
    },
    UiModelOption {
        id: "llama-4-scout-17b-16e",
        label: "Llama 4 Scout",
    },
    UiModelOption {
        id: "deepseek-r1-distill-llama-70b",
        label: "DeepSeek R1 70B",
    },
];

const OLLAMA_MODELS: &[UiModelOption] = &[
    UiModelOption {
        id: "llama4:maverick",
        label: "Llama 4 Maverick",
    },
    UiModelOption {
        id: "llama4:scout",
        label: "Llama 4 Scout",
    },
    UiModelOption {
        id: "gemma4",
        label: "Gemma 4",
    },
    UiModelOption {
        id: "deepseek-r1:7b",
        label: "DeepSeek R1 7B",
    },
    UiModelOption {
        id: "mistral",
        label: "Mistral",
    },
];

const KIMI_MODELS: &[UiModelOption] = &[
    UiModelOption {
        id: "kimi-k2.5",
        label: "Kimi K2.5",
    },
    UiModelOption {
        id: "kimi-k2",
        label: "Kimi K2",
    },
];

const PROVIDER_PRESETS: &[UiProviderPreset] = &[
    UiProviderPreset {
        id: "openrouter",
        label: "OpenRouter",
        hint: "One key, many models",
        route_editable: false,
        route_placeholder: "openrouter",
        default_model: DEFAULT_AI_MODEL,
        default_auth: AUTH_MODE_API_KEY,
        auth_options: &[AUTH_API_KEY],
        models: OPENROUTER_MODELS,
    },
    UiProviderPreset {
        id: "anthropic",
        label: "Anthropic",
        hint: "Claude direct",
        route_editable: false,
        route_placeholder: "anthropic",
        default_model: "claude-sonnet-4-6",
        default_auth: AUTH_MODE_API_KEY,
        auth_options: &[AUTH_ANTHROPIC_KEY],
        models: ANTHROPIC_MODELS,
    },
    UiProviderPreset {
        id: "openai",
        label: "OpenAI",
        hint: "GPT direct",
        route_editable: false,
        route_placeholder: "openai",
        default_model: "gpt-5.4",
        default_auth: AUTH_MODE_API_KEY,
        auth_options: &[AUTH_API_KEY],
        models: OPENAI_MODELS,
    },
    UiProviderPreset {
        id: "openai-codex",
        label: "OpenAI Codex",
        hint: "ChatGPT account",
        route_editable: false,
        route_placeholder: "openai-codex",
        default_model: "gpt-5.3-codex",
        default_auth: AUTH_MODE_DEVICE_LOGIN,
        auth_options: &[AUTH_DEVICE_LOGIN],
        models: OPENAI_CODEX_MODELS,
    },
    UiProviderPreset {
        id: "gemini",
        label: "Gemini",
        hint: "Key or device login",
        route_editable: false,
        route_placeholder: "gemini",
        default_model: "gemini-3.1-flash",
        default_auth: AUTH_MODE_API_KEY,
        auth_options: &[AUTH_API_KEY, AUTH_DEVICE_LOGIN],
        models: GEMINI_MODELS,
    },
    UiProviderPreset {
        id: "groq",
        label: "Groq",
        hint: "Fast inference",
        route_editable: false,
        route_placeholder: "groq",
        default_model: "llama-4-maverick-17b-128e",
        default_auth: AUTH_MODE_API_KEY,
        auth_options: &[AUTH_API_KEY],
        models: GROQ_MODELS,
    },
    UiProviderPreset {
        id: "kimi",
        label: "Kimi",
        hint: "Moonshot AI",
        route_editable: false,
        route_placeholder: "kimi",
        default_model: "kimi-k2.5",
        default_auth: AUTH_MODE_API_KEY,
        auth_options: &[AUTH_API_KEY],
        models: KIMI_MODELS,
    },
    UiProviderPreset {
        id: "ollama",
        label: "Ollama",
        hint: "Local on this device",
        route_editable: false,
        route_placeholder: "ollama",
        default_model: "llama4:maverick",
        default_auth: AUTH_MODE_LOCAL,
        auth_options: &[AUTH_LOCAL],
        models: OLLAMA_MODELS,
    },
];

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
            let provider = resolve_provider(&fields);
            let model = resolve_model(&fields);
            let auth_mode = fields
                .get("auth_mode")
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
                .unwrap_or(AUTH_MODE_API_KEY);
            let submitted_api_key = fields
                .get("api_key")
                .map(|value| value.trim())
                .unwrap_or("");
            let api_key = match resolve_ai_secret(config, &provider, auth_mode, submitted_api_key) {
                Ok(api_key) => api_key,
                Err(err) => {
                    write_http_response(
                        stream,
                        "422 Unprocessable Entity",
                        "text/html; charset=utf-8",
                        render_home_page(
                            layout,
                            config,
                            None,
                            Some(&err.to_string()),
                            None,
                            None,
                            None,
                        )?,
                    )?;
                    return Ok(());
                }
            };

            match set_ai_profile(layout, &provider, model.as_deref(), api_key.as_deref()) {
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
                        render_home_page(layout, updated_config, None, None, None, None, None)?,
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
    _layout: &Layout,
    config: &ClawPiConfig,
    notice: Option<&str>,
    error: Option<&str>,
    draft_prompt: Option<&str>,
    last_prompt: Option<&str>,
    answer: Option<&str>,
) -> io::Result<String> {
    let ai_ready = ai_configured(config);
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

    let body_html = if ai_ready {
        render_chat_view(
            config,
            wifi_ssid,
            &notice_html,
            &error_html,
            draft_prompt,
            last_prompt,
            answer,
        )
    } else {
        render_setup_view(config, wifi_ssid, &notice_html, &error_html)
    };

    Ok(render_document(&config.device_name, &body_html))
}

fn render_setup_view(
    config: &ClawPiConfig,
    wifi_ssid: &str,
    notice_html: &str,
    error_html: &str,
) -> String {
    let ai_form = render_ai_form(config, "setup-ai", "Continue", "setup");

    format!(
        "<div class=\"hint\">Set up your AI provider to get started.</div>\
         {notice_html}\
         {error_html}\
         {ai_form}\
         {device_info}",
        notice_html = notice_html,
        error_html = error_html,
        ai_form = ai_form,
        device_info = render_device_info(config, wifi_ssid),
    )
}

fn render_chat_view(
    config: &ClawPiConfig,
    wifi_ssid: &str,
    notice_html: &str,
    error_html: &str,
    draft_prompt: Option<&str>,
    last_prompt: Option<&str>,
    answer: Option<&str>,
) -> String {
    let ai_form = render_ai_form(config, "console-ai", "Save", "update");
    let transcript_html = render_transcript(last_prompt, answer);
    let model_label = config.ai_model.as_deref().unwrap_or("unknown");
    let provider_label = config.ai_provider.as_deref().unwrap_or("unknown");

    format!(
        "<div class=\"terminal\">\
           <div class=\"terminal-output\" id=\"output\">\
             {notice_html}\
             {error_html}\
             {transcript_html}\
           </div>\
           <div class=\"terminal-input\">\
             <form method=\"post\" action=\"/prompt\" class=\"input-line\" id=\"prompt-form\">\
               <span class=\"input-prefix\">&gt;</span>\
               <textarea id=\"prompt\" name=\"prompt\" rows=\"1\" placeholder=\"Message Claw... (type /help for commands)\" autofocus>{draft_prompt}</textarea>\
             </form>\
           </div>\
           <div class=\"statusbar\">\
             <span>{provider_label}/{model_label}</span>\
             <span>{device_name} · {wifi_ssid}</span>\
           </div>\
         </div>\
         <div class=\"settings-panel is-hidden\" id=\"settings-panel\">\
           <div class=\"settings-header\">\
             <span>Settings</span>\
             <button type=\"button\" onclick=\"document.getElementById('settings-panel').classList.add('is-hidden')\">Close</button>\
           </div>\
           {ai_form}\
         </div>",
        notice_html = notice_html,
        error_html = error_html,
        transcript_html = transcript_html,
        draft_prompt = escape_html(draft_prompt.unwrap_or("")),
        provider_label = escape_html(provider_label),
        model_label = escape_html(model_label),
        device_name = escape_html(&config.device_name),
        wifi_ssid = escape_html(wifi_ssid),
        ai_form = ai_form,
    )
}

fn render_ai_form(
    config: &ClawPiConfig,
    form_id: &str,
    submit_label: &str,
    form_mode: &str,
) -> String {
    let initial_provider = config.ai_provider.as_deref().unwrap_or("");
    let initial_model = config.ai_model.as_deref().unwrap_or("");
    let initial_has_secret = config
        .ai_api_key
        .as_deref()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    let api_key_placeholder = if form_mode == "update" && initial_has_secret {
        "Current key saved"
    } else {
        "sk-..."
    };

    let provider_options_html = render_provider_select_options(initial_provider);

    format!(
        "<form method=\"post\" action=\"/configure-ai\" class=\"ai-config-form form-stack\" data-form-mode=\"{form_mode}\" data-initial-provider=\"{initial_provider}\" data-initial-model=\"{initial_model}\" data-initial-has-secret=\"{initial_has_secret}\">\
           <input type=\"hidden\" name=\"auth_mode\" value=\"\">\
           <div class=\"field\">\
             <label for=\"{form_id}-provider\">Provider</label>\
             <select id=\"{form_id}-provider\" name=\"provider_value\" data-provider-select>\
               {provider_options_html}\
             </select>\
           </div>\
           <div class=\"field is-hidden\" data-field=\"auth\">\
             <label for=\"{form_id}-auth\">Sign-in method</label>\
             <select id=\"{form_id}-auth\" data-auth-select></select>\
           </div>\
           <div class=\"field is-hidden\" data-field=\"credential\">\
             <label for=\"{form_id}-api-key\" data-credential-label>API key</label>\
             <input id=\"{form_id}-api-key\" name=\"api_key\" type=\"password\" placeholder=\"{api_key_placeholder}\" autocomplete=\"off\" spellcheck=\"false\">\
           </div>\
           <div class=\"field is-hidden\" data-field=\"model\">\
             <label for=\"{form_id}-model\">Model</label>\
             <select id=\"{form_id}-model\" name=\"model\" data-model-select></select>\
           </div>\
           <button type=\"submit\">{submit_label}</button>\
         </form>",
        form_mode = escape_html(form_mode),
        initial_provider = escape_html(initial_provider),
        initial_model = escape_html(initial_model),
        initial_has_secret = if initial_has_secret { "true" } else { "false" },
        form_id = escape_html(form_id),
        api_key_placeholder = escape_html(api_key_placeholder),
        provider_options_html = provider_options_html,
        submit_label = escape_html(submit_label),
    )
}

fn render_transcript(last_prompt: Option<&str>, answer: Option<&str>) -> String {
    match (last_prompt, answer) {
        (Some(prompt), Some(answer)) => format!(
            "<div class=\"msg msg-user\"><span class=\"msg-prefix\">you&gt;</span> {prompt}</div>\
             <div class=\"msg msg-claw\"><span class=\"msg-prefix\">claw&gt;</span> {answer}</div>",
            prompt = escape_html(prompt),
            answer = escape_html(answer),
        ),
        (Some(prompt), None) => format!(
            "<div class=\"msg msg-user\"><span class=\"msg-prefix\">you&gt;</span> {prompt}</div>",
            prompt = escape_html(prompt),
        ),
        (None, _) => String::from(
            "<div class=\"msg msg-empty\">No messages yet.</div>",
        ),
    }
}

fn render_device_info(config: &ClawPiConfig, wifi_ssid: &str) -> String {
    format!(
        "<div class=\"device-info\">{device_name} · {wifi_ssid}</div>",
        device_name = escape_html(&config.device_name),
        wifi_ssid = escape_html(wifi_ssid),
    )
}

fn render_provider_select_options(selected: &str) -> String {
    let mut html = String::from("<option value=\"\">-- select --</option>");
    for preset in PROVIDER_PRESETS {
        let sel = if selected.eq_ignore_ascii_case(preset.id) {
            " selected"
        } else {
            ""
        };
        html.push_str(&format!(
            "<option value=\"{id}\"{sel}>{label}</option>",
            id = preset.id,
            sel = sel,
            label = escape_html(preset.label),
        ));
    }
    html
}

fn resolve_provider(fields: &HashMap<String, String>) -> String {
    fields
        .get("provider_value")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            fields
                .get("provider_custom")
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
        })
        .or_else(|| {
            fields
                .get("provider")
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
        })
        .unwrap_or(DEFAULT_AI_PROVIDER)
        .to_string()
}

fn resolve_model(fields: &HashMap<String, String>) -> Option<String> {
    let model = fields
        .get("model")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());

    if model == Some(MODEL_CUSTOM_ID) {
        return fields
            .get("model_custom")
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(String::from);
    }

    model
        .or_else(|| {
            fields
                .get("model_custom")
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
        })
        .or_else(|| {
            fields
                .get("model_value")
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
        })
        .map(String::from)
}

fn resolve_ai_secret(
    config: &ClawPiConfig,
    provider: &str,
    auth_mode: &str,
    submitted_secret: &str,
) -> io::Result<Option<String>> {
    let trimmed_secret = submitted_secret.trim();
    PROVIDER_PRESETS
        .iter()
        .find(|preset| provider.eq_ignore_ascii_case(preset.id))
        .and_then(|preset| {
            preset
                .auth_options
                .iter()
                .find(|option| option.id == auth_mode)
                .copied()
        })
        .map_or_else(
            || match auth_mode {
                AUTH_MODE_DEVICE_LOGIN | AUTH_MODE_LOCAL | AUTH_MODE_NO_KEY => Ok(None),
                _ if !trimmed_secret.is_empty() => Ok(Some(trimmed_secret.to_string())),
                _ => Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Enter a key to continue.",
                )),
            },
            |auth_option| {
                if auth_option.requires_secret {
                    if !trimmed_secret.is_empty() {
                        return Ok(Some(trimmed_secret.to_string()));
                    }

                    let can_reuse_secret = config
                        .ai_provider
                        .as_deref()
                        .is_some_and(|current| current == provider)
                        && config
                            .ai_api_key
                            .as_deref()
                            .is_some_and(|value| !value.trim().is_empty());

                    if can_reuse_secret {
                        Ok(config.ai_api_key.clone())
                    } else {
                        Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "Enter a key to continue.",
                        ))
                    }
                } else {
                    Ok(None)
                }
            },
        )
}

fn render_provider_catalog_json() -> String {
    serde_json::to_string(PROVIDER_PRESETS)
        .unwrap_or_else(|_| String::from("[]"))
        .replace('<', "\\u003c")
}

fn render_document(device_name: &str, body_html: &str) -> String {
    let provider_catalog_json = render_provider_catalog_json();
    let ui_script = render_ui_script();
    format!(
        "<!doctype html>\
<html lang=\"en\">\
<head>\
  <meta charset=\"utf-8\">\
  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
  <title>{device_name} · ClawPi</title>\
  <style>\
    :root {{ color-scheme: dark; }}\
    * {{ box-sizing: border-box; margin: 0; padding: 0; }}\
    body {{ height: 100vh; background: #0d1117; color: #c9d1d9; font-family: \"SF Mono\", \"Fira Code\", \"Cascadia Code\", ui-monospace, monospace; font-size: 14px; line-height: 1.5; overflow: hidden; }}\
    main {{ height: 100vh; display: flex; flex-direction: column; }}\
    .hint {{ color: #8b949e; font-size: 13px; padding: 1.5rem 1rem 0; }}\
    .notice {{ padding: 0.5rem 0.8rem; margin: 0.5rem 1rem 0; font-size: 13px; }}\
    .notice-ok {{ color: #3fb950; background: rgba(63,185,80,0.08); border: 1px solid rgba(63,185,80,0.2); }}\
    .notice-error {{ color: #f85149; background: rgba(248,81,73,0.08); border: 1px solid rgba(248,81,73,0.2); }}\
    label {{ display: block; color: #8b949e; font-size: 12px; text-transform: uppercase; letter-spacing: 0.08em; }}\
    .field {{ display: grid; gap: 0.3rem; }}\
    input, select, textarea {{ width: 100%; background: #0d1117; color: #c9d1d9; border: 1px solid #30363d; padding: 0.5rem 0.6rem; font: inherit; font-size: 14px; }}\
    input:focus, select:focus, textarea:focus {{ outline: none; border-color: #3fb950; }}\
    select {{ cursor: pointer; -webkit-appearance: none; appearance: none; background-image: url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' fill='%238b949e'%3E%3Cpath d='M6 8L1 3h10z'/%3E%3C/svg%3E\"); background-repeat: no-repeat; background-position: right 0.6rem center; padding-right: 1.8rem; }}\
    select option {{ background: #161b22; color: #c9d1d9; }}\
    button {{ padding: 0.5rem 0.8rem; background: #3fb950; color: #0d1117; border: none; font: inherit; font-weight: 600; cursor: pointer; }}\
    button:hover {{ background: #2ea043; }}\
    .form-stack {{ display: grid; gap: 0.8rem; padding: 1rem; }}\
    .is-hidden {{ display: none !important; }}\
    .terminal {{ display: flex; flex-direction: column; flex: 1; min-height: 0; }}\
    .terminal-output {{ flex: 1; overflow-y: auto; padding: 1rem; }}\
    .terminal-output::-webkit-scrollbar {{ width: 6px; }}\
    .terminal-output::-webkit-scrollbar-track {{ background: transparent; }}\
    .terminal-output::-webkit-scrollbar-thumb {{ background: #30363d; border-radius: 3px; }}\
    .msg {{ padding: 0.3rem 0; white-space: pre-wrap; overflow-wrap: anywhere; }}\
    .msg-user .msg-prefix {{ color: #3fb950; font-weight: 600; }}\
    .msg-claw .msg-prefix {{ color: #d2a8ff; font-weight: 600; }}\
    .msg-empty {{ color: #484f58; font-style: italic; }}\
    .msg-system {{ color: #8b949e; font-size: 13px; padding: 0.2rem 0; }}\
    .terminal-input {{ border-top: 1px solid #21262d; padding: 0.6rem 1rem; background: #0d1117; }}\
    .input-line {{ display: flex; align-items: flex-start; gap: 0.5rem; }}\
    .input-prefix {{ color: #3fb950; font-weight: 600; padding-top: 0.15rem; flex-shrink: 0; }}\
    .input-line textarea {{ border: none; background: transparent; color: #c9d1d9; padding: 0; resize: none; min-height: 1.5em; overflow: hidden; }}\
    .input-line textarea:focus {{ outline: none; border-color: transparent; }}\
    .statusbar {{ display: flex; justify-content: space-between; padding: 0.25rem 1rem; background: #161b22; border-top: 1px solid #21262d; color: #484f58; font-size: 12px; }}\
    .settings-panel {{ position: fixed; inset: 0; background: #0d1117; z-index: 20; overflow-y: auto; }}\
    .settings-header {{ display: flex; justify-content: space-between; align-items: center; padding: 1rem; border-bottom: 1px solid #21262d; }}\
    .settings-header span {{ color: #8b949e; font-size: 13px; text-transform: uppercase; letter-spacing: 0.08em; }}\
    .settings-header button {{ background: transparent; color: #8b949e; font-size: 13px; }}\
    .settings-header button:hover {{ color: #c9d1d9; background: transparent; }}\
    .device-info {{ color: #8b949e; font-size: 12px; margin-top: 2rem; border-top: 1px solid #21262d; padding-top: 0.75rem; }}\
  </style>\
</head>\
<body>\
  <main>{body_html}</main>\
  <script type=\"application/json\" id=\"clawpi-provider-catalog\">{provider_catalog_json}</script>\
  <script>{ui_script}</script>\
</body>\
</html>",
        device_name = escape_html(device_name),
        body_html = body_html,
        provider_catalog_json = provider_catalog_json,
        ui_script = ui_script,
    )
}

fn render_ui_script() -> String {
    String::from(
        r#"(function () {
  var catalogNode = document.getElementById("clawpi-provider-catalog");
  if (!catalogNode) return;

  var presets = JSON.parse(catalogNode.textContent || "[]");
  var presetMap = {};
  presets.forEach(function (p) { presetMap[p.id] = p; });

  function toggle(el, hidden) {
    if (el) el.classList.toggle("is-hidden", hidden);
  }

  function initForm(form) {
    var formMode = form.dataset.formMode || "setup";
    var initialProvider = form.dataset.initialProvider || "";
    var initialModel = form.dataset.initialModel || "";
    var initialHasSecret = form.dataset.initialHasSecret === "true";

    var providerSelect = form.querySelector("[data-provider-select]");
    var authSelect = form.querySelector("[data-auth-select]");
    var modelSelect = form.querySelector("[data-model-select]");
    var authModeInput = form.querySelector('input[name="auth_mode"]');
    var apiKeyInput = form.querySelector('input[name="api_key"]');
    var credentialLabel = form.querySelector("[data-credential-label]");
    var authField = form.querySelector('[data-field="auth"]');
    var credentialField = form.querySelector('[data-field="credential"]');
    var modelField = form.querySelector('[data-field="model"]');

    function getPreset() {
      return presetMap[providerSelect.value] || null;
    }

    function updateAuth(preset) {
      if (!preset || preset.auth_options.length <= 1) {
        toggle(authField, true);
        if (preset && preset.auth_options.length === 1) {
          authModeInput.value = preset.auth_options[0].id;
          updateCredential(preset.auth_options[0]);
        } else {
          authModeInput.value = "";
          toggle(credentialField, true);
        }
        return;
      }
      authSelect.innerHTML = "";
      preset.auth_options.forEach(function (opt) {
        var o = document.createElement("option");
        o.value = opt.id;
        o.textContent = opt.label;
        authSelect.appendChild(o);
      });
      var defaultAuth = preset.default_auth;
      if (initialHasSecret && preset.auth_options.some(function (o) { return o.id === "api_key"; })) {
        defaultAuth = "api_key";
      }
      authSelect.value = defaultAuth;
      authModeInput.value = defaultAuth;
      toggle(authField, false);
      var selected = preset.auth_options.find(function (o) { return o.id === defaultAuth; }) || preset.auth_options[0];
      updateCredential(selected);
    }

    function updateCredential(authOption) {
      if (!authOption || !authOption.requires_secret) {
        toggle(credentialField, true);
        return;
      }
      if (credentialLabel) credentialLabel.textContent = authOption.secret_label || "API key";
      if (apiKeyInput) apiKeyInput.placeholder = authOption.secret_placeholder || "sk-...";
      toggle(credentialField, false);
    }

    function updateModels(preset) {
      if (!preset) {
        toggle(modelField, true);
        return;
      }
      modelSelect.innerHTML = "";
      preset.models.forEach(function (m) {
        var o = document.createElement("option");
        o.value = m.id;
        o.textContent = m.label;
        modelSelect.appendChild(o);
      });
      var chosen = preset.default_model;
      if (initialModel && initialProvider === preset.id) {
        if (preset.models.some(function (m) { return m.id === initialModel; })) {
          chosen = initialModel;
        }
      }
      modelSelect.value = chosen;
      toggle(modelField, false);
    }

    function onProviderChange() {
      var preset = getPreset();
      updateAuth(preset);
      updateModels(preset);
    }

    providerSelect.addEventListener("change", onProviderChange);

    if (authSelect) {
      authSelect.addEventListener("change", function () {
        var preset = getPreset();
        if (!preset) return;
        authModeInput.value = authSelect.value;
        var opt = preset.auth_options.find(function (o) { return o.id === authSelect.value; });
        updateCredential(opt);
      });
    }

    form.addEventListener("submit", function (e) {
      if (!providerSelect.value) {
        e.preventDefault();
        providerSelect.focus();
        return;
      }
      if (modelSelect && !modelSelect.value) {
        e.preventDefault();
        modelSelect.focus();
        return;
      }
      var preset = getPreset();
      var authId = authModeInput.value;
      var authOpt = preset && preset.auth_options.find(function (o) { return o.id === authId; });
      if (authOpt && authOpt.requires_secret && apiKeyInput && !apiKeyInput.value.trim()) {
        var canReuse = formMode === "update" && initialHasSecret && initialProvider === providerSelect.value;
        if (!canReuse) {
          e.preventDefault();
          apiKeyInput.focus();
          return;
        }
      }
    });

    if (providerSelect.value) {
      onProviderChange();
    }
  }

  document.querySelectorAll(".ai-config-form").forEach(initForm);

  // Auto-resize textarea to content
  var prompt = document.getElementById("prompt");
  if (prompt) {
    function autoResize() {
      prompt.style.height = "auto";
      prompt.style.height = prompt.scrollHeight + "px";
    }
    prompt.addEventListener("input", autoResize);
    autoResize();

    // Enter to submit, Shift+Enter for newline
    var form = document.getElementById("prompt-form");
    if (form) {
      prompt.addEventListener("keydown", function (e) {
        if (e.key === "Enter" && !e.shiftKey) {
          e.preventDefault();
          var text = prompt.value.trim();
          if (!text) return;

          // Slash commands handled client-side
          if (text === "/help") {
            e.preventDefault();
            var output = document.getElementById("output");
            if (output) {
              output.innerHTML += '<div class="msg msg-system">Commands: /settings /help</div>';
              output.scrollTop = output.scrollHeight;
            }
            prompt.value = "";
            autoResize();
            return;
          }
          if (text === "/settings") {
            e.preventDefault();
            var panel = document.getElementById("settings-panel");
            if (panel) panel.classList.remove("is-hidden");
            prompt.value = "";
            autoResize();
            return;
          }

          form.submit();
        }
      });
    }
  }

  // Scroll transcript to bottom on load
  var output = document.getElementById("output");
  if (output) {
    output.scrollTop = output.scrollHeight;
  }
})();"#,
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

        assert!(html.contains("Pick an AI provider"));
        assert!(html.contains("data-open-picker=\"provider\""));
        assert!(html.contains("name=\"provider_custom\""));
        assert!(html.contains(">Device</summary>"));
        assert!(!html.contains("ClawPi local console"));
        assert!(!html.contains("This browser handoff should stay narrow"));
    }

    #[test]
    fn render_home_page_shows_console_after_ai_configuration() {
        let layout = test_layout("console");
        let mut config = base_config();
        config.ai_provider = Some(String::from("openrouter"));
        config.ai_model = Some(String::from("anthropic/claude-sonnet-4.6"));
        config.ai_api_key = Some(String::from("sk-test-secret"));

        let html = render_home_page(&layout, &config, None, None, None, None, None).unwrap();

        assert!(html.contains("<summary>AI</summary>"));
        assert!(html.contains("Ask Claw anything."));
        assert!(html.contains("placeholder=\"Ask Claw anything.\""));
        assert!(!html.contains("AI settings and device details"));
    }

    #[test]
    fn render_home_page_allows_keyless_ai_configuration() {
        let layout = test_layout("keyless");
        let mut config = base_config();
        config.ai_provider = Some(String::from("ollama"));
        config.ai_model = Some(String::from("llama3.2"));

        let html = render_home_page(&layout, &config, None, None, None, None, None).unwrap();

        assert!(html.contains("<summary>AI</summary>"));
        assert!(html.contains("data-initial-provider=\"ollama\""));
    }

    #[test]
    fn render_ai_form_keeps_custom_provider_value() {
        let mut config = base_config();
        config.ai_provider = Some(String::from("acme/router"));

        let html = render_ai_form(&config, "custom-ai", "Save", "update", false);

        assert!(html.contains("data-initial-provider=\"acme/router\""));
        assert!(html.contains("name=\"provider_value\""));
    }

    #[test]
    fn resolve_provider_prefers_provider_value() {
        let fields = HashMap::from([(String::from("provider_value"), String::from("ollama"))]);

        assert_eq!(resolve_provider(&fields), "ollama");
    }

    #[test]
    fn resolve_provider_uses_custom_route_when_selected() {
        let fields = HashMap::from([
            (String::from("provider_preset"), String::from("custom")),
            (
                String::from("provider_custom"),
                String::from("gateway.example/provider"),
            ),
        ]);

        assert_eq!(resolve_provider(&fields), "gateway.example/provider");
    }

    #[test]
    fn resolve_provider_preserves_legacy_field_support() {
        let fields = HashMap::from([(String::from("provider"), String::from("openrouter"))]);

        assert_eq!(resolve_provider(&fields), "openrouter");
    }

    #[test]
    fn resolve_model_prefers_hidden_model_value() {
        let fields = HashMap::from([(String::from("model"), String::from("gpt-5.2"))]);

        assert_eq!(resolve_model(&fields).as_deref(), Some("gpt-5.2"));
    }

    #[test]
    fn resolve_ai_secret_clears_secret_for_local_auth() {
        let config = base_config();

        assert_eq!(
            resolve_ai_secret(&config, "ollama", AUTH_MODE_LOCAL, "").unwrap(),
            None
        );
    }

    #[test]
    fn resolve_ai_secret_reuses_existing_secret_on_update() {
        let mut config = base_config();
        config.ai_provider = Some(String::from("openrouter"));
        config.ai_model = Some(String::from("anthropic/claude-sonnet-4.6"));
        config.ai_api_key = Some(String::from("sk-test-secret"));

        assert_eq!(
            resolve_ai_secret(&config, "openrouter", AUTH_MODE_API_KEY, "")
                .unwrap()
                .as_deref(),
            Some("sk-test-secret")
        );
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
