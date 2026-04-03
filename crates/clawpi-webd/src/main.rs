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

const AUTH_NO_KEY: UiAuthOption = UiAuthOption {
    id: AUTH_MODE_NO_KEY,
    label: "No key",
    secret_label: None,
    secret_placeholder: None,
    requires_secret: false,
};

const OPENROUTER_MODELS: &[UiModelOption] = &[
    UiModelOption {
        id: "anthropic/claude-sonnet-4.6",
        label: "Claude Sonnet 4.6",
    },
    UiModelOption {
        id: "openai/gpt-5.2",
        label: "GPT-5.2",
    },
    UiModelOption {
        id: "openai/gpt-5-mini",
        label: "GPT-5 mini",
    },
    UiModelOption {
        id: "google/gemini-3-pro-preview",
        label: "Gemini 3 Pro Preview",
    },
    UiModelOption {
        id: MODEL_CUSTOM_ID,
        label: "Custom model",
    },
];

const ANTHROPIC_MODELS: &[UiModelOption] = &[
    UiModelOption {
        id: "claude-sonnet-4-5-20250929",
        label: "Claude Sonnet 4.5",
    },
    UiModelOption {
        id: "claude-opus-4-6",
        label: "Claude Opus 4.6",
    },
    UiModelOption {
        id: "claude-haiku-4-5-20251001",
        label: "Claude Haiku 4.5",
    },
    UiModelOption {
        id: MODEL_CUSTOM_ID,
        label: "Custom model",
    },
];

const OPENAI_MODELS: &[UiModelOption] = &[
    UiModelOption {
        id: "gpt-5.2",
        label: "GPT-5.2",
    },
    UiModelOption {
        id: "gpt-5-mini",
        label: "GPT-5 mini",
    },
    UiModelOption {
        id: "gpt-5.2-codex",
        label: "GPT-5.2 Codex",
    },
    UiModelOption {
        id: MODEL_CUSTOM_ID,
        label: "Custom model",
    },
];

const OPENAI_CODEX_MODELS: &[UiModelOption] = &[
    UiModelOption {
        id: "gpt-5-codex",
        label: "GPT-5 Codex",
    },
    UiModelOption {
        id: "gpt-5.2-codex",
        label: "GPT-5.2 Codex",
    },
    UiModelOption {
        id: "o4-mini",
        label: "o4-mini",
    },
];

const GEMINI_MODELS: &[UiModelOption] = &[
    UiModelOption {
        id: "gemini-3-pro-preview",
        label: "Gemini 3 Pro Preview",
    },
    UiModelOption {
        id: "gemini-2.5-pro",
        label: "Gemini 2.5 Pro",
    },
    UiModelOption {
        id: "gemini-2.5-flash",
        label: "Gemini 2.5 Flash",
    },
    UiModelOption {
        id: MODEL_CUSTOM_ID,
        label: "Custom model",
    },
];

const GROQ_MODELS: &[UiModelOption] = &[
    UiModelOption {
        id: "llama-3.3-70b-versatile",
        label: "Llama 3.3 70B",
    },
    UiModelOption {
        id: "openai/gpt-oss-120b",
        label: "GPT-OSS 120B",
    },
    UiModelOption {
        id: "openai/gpt-oss-20b",
        label: "GPT-OSS 20B",
    },
    UiModelOption {
        id: MODEL_CUSTOM_ID,
        label: "Custom model",
    },
];

const OLLAMA_MODELS: &[UiModelOption] = &[
    UiModelOption {
        id: "llama3.2",
        label: "Llama 3.2",
    },
    UiModelOption {
        id: "qwen2.5-coder:7b",
        label: "Qwen 2.5 Coder 7B",
    },
    UiModelOption {
        id: "mistral",
        label: "Mistral",
    },
    UiModelOption {
        id: MODEL_CUSTOM_ID,
        label: "Custom model",
    },
];

const CUSTOM_MODELS: &[UiModelOption] = &[UiModelOption {
    id: MODEL_CUSTOM_ID,
    label: "Custom model",
}];

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
        default_model: "claude-sonnet-4-5-20250929",
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
        default_model: "gpt-5.2",
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
        default_model: "gpt-5-codex",
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
        default_model: "gemini-2.5-pro",
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
        default_model: "llama-3.3-70b-versatile",
        default_auth: AUTH_MODE_API_KEY,
        auth_options: &[AUTH_API_KEY],
        models: GROQ_MODELS,
    },
    UiProviderPreset {
        id: "ollama",
        label: "Ollama",
        hint: "Local on this device",
        route_editable: false,
        route_placeholder: "ollama",
        default_model: "llama3.2",
        default_auth: AUTH_MODE_LOCAL,
        auth_options: &[AUTH_LOCAL],
        models: OLLAMA_MODELS,
    },
    UiProviderPreset {
        id: "custom",
        label: "Custom",
        hint: "OpenAI-compatible route",
        route_editable: true,
        route_placeholder: "custom:https://your-endpoint/v1",
        default_model: "default",
        default_auth: AUTH_MODE_NO_KEY,
        auth_options: &[AUTH_NO_KEY, AUTH_API_KEY],
        models: CUSTOM_MODELS,
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
    let ai_form = render_ai_form(config, "setup-ai", "Continue", "setup", true);
    let device_menu = render_device_menu(config, wifi_ssid);

    format!(
        "<section class=\"page-shell\">\
           <header class=\"page-topbar\">\
             <div class=\"wordmark\">ClawPi</div>\
             {device_menu}\
           </header>\
           <div class=\"setup-wrap\">\
             {notice_html}\
             {error_html}\
             <section class=\"setup-card\">\
               <h1>Pick an AI provider</h1>\
               {ai_form}\
             </section>\
           </div>\
         </section>",
        device_menu = device_menu,
        notice_html = notice_html,
        error_html = error_html,
        ai_form = ai_form,
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
    let ai_form = render_ai_form(config, "console-ai", "Save", "update", false);
    let transcript_html = render_transcript(last_prompt, answer);
    let device_menu = render_device_menu(config, wifi_ssid);

    format!(
        "<section class=\"page-shell\">\
           <header class=\"page-topbar\">\
             <div class=\"wordmark\">ClawPi</div>\
             {device_menu}\
           </header>\
           <div class=\"console-wrap\">\
             {notice_html}\
             {error_html}\
             <section class=\"console-card\">\
               <div class=\"transcript\">{transcript_html}</div>\
               <form method=\"post\" action=\"/prompt\" class=\"composer\">\
                 <label class=\"visually-hidden\" for=\"prompt\">Message Claw</label>\
                 <textarea id=\"prompt\" name=\"prompt\" rows=\"4\" class=\"composer-box\" placeholder=\"Ask Claw anything.\" autofocus>{draft_prompt}</textarea>\
                 <div class=\"composer-row\">\
                   <button type=\"submit\">Send</button>\
                 </div>\
               </form>\
               <details class=\"settings-drawer\">\
                 <summary>AI</summary>\
                 {ai_form}\
               </details>\
             </section>\
           </div>\
         </section>",
        device_menu = device_menu,
        notice_html = notice_html,
        error_html = error_html,
        ai_form = ai_form,
        transcript_html = transcript_html,
        draft_prompt = escape_html(draft_prompt.unwrap_or("")),
    )
}

fn render_ai_form(
    config: &ClawPiConfig,
    form_id: &str,
    submit_label: &str,
    form_mode: &str,
    autofocus_provider: bool,
) -> String {
    let provider_label = config
        .ai_provider
        .as_deref()
        .and_then(find_provider_preset)
        .map(|preset| preset.label)
        .unwrap_or("Select provider");
    let model_label = config
        .ai_provider
        .as_deref()
        .zip(config.ai_model.as_deref())
        .and_then(|(provider, model)| model_label_for(provider, model))
        .unwrap_or("Select model");
    let provider_autofocus = if autofocus_provider { " autofocus" } else { "" };
    let provider_options = render_provider_picker_options();
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

    format!(
        "<form method=\"post\" action=\"/configure-ai\" class=\"ai-config-form\" data-form-mode=\"{form_mode}\" data-initial-provider=\"{initial_provider}\" data-initial-model=\"{initial_model}\" data-initial-has-secret=\"{initial_has_secret}\">\
           <input type=\"hidden\" name=\"provider_preset\" value=\"\">\
           <input type=\"hidden\" name=\"provider_value\" value=\"\">\
           <input type=\"hidden\" name=\"auth_mode\" value=\"\">\
           <input type=\"hidden\" name=\"model\" value=\"\">\
           <div class=\"flow-stack\">\
             <button type=\"button\" class=\"select-card\" data-open-picker=\"provider\"{provider_autofocus}>\
               <span class=\"field-kicker\">Provider</span>\
               <strong data-provider-label>{provider_label}</strong>\
             </button>\
             <div class=\"field-shell is-hidden\" data-field=\"route\">\
               <label for=\"{form_id}-provider-custom\">Provider route</label>\
               <input id=\"{form_id}-provider-custom\" name=\"provider_custom\" value=\"{initial_provider}\" placeholder=\"custom:https://your-endpoint/v1\" spellcheck=\"false\" autocapitalize=\"off\">\
             </div>\
             <div class=\"field-shell is-hidden\" data-field=\"auth\">\
               <div class=\"choice-strip\" data-auth-options></div>\
             </div>\
             <div class=\"field-shell is-hidden\" data-field=\"credential\">\
               <label for=\"{form_id}-api-key\" data-credential-label>API key</label>\
               <input id=\"{form_id}-api-key\" name=\"api_key\" type=\"password\" placeholder=\"{api_key_placeholder}\" autocomplete=\"off\" spellcheck=\"false\">\
             </div>\
             <button type=\"button\" class=\"select-card is-hidden\" data-field=\"model\" data-open-picker=\"model\">\
               <span class=\"field-kicker\">Model</span>\
               <strong data-model-label>{model_label}</strong>\
             </button>\
             <div class=\"field-shell is-hidden\" data-field=\"custom-model\">\
               <label for=\"{form_id}-model-custom\">Custom model</label>\
               <input id=\"{form_id}-model-custom\" name=\"model_custom\" value=\"{initial_model}\" placeholder=\"model-id\" spellcheck=\"false\" autocapitalize=\"off\">\
             </div>\
           </div>\
           <div class=\"picker-sheet is-hidden\" data-picker=\"provider\">\
             <div class=\"picker-panel\">\
               <div class=\"picker-head\">\
                 <span>Pick a provider</span>\
                 <button type=\"button\" class=\"picker-close\" data-close-picker aria-label=\"Close\">Close</button>\
               </div>\
               <input type=\"search\" class=\"picker-search\" data-picker-search placeholder=\"Search providers\">\
               <div class=\"picker-list\" data-picker-list>\
                 {provider_options}\
               </div>\
             </div>\
           </div>\
           <div class=\"picker-sheet is-hidden\" data-picker=\"model\">\
             <div class=\"picker-panel\">\
               <div class=\"picker-head\">\
                 <span>Pick a model</span>\
                 <button type=\"button\" class=\"picker-close\" data-close-picker aria-label=\"Close\">Close</button>\
               </div>\
               <input type=\"search\" class=\"picker-search\" data-model-search placeholder=\"Search models\">\
               <div class=\"picker-list\" data-model-options></div>\
             </div>\
           </div>\
           <button type=\"submit\">{submit_label}</button>\
         </form>",
        form_mode = escape_html(form_mode),
        initial_provider = escape_html(initial_provider),
        initial_model = escape_html(initial_model),
        initial_has_secret = if initial_has_secret { "true" } else { "false" },
        provider_autofocus = provider_autofocus,
        provider_label = escape_html(provider_label),
        form_id = escape_html(form_id),
        api_key_placeholder = escape_html(api_key_placeholder),
        model_label = escape_html(model_label),
        provider_options = provider_options,
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
               Ask Claw anything.\
             </article>",
        ),
    }
}

fn render_device_menu(config: &ClawPiConfig, wifi_ssid: &str) -> String {
    format!(
        "<details class=\"device-menu\">\
           <summary>Device</summary>\
           <div class=\"device-popover\">\
             <div class=\"device-row\"><span>Name</span><strong>{device_name}</strong></div>\
             <div class=\"device-row\"><span>Wi-Fi</span><strong>{wifi_ssid}</strong></div>\
           </div>\
         </details>",
        device_name = escape_html(&config.device_name),
        wifi_ssid = escape_html(wifi_ssid),
    )
}

fn render_provider_picker_options() -> String {
    PROVIDER_PRESETS
        .iter()
        .map(|preset| {
            format!(
                "<button type=\"button\" class=\"picker-option\" data-provider-id=\"{id}\" data-searchable=\"{searchable}\">\
                   <strong>{label}</strong>\
                 </button>",
                id = preset.id,
                searchable = escape_html(&format!("{} {}", preset.label, preset.hint)),
                label = escape_html(preset.label),
            )
        })
        .collect()
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
    fields
        .get("model")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
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

fn find_provider_preset(provider: &str) -> Option<&'static UiProviderPreset> {
    let normalized = provider.trim();
    PROVIDER_PRESETS
        .iter()
        .find(|preset| normalized.eq_ignore_ascii_case(preset.id))
}

fn model_label_for(provider: &str, model: &str) -> Option<&'static str> {
    let preset = find_provider_preset(provider)?;
    preset
        .models
        .iter()
        .find(|option| option.id == model)
        .map(|option| option.label)
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
    :root {{\
      color-scheme: light;\
      --bg-0: #f3ead7;\
      --bg-1: #dfe8db;\
      --ink: #17211c;\
      --muted: #536159;\
      --line: rgba(23, 33, 28, 0.11);\
      --surface: rgba(248, 244, 236, 0.74);\
      --surface-strong: rgba(255, 255, 255, 0.88);\
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
      max-width: 56rem;\
      margin: 0 auto;\
      padding: 1rem;\
      display: grid;\
    }}\
    h1 {{ margin: 0; font-size: clamp(2rem, 4vw, 2.8rem); line-height: 1; letter-spacing: -0.05em; }}\
    p {{ margin: 0; line-height: 1.5; }}\
    strong {{ font-weight: 700; }}\
    .page-shell {{ display: grid; gap: 1rem; align-content: start; }}\
    .page-topbar {{ display: flex; justify-content: space-between; align-items: start; gap: 1rem; }}\
    .wordmark {{ font-size: 1rem; font-weight: 700; letter-spacing: -0.03em; }}\
    .setup-wrap {{ display: grid; gap: 0.9rem; min-height: calc(100vh - 5rem); align-content: center; }}\
    .console-wrap {{ display: grid; gap: 0.9rem; }}\
    .setup-card, .console-card {{\
      border: 1px solid var(--line);\
      border-radius: 1.6rem;\
      background: var(--surface-strong);\
      box-shadow: 0 1.5rem 4rem rgba(15, 23, 18, 0.12);\
      padding: 1.1rem;\
      backdrop-filter: blur(18px);\
      animation: rise 0.32s ease;\
    }}\
    .setup-card {{ display: grid; gap: 1rem; max-width: 34rem; width: 100%; margin: 0 auto; }}\
    .console-card {{ display: grid; gap: 1rem; }}\
    label, .field-kicker {{\
      display: block;\
      font: 700 0.78rem/1 \"IBM Plex Mono\", \"SFMono-Regular\", Menlo, monospace;\
      text-transform: uppercase;\
      letter-spacing: 0.12em;\
      color: var(--muted);\
    }}\
    input, select, textarea {{\
      width: 100%;\
      border: 1px solid #c3cfc3;\
      border-radius: 1rem;\
      background: rgba(255, 255, 255, 0.92);\
      color: var(--ink);\
      padding: 0.95rem 1rem;\
      font: inherit;\
      box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.75);\
    }}\
    input:focus, select:focus, textarea:focus {{\
      outline: 2px solid rgba(31, 91, 66, 0.18);\
      outline-offset: 2px;\
      border-color: rgba(31, 91, 66, 0.35);\
    }}\
    textarea {{ resize: vertical; min-height: 6rem; }}\
    .flow-stack {{ display: grid; gap: 0.85rem; }}\
    .field-shell {{ display: grid; gap: 0.55rem; }}\
    .select-card {{\
      width: 100%;\
      display: flex;\
      flex-direction: column;\
      align-items: start;\
      gap: 0.45rem;\
      padding: 0.95rem 1rem;\
      border-radius: 1rem;\
      border: 1px solid #c3cfc3;\
      background: rgba(255, 255, 255, 0.92);\
      color: var(--ink);\
      box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.75);\
      text-align: left;\
    }}\
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
    .notice {{ padding: 0.95rem 1rem; border-radius: 1rem; border: 1px solid transparent; }}\
    .notice-ok {{ background: rgba(220, 243, 228, 0.95); color: #12482f; border-color: rgba(18, 72, 47, 0.12); }}\
    .notice-error {{ background: rgba(255, 230, 220, 0.96); color: #7a2a19; border-color: rgba(122, 42, 25, 0.12); }}\
    .choice-strip {{ display: flex; flex-wrap: wrap; gap: 0.55rem; }}\
    .choice-pill {{\
      background: rgba(23, 33, 28, 0.05);\
      color: var(--ink);\
      border: 1px solid rgba(23, 33, 28, 0.08);\
      padding: 0.78rem 0.95rem;\
    }}\
    .choice-pill.is-active {{ background: var(--accent); color: #fff; border-color: transparent; }}\
    .settings-drawer {{\
      border-radius: 1rem;\
      border: 1px solid rgba(23, 33, 28, 0.08);\
      overflow: hidden;\
      background: rgba(255, 255, 255, 0.45);\
    }}\
    .settings-drawer summary {{\
      list-style: none;\
      cursor: pointer;\
      padding: 0.95rem 1rem;\
      font-weight: 700;\
    }}\
    .settings-drawer summary::-webkit-details-marker {{ display: none; }}\
    .settings-drawer[open] {{ padding-bottom: 1rem; }}\
    .settings-drawer[open] summary {{ margin-bottom: 0.4rem; }}\
    .transcript {{\
      display: grid;\
      gap: 0.85rem;\
      min-height: 16rem;\
      padding: 0.95rem;\
      border-radius: 1.1rem;\
      border: 1px solid rgba(23, 33, 28, 0.08);\
      background: rgba(23, 33, 28, 0.045);\
    }}\
    .message {{\
      max-width: 44rem;\
      padding: 0.95rem 1rem;\
      border-radius: 1rem;\
      line-height: 1.6;\
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
    .composer {{ display: grid; gap: 0.8rem; }}\
    .composer-row {{ display: flex; justify-content: flex-end; gap: 1rem; align-items: center; }}\
    .device-menu {{ position: relative; }}\
    .device-menu summary {{\
      list-style: none;\
      cursor: pointer;\
      padding: 0.68rem 0.92rem;\
      border-radius: 999px;\
      background: rgba(255, 255, 255, 0.82);\
      border: 1px solid rgba(23, 33, 28, 0.08);\
      font: 700 0.78rem/1 \"IBM Plex Mono\", \"SFMono-Regular\", Menlo, monospace;\
      color: var(--muted);\
    }}\
    .device-menu summary::-webkit-details-marker {{ display: none; }}\
    .device-popover {{\
      position: absolute;\
      top: calc(100% + 0.6rem);\
      right: 0;\
      min-width: 14rem;\
      display: grid;\
      gap: 0.55rem;\
      padding: 0.8rem;\
      border-radius: 1rem;\
      background: rgba(255, 255, 255, 0.96);\
      border: 1px solid rgba(23, 33, 28, 0.08);\
      box-shadow: 0 1rem 2.6rem rgba(15, 23, 18, 0.16);\
    }}\
    .device-row {{ display: flex; justify-content: space-between; gap: 1rem; align-items: center; }}\
    .device-row span {{ color: var(--muted); }}\
    .device-row strong {{ font: 600 0.9rem/1.4 \"IBM Plex Mono\", \"SFMono-Regular\", Menlo, monospace; text-align: right; }}\
    .picker-sheet {{\
      position: fixed;\
      inset: 0;\
      display: grid;\
      place-items: center;\
      padding: 1rem;\
      background: rgba(23, 33, 28, 0.2);\
      z-index: 10;\
    }}\
    .picker-panel {{\
      width: min(32rem, 100%);\
      max-height: min(70vh, 36rem);\
      display: grid;\
      gap: 0.8rem;\
      padding: 1rem;\
      border-radius: 1.2rem;\
      background: rgba(255, 255, 255, 0.98);\
      border: 1px solid rgba(23, 33, 28, 0.08);\
      box-shadow: 0 1.5rem 4rem rgba(15, 23, 18, 0.16);\
    }}\
    .picker-head {{ display: flex; justify-content: space-between; gap: 1rem; align-items: center; font-weight: 700; }}\
    .picker-close {{\
      background: rgba(23, 33, 28, 0.06);\
      color: var(--ink);\
      border: 1px solid rgba(23, 33, 28, 0.08);\
      padding: 0.65rem 0.95rem;\
    }}\
    .picker-list {{ display: grid; gap: 0.55rem; overflow: auto; padding-right: 0.2rem; }}\
    .picker-option {{\
      width: 100%;\
      display: grid;\
      gap: 0.22rem;\
      justify-items: start;\
      padding: 0.92rem 1rem;\
      border-radius: 1rem;\
      background: rgba(23, 33, 28, 0.04);\
      color: var(--ink);\
      border: 1px solid rgba(23, 33, 28, 0.08);\
      text-align: left;\
    }}\
    .picker-option span {{ color: var(--muted); font-size: 0.94rem; }}\
    .is-hidden {{ display: none !important; }}\
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
    @media (max-width: 760px) {{\
      main {{ padding: 0.75rem; }}\
      .page-topbar {{ align-items: center; }}\
      .setup-card, .console-card {{ padding: 0.95rem; border-radius: 1.25rem; }}\
      .device-popover {{ position: fixed; top: 4.2rem; right: 0.75rem; left: 0.75rem; min-width: 0; }}\
      button {{ width: 100%; }}\
      .message {{ max-width: none; }}\
    }}\
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
  const catalogNode = document.getElementById("clawpi-provider-catalog");
  if (!catalogNode) {
    return;
  }

  const MODEL_CUSTOM_ID = "__custom__";
  const AUTH_MODE_API_KEY = "api_key";
  const presets = JSON.parse(catalogNode.textContent || "[]");
  const presetMap = Object.fromEntries(presets.map((preset) => [preset.id, preset]));

  function text(node, value) {
    if (node) {
      node.textContent = value;
    }
  }

  function toggle(node, hidden) {
    if (node) {
      node.classList.toggle("is-hidden", hidden);
    }
  }

  function filterPicker(container, query) {
    const needle = query.trim().toLowerCase();
    container.querySelectorAll("[data-searchable]").forEach((item) => {
      const haystack = (item.dataset.searchable || "").toLowerCase();
      item.classList.toggle("is-hidden", needle && !haystack.includes(needle));
    });
  }

  function presetForProvider(providerValue) {
    return presetMap[providerValue] || (providerValue ? presetMap.custom : null);
  }

  function modelLabelFor(preset, modelId) {
    if (!preset || !modelId) {
      return "Select model";
    }

    const match = preset.models.find((option) => option.id === modelId);
    return match ? match.label : "Custom model";
  }

  function defaultAuthFor(preset, hasSecret) {
    if (!preset) {
      return AUTH_MODE_API_KEY;
    }

    if (preset.id === "custom") {
      return hasSecret ? AUTH_MODE_API_KEY : preset.default_auth;
    }

    if (preset.id === "gemini" && !hasSecret) {
      return "device_login";
    }

    return hasSecret ? AUTH_MODE_API_KEY : preset.default_auth;
  }

  function initForm(form) {
    const formMode = form.dataset.formMode || "setup";
    const initialProviderValue = form.dataset.initialProvider || "";
    const initialModelValue = form.dataset.initialModel || "";
    const initialHasSecret = form.dataset.initialHasSecret === "true";

    const providerPresetInput = form.querySelector('input[name="provider_preset"]');
    const providerValueInput = form.querySelector('input[name="provider_value"]');
    const authModeInput = form.querySelector('input[name="auth_mode"]');
    const modelInput = form.querySelector('input[name="model"]');
    const providerCustomInput = form.querySelector('input[name="provider_custom"]');
    const modelCustomInput = form.querySelector('input[name="model_custom"]');
    const apiKeyInput = form.querySelector('input[name="api_key"]');

    const providerButton = form.querySelector('[data-open-picker="provider"]');
    const modelButton = form.querySelector('[data-open-picker="model"]');
    const providerLabel = form.querySelector("[data-provider-label]");
    const modelLabel = form.querySelector("[data-model-label]");
    const routeField = form.querySelector('[data-field="route"]');
    const authField = form.querySelector('[data-field="auth"]');
    const authOptionsRoot = form.querySelector("[data-auth-options]");
    const credentialField = form.querySelector('[data-field="credential"]');
    const credentialLabel = form.querySelector("[data-credential-label]");
    const customModelField = form.querySelector('[data-field="custom-model"]');
    const submitButton = form.querySelector('button[type="submit"]');
    const modelOptionsRoot = form.querySelector("[data-model-options]");

    const providerSheet = form.querySelector('[data-picker="provider"]');
    const modelSheet = form.querySelector('[data-picker="model"]');
    const providerSearch = form.querySelector("[data-picker-search]");
    const modelSearch = form.querySelector("[data-model-search]");

    let state = {
      preset: null,
      providerValue: "",
      authMode: "",
      modelValue: "",
      customModel: false,
    };

    function openSheet(sheet, searchInput) {
      if (!sheet) return;
      toggle(sheet, false);
      if (searchInput) {
        searchInput.value = "";
        searchInput.focus();
      }
    }

    function closeSheet(sheet) {
      toggle(sheet, true);
    }

    function renderAuthOptions(preset) {
      if (!preset || preset.auth_options.length <= 1) {
        authOptionsRoot.innerHTML = "";
        toggle(authField, true);
        return;
      }

      authOptionsRoot.innerHTML = preset.auth_options
        .map((option) => {
          const activeClass = option.id === state.authMode ? " is-active" : "";
          return (
            '<button type="button" class="choice-pill' +
            activeClass +
            '" data-auth-option="' +
            option.id +
            '">' +
            option.label +
            "</button>"
          );
        })
        .join("");

      authOptionsRoot.querySelectorAll("[data-auth-option]").forEach((button) => {
        button.addEventListener("click", () => {
          setAuthMode(button.dataset.authOption || "");
        });
      });

      toggle(authField, false);
    }

    function renderModelOptions(preset) {
      if (!preset) {
        modelOptionsRoot.innerHTML = "";
        return;
      }

      modelOptionsRoot.innerHTML = preset.models
        .map((option) => {
          const searchable = (option.label + " " + option.id).toLowerCase();
          return (
            '<button type="button" class="picker-option" data-model-id="' +
            option.id +
            '" data-searchable="' +
            searchable +
            '"><strong>' +
            option.label +
            "</strong><span>" +
            (option.id === MODEL_CUSTOM_ID ? "Type a model ID" : option.id) +
            "</span></button>"
          );
        })
        .join("");

      modelOptionsRoot.querySelectorAll("[data-model-id]").forEach((button) => {
        button.addEventListener("click", () => {
          setModel(button.dataset.modelId || "");
          closeSheet(modelSheet);
        });
      });
    }

    function syncProviderValue() {
      if (!state.preset) {
        providerValueInput.value = "";
        return;
      }

      if (state.preset.route_editable) {
        state.providerValue = providerCustomInput.value.trim();
      } else {
        state.providerValue = state.preset.id;
      }
      providerValueInput.value = state.providerValue;
    }

    function syncModelValue() {
      if (state.customModel) {
        state.modelValue = modelCustomInput.value.trim();
      }
      modelInput.value = state.modelValue;
    }

    function setAuthMode(modeId) {
      if (!state.preset) {
        return;
      }

      const authOption =
        state.preset.auth_options.find((option) => option.id === modeId) ||
        state.preset.auth_options[0];

      state.authMode = authOption.id;
      authModeInput.value = authOption.id;
      renderAuthOptions(state.preset);

      if (authOption.requires_secret) {
        text(credentialLabel, authOption.secret_label || "API key");
        apiKeyInput.placeholder = authOption.secret_placeholder || "sk-...";
        toggle(credentialField, false);
      } else {
        toggle(credentialField, true);
      }
    }

    function setModel(modelId) {
      if (!state.preset) {
        return;
      }

      const nextModel = modelId || state.preset.default_model;
      state.customModel = nextModel === MODEL_CUSTOM_ID;

      if (state.customModel) {
        text(modelLabel, "Custom model");
        toggle(customModelField, false);
        state.modelValue = modelCustomInput.value.trim();
      } else {
        state.modelValue = nextModel;
        text(modelLabel, modelLabelFor(state.preset, nextModel));
        toggle(customModelField, true);
      }

      syncModelValue();
      toggle(modelButton, false);
    }

    function setProvider(providerId, initialSelection) {
      const preset = presetMap[providerId];
      if (!preset) {
        return;
      }

      state.preset = preset;
      providerPresetInput.value = preset.id;
      text(providerLabel, preset.label);

      providerCustomInput.readOnly = !preset.route_editable;
      providerCustomInput.value = preset.route_editable
        ? initialSelection || providerCustomInput.value
        : preset.id;
      toggle(routeField, preset.route_editable);
      syncProviderValue();

      state.authMode = defaultAuthFor(preset, initialHasSecret);
      renderAuthOptions(preset);
      setAuthMode(state.authMode);
      renderModelOptions(preset);

      const selectedModel =
        initialModelValue && presetForProvider(initialProviderValue)?.id === preset.id
          ? initialModelValue
          : preset.default_model;

      if (preset.models.some((option) => option.id === selectedModel)) {
        setModel(selectedModel);
      } else if (initialModelValue && presetForProvider(initialProviderValue)?.id === preset.id) {
        modelCustomInput.value = initialModelValue;
        setModel(MODEL_CUSTOM_ID);
      } else {
        modelCustomInput.value = "";
        setModel(selectedModel);
      }
    }

    providerCustomInput.addEventListener("input", syncProviderValue);
    modelCustomInput.addEventListener("input", syncModelValue);

    providerSearch?.addEventListener("input", () => {
      filterPicker(providerSheet, providerSearch.value);
    });

    modelSearch?.addEventListener("input", () => {
      filterPicker(modelSheet, modelSearch.value);
    });

    form.querySelectorAll("[data-close-picker]").forEach((button) => {
      button.addEventListener("click", () => {
        closeSheet(providerSheet);
        closeSheet(modelSheet);
      });
    });

    providerButton?.addEventListener("click", () => {
      openSheet(providerSheet, providerSearch);
    });

    modelButton?.addEventListener("click", () => {
      openSheet(modelSheet, modelSearch);
    });

    providerSheet?.addEventListener("click", (event) => {
      if (event.target === providerSheet) {
        closeSheet(providerSheet);
      }
    });

    modelSheet?.addEventListener("click", (event) => {
      if (event.target === modelSheet) {
        closeSheet(modelSheet);
      }
    });

    form.querySelectorAll("[data-provider-id]").forEach((button) => {
      button.addEventListener("click", () => {
        const providerId = button.dataset.providerId || "";
        setProvider(providerId, providerId === "custom" ? initialProviderValue : providerId);
        closeSheet(providerSheet);
      });
    });

    form.addEventListener("submit", (event) => {
      syncProviderValue();
      syncModelValue();

      if (!providerValueInput.value.trim()) {
        event.preventDefault();
        openSheet(providerSheet, providerSearch);
        return;
      }

      if (!modelInput.value.trim()) {
        event.preventDefault();
        openSheet(modelSheet, modelSearch);
        return;
      }

      const authOption =
        state.preset?.auth_options.find((option) => option.id === state.authMode) || null;
      if (authOption && authOption.requires_secret && !apiKeyInput.value.trim()) {
        const canReuse =
          formMode === "update" &&
          initialHasSecret &&
          initialProviderValue === providerValueInput.value.trim();
        if (!canReuse) {
          event.preventDefault();
          apiKeyInput.focus();
        }
      }
    });

    if (initialProviderValue) {
      const initialPreset = presetForProvider(initialProviderValue);
      if (initialPreset) {
        setProvider(
          initialPreset.id,
          initialPreset.id === "custom" ? initialProviderValue : initialPreset.id
        );
      }
    }
  }

  document.querySelectorAll(".ai-config-form").forEach(initForm);
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
