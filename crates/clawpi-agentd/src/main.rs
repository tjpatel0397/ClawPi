use clawpi_core::{
    ai_configured, inspect_state, AgentPromptRequest, AgentPromptResponse, ClawPiConfig, Layout,
    Mode, DEFAULT_AI_MODEL, DEFAULT_AI_PROVIDER,
};
use serde_json::{json, Value};
use std::fs;
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

const OPENAI_RESPONSES_URL: &str = "https://api.openai.com/v1/responses";

fn main() -> ExitCode {
    let layout = Layout::detect();

    match run(&layout) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("clawpi-agentd: {err}");
            ExitCode::from(1)
        }
    }
}

fn run(layout: &Layout) -> io::Result<()> {
    layout.ensure_dirs()?;

    let state = inspect_state(layout)?;
    if state.mode != Mode::Normal {
        write_status_file(layout, "skipped", "mode-is-not-normal", None)?;
        return Ok(());
    }

    let config = state.config_status.as_config().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "normal mode requires a valid config",
        )
    })?;

    remove_existing_socket(&layout.agent_socket_path())?;
    let listener = UnixListener::bind(layout.agent_socket_path())?;
    write_status_file(layout, "serving", agent_note(config), Some(config))?;

    for stream in listener.incoming() {
        let state = inspect_state(layout)?;
        if state.mode != Mode::Normal {
            write_status_file(layout, "stopped", "mode-changed-from-normal", None)?;
            fs::remove_file(layout.agent_socket_path()).ok();
            return Ok(());
        }

        let config = state.config_status.as_config().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "normal mode requires a valid config",
            )
        })?;
        write_status_file(layout, "serving", agent_note(config), Some(config))?;

        match stream {
            Ok(mut stream) => handle_connection(layout, config, &mut stream)?,
            Err(err) => return Err(err),
        }
    }

    Ok(())
}

fn handle_connection(
    layout: &Layout,
    config: &ClawPiConfig,
    stream: &mut UnixStream,
) -> io::Result<()> {
    let request = match read_json_message::<AgentPromptRequest>(stream) {
        Ok(request) => request,
        Err(err) => {
            write_json_message(
                stream,
                &AgentPromptResponse::failure(format!("invalid prompt request: {err}")),
            )?;
            return Ok(());
        }
    };

    let prompt = request.prompt.trim();
    if prompt.is_empty() {
        write_json_message(
            stream,
            &AgentPromptResponse::failure("prompt must not be empty"),
        )?;
        return Ok(());
    }

    write_status_file(layout, "busy", "handling-prompt", Some(config))?;

    let response = match prompt_claw(config, prompt) {
        Ok(reply) => AgentPromptResponse::success(reply),
        Err(err) => AgentPromptResponse::failure(err.to_string()),
    };

    write_status_file(layout, "serving", agent_note(config), Some(config))?;
    write_json_message(stream, &response)
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
        "You are Claw, the local operating-system companion for a Raspberry Pi device named {}. Reply like an on-device agent daemon: concise, practical, and device-oriented. Avoid unnecessary formatting. If the user asks you to take actions that are not wired into this ClawPi proving-ground runtime yet, say so directly and suggest the next step.",
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

fn read_json_message<T>(stream: &mut UnixStream) -> io::Result<T>
where
    T: serde::de::DeserializeOwned,
{
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;

    let mut body = Vec::new();
    stream.read_to_end(&mut body)?;
    if body.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "request body was empty",
        ));
    }

    serde_json::from_slice(&body).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

fn write_json_message<T>(stream: &mut UnixStream, value: &T) -> io::Result<()>
where
    T: serde::Serialize,
{
    let body = serde_json::to_vec(value)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    stream.write_all(&body)?;
    stream.shutdown(Shutdown::Write)
}

fn agent_note(config: &ClawPiConfig) -> &'static str {
    if ai_configured(config) {
        "ready-for-prompts"
    } else {
        "awaiting-ai-setup"
    }
}

fn write_status_file(
    layout: &Layout,
    status: &str,
    note: &str,
    config: Option<&ClawPiConfig>,
) -> io::Result<()> {
    let ai_configured = config.map(ai_configured).unwrap_or(false);
    let ai_provider = config
        .and_then(|config| config.ai_provider.as_deref())
        .unwrap_or("unset");
    let ai_model = config
        .and_then(|config| config.ai_model.as_deref())
        .unwrap_or("unset");

    fs::write(
        layout.agent_status_path(),
        format!(
            "phase=4\nservice=clawpi-agentd\nstatus={status}\nsocket={}\nai_configured={ai_configured}\nai_provider={ai_provider}\nai_model={ai_model}\nnote={note}\n",
            layout.agent_socket_path().display()
        ),
    )
}

fn remove_existing_socket(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_response_text_prefers_output_text() {
        let value = json!({
            "output_text": "hello from claw"
        });

        assert_eq!(
            extract_response_text(&value).as_deref(),
            Some("hello from claw")
        );
    }

    #[test]
    fn extract_response_text_falls_back_to_output_content() {
        let value = json!({
            "output": [
                {
                    "content": [
                        { "text": "first" },
                        { "text": "second" }
                    ]
                }
            ]
        });

        assert_eq!(
            extract_response_text(&value).as_deref(),
            Some("first\n\nsecond")
        );
    }
}
