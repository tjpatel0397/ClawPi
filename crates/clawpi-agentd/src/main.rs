use clawpi_core::{
    ai_configured, inspect_state, AgentPromptRequest, AgentPromptResponse, ClawPiConfig, Layout,
    Mode, DEFAULT_AI_MODEL, DEFAULT_AI_PROVIDER,
};
use serde::de::DeserializeOwned;
use serde_json::json;
use std::fs;
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use tokio::runtime::{Builder, Runtime};
use zeroclaw::{agent, Config as ZeroClawConfig};

const ZEROCLAW_UPSTREAM_REV: &str = "3482771d6d681efe97a3517eb65912b8763ca2d5";
const ZEROCLAW_PROVIDER_TIMEOUT_SECS: u64 = 180;
const ZEROCLAW_SHELL_TIMEOUT_SECS: u64 = 60;
const ZEROCLAW_MAX_ACTIONS_PER_HOUR: u32 = 10_000;
const ZEROCLAW_MAX_COST_PER_DAY_CENTS: u32 = 100_000;
const ZEROCLAW_DEFAULT_TEMPERATURE: f64 = 0.2;

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

    let runtime = Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|err| io::Error::other(err.to_string()))?;

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
            Ok(mut stream) => handle_connection(layout, &runtime, config, &mut stream)?,
            Err(err) => return Err(err),
        }
    }

    Ok(())
}

fn handle_connection(
    layout: &Layout,
    runtime: &Runtime,
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

    let response = match prompt_claw(layout, runtime, config, prompt) {
        Ok(reply) => AgentPromptResponse::success(reply),
        Err(err) => AgentPromptResponse::failure(err.to_string()),
    };

    write_status_file(layout, "serving", agent_note(config), Some(config))?;
    write_json_message(stream, &response)
}

fn prompt_claw(
    layout: &Layout,
    runtime: &Runtime,
    config: &ClawPiConfig,
    prompt: &str,
) -> io::Result<String> {
    if !ai_configured(config) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "AI is not configured yet",
        ));
    }

    runtime.block_on(prompt_zeroclaw(layout, config, prompt))
}

async fn prompt_zeroclaw(
    layout: &Layout,
    config: &ClawPiConfig,
    prompt: &str,
) -> io::Result<String> {
    let zeroclaw_config = build_zeroclaw_config(layout, config)?;
    agent::process_message(zeroclaw_config, prompt, None)
        .await
        .map_err(|err| io::Error::other(err.to_string()))
}

fn build_zeroclaw_config(layout: &Layout, config: &ClawPiConfig) -> io::Result<ZeroClawConfig> {
    let api_key = config
        .ai_api_key
        .clone()
        .filter(|value| !value.trim().is_empty());
    let provider = config
        .ai_provider
        .clone()
        .unwrap_or_else(|| String::from(DEFAULT_AI_PROVIDER));
    let model = config
        .ai_model
        .clone()
        .unwrap_or_else(|| String::from(DEFAULT_AI_MODEL));

    let workspace_dir = ensure_zeroclaw_workspace(layout, config)?;
    let zeroclaw_root = workspace_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("invalid zeroclaw workspace path"))?;

    let mut zeroclaw_config: ZeroClawConfig = serde_json::from_value(json!({
        "api_key": api_key,
        "default_provider": provider,
        "default_model": model,
        "default_temperature": ZEROCLAW_DEFAULT_TEMPERATURE,
        "provider_timeout_secs": ZEROCLAW_PROVIDER_TIMEOUT_SECS,
        "runtime": {
            "kind": "native"
        },
        "autonomy": {
            "level": "full",
            "workspace_only": false,
            "allowed_commands": ["*"],
            "forbidden_paths": [],
            "max_actions_per_hour": ZEROCLAW_MAX_ACTIONS_PER_HOUR,
            "max_cost_per_day_cents": ZEROCLAW_MAX_COST_PER_DAY_CENTS,
            "require_approval_for_medium_risk": false,
            "block_high_risk_commands": false,
            "shell_timeout_secs": ZEROCLAW_SHELL_TIMEOUT_SECS
        },
        "shell_tool": {
            "timeout_secs": ZEROCLAW_SHELL_TIMEOUT_SECS
        }
    }))
    .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;

    zeroclaw_config.workspace_dir = workspace_dir;
    zeroclaw_config.config_path = zeroclaw_root.join("config.toml");

    Ok(zeroclaw_config)
}

fn ensure_zeroclaw_workspace(layout: &Layout, config: &ClawPiConfig) -> io::Result<PathBuf> {
    let zeroclaw_root = layout.state_dir().join("zeroclaw");
    let workspace_dir = zeroclaw_root.join("workspace");
    fs::create_dir_all(&workspace_dir)?;

    write_workspace_file(&workspace_dir.join("AGENTS.md"), &render_agents_md(config))?;
    write_workspace_file(
        &workspace_dir.join("IDENTITY.md"),
        &render_identity_md(config),
    )?;
    write_workspace_file(
        &workspace_dir.join("BOOTSTRAP.md"),
        &render_bootstrap_md(config),
    )?;

    Ok(workspace_dir)
}

fn write_workspace_file(path: &Path, content: &str) -> io::Result<()> {
    if let Ok(existing) = fs::read_to_string(path) {
        if existing == content {
            return Ok(());
        }
    }

    fs::write(path, content)
}

fn render_agents_md(config: &ClawPiConfig) -> String {
    format!(
        "# AGENTS\n\nMission\nBuild ClawPi as an agentic operating system for Raspberry Pi devices.\n\nRuntime Role\nYou are Claw, the embedded agent runtime for ClawPi on the device named {device_name}.\nYou are not a generic chatbot, dashboard narrator, or cloud control plane.\nAct like a built-in operating-system agent.\n\nOperating Rules\n- Prefer direct local inspection before answering device-state questions.\n- Use local tools and OS commands when they materially improve correctness.\n- Keep responses concise, practical, and action-oriented.\n- Treat clawpi.local as a thin UI over your runtime.\n- Assume this Raspberry Pi is the primary environment you should operate on.\n",
        device_name = config.device_name
    )
}

fn render_identity_md(config: &ClawPiConfig) -> String {
    format!(
        "# Identity\n\nName: Claw\nProduct: ClawPi\nDevice: {device_name}\nRole: Embedded AI runtime for a Raspberry Pi operating system.\nSurface: clawpi.local is one UI over this local runtime.\n",
        device_name = config.device_name
    )
}

fn render_bootstrap_md(config: &ClawPiConfig) -> String {
    format!(
        "# Bootstrap\n\nThis workspace belongs to the embedded ZeroClaw-backed runtime inside ClawPi.\nYou are running on-device on the Raspberry Pi named {device_name}.\nIt is acceptable to inspect services, logs, files, and operating-system state directly when needed.\nFavor direct action and verification over speculative answers.\n",
        device_name = config.device_name
    )
}

fn read_json_message<T>(stream: &mut UnixStream) -> io::Result<T>
where
    T: DeserializeOwned,
{
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;

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
    stream.set_write_timeout(Some(std::time::Duration::from_secs(5)))?;
    stream.write_all(&body)?;
    stream.shutdown(Shutdown::Write)
}

fn agent_note(config: &ClawPiConfig) -> &'static str {
    if ai_configured(config) {
        "ready-for-prompts-via-zeroclaw"
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
            "phase=4\nservice=clawpi-agentd\nstatus={status}\nengine=zeroclaw\nengine_rev={ZEROCLAW_UPSTREAM_REV}\nsocket={}\nai_configured={ai_configured}\nai_provider={ai_provider}\nai_model={ai_model}\nnote={note}\n",
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
    use clawpi_core::{ClawPiConfig, SetupState, DEFAULT_WIFI_COUNTRY, RUNTIME_PROFILE};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_config() -> ClawPiConfig {
        ClawPiConfig {
            config_version: 1,
            device_name: String::from("clawpi-test"),
            setup_state: SetupState::Complete,
            runtime_profile: String::from(RUNTIME_PROFILE),
            wifi_country: String::from(DEFAULT_WIFI_COUNTRY),
            wifi_ssid: None,
            wifi_passphrase: None,
            ai_provider: Some(String::from(DEFAULT_AI_PROVIDER)),
            ai_model: Some(String::from(DEFAULT_AI_MODEL)),
            ai_api_key: Some(String::from("sk-test")),
        }
    }

    fn temp_root() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("clawpi-agentd-test-{unique}"))
    }

    fn cleanup_root(root: PathBuf) {
        if let Err(err) = fs::remove_dir_all(root) {
            assert_eq!(err.kind(), io::ErrorKind::NotFound);
        }
    }

    #[test]
    fn build_zeroclaw_config_sets_unrestricted_runtime_profile() {
        let root = temp_root();
        let layout = Layout::from_root(&root);
        layout.ensure_dirs().unwrap();

        let zeroclaw_config = build_zeroclaw_config(&layout, &test_config()).unwrap();
        let serialized = serde_json::to_value(&zeroclaw_config).unwrap();

        assert_eq!(
            zeroclaw_config.workspace_dir,
            root.join("var/lib/clawpi/zeroclaw/workspace")
        );
        assert_eq!(
            zeroclaw_config.config_path,
            root.join("var/lib/clawpi/zeroclaw/config.toml")
        );
        assert_eq!(
            serialized["default_provider"],
            serde_json::Value::String(String::from("openrouter"))
        );
        assert_eq!(
            serialized["default_model"],
            serde_json::Value::String(String::from("anthropic/claude-sonnet-4.6"))
        );
        assert_eq!(
            serialized["autonomy"]["level"],
            serde_json::Value::String(String::from("full"))
        );
        assert_eq!(
            serialized["autonomy"]["workspace_only"],
            serde_json::Value::Bool(false)
        );
        assert_eq!(serialized["autonomy"]["allowed_commands"], json!(["*"]));
        assert_eq!(
            serialized["autonomy"]["block_high_risk_commands"],
            serde_json::Value::Bool(false)
        );

        cleanup_root(root);
    }

    #[test]
    fn ensure_zeroclaw_workspace_writes_clawpi_identity_files() {
        let root = temp_root();
        let layout = Layout::from_root(&root);
        layout.ensure_dirs().unwrap();
        let config = test_config();

        let workspace_dir = ensure_zeroclaw_workspace(&layout, &config).unwrap();
        let agents = fs::read_to_string(workspace_dir.join("AGENTS.md")).unwrap();
        let identity = fs::read_to_string(workspace_dir.join("IDENTITY.md")).unwrap();
        let bootstrap = fs::read_to_string(workspace_dir.join("BOOTSTRAP.md")).unwrap();

        assert!(agents.contains("You are Claw"));
        assert!(identity.contains("Product: ClawPi"));
        assert!(bootstrap.contains("ZeroClaw-backed runtime"));

        cleanup_root(root);
    }

    #[test]
    fn build_zeroclaw_config_preserves_non_openai_provider() {
        let root = temp_root();
        let layout = Layout::from_root(&root);
        layout.ensure_dirs().unwrap();

        let mut config = test_config();
        config.ai_provider = Some(String::from("openrouter"));
        config.ai_model = Some(String::from("anthropic/claude-sonnet-4.6"));

        let zeroclaw_config = build_zeroclaw_config(&layout, &config).unwrap();
        let serialized = serde_json::to_value(&zeroclaw_config).unwrap();

        assert_eq!(
            serialized["default_provider"],
            serde_json::Value::String(String::from("openrouter"))
        );
        assert_eq!(
            serialized["default_model"],
            serde_json::Value::String(String::from("anthropic/claude-sonnet-4.6"))
        );

        cleanup_root(root);
    }
}
