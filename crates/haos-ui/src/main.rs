mod gateway_ws;

use axum::{
    Router,
    extract::State,
    response::{Html, IntoResponse, Sse, sse::Event},
    routing::{get, post},
    Json,
};
use futures_util::{StreamExt as _, stream};
use std::{env, fs, net::SocketAddr, path::PathBuf, process::Command, sync::Arc, time::Duration};
use tokio::{
    net::TcpStream,
    sync::{RwLock, broadcast},
    time::timeout,
};

use gateway_ws::PendingPair;

/// Snapshot cached by the background updater task every 30 seconds.
#[derive(Clone)]
struct CachedSnapshot {
    snapshot: SystemSnapshot,
    health_ok: Option<bool>,
    pending_devices: usize,
}

/// 待配对设备列表（通过 WebSocket 轮询，每 30s 更新一次）。
#[derive(Clone)]
struct PairingState {
    pairs: Arc<RwLock<Vec<PendingPair>>>,
    /// 每当列表变化时广播，SSE 连接订阅此 channel。
    notify: broadcast::Sender<()>,
}

impl PairingState {
    fn new() -> Self {
        let (notify, _) = broadcast::channel(4);
        Self {
            pairs: Arc::new(RwLock::new(vec![])),
            notify,
        }
    }
}

#[derive(Clone)]
struct AppState {
    cache: Arc<RwLock<Option<CachedSnapshot>>>,
    pairing: PairingState,
}

#[derive(Clone, Debug)]
struct PageConfig {
    addon_version: String,
    access_mode: String,
    gateway_mode: String,
    gateway_url: String,
    openclaw_version: String,
    https_port: String,
    openclaw_config_path: String,
    openclaw_state_dir: String,
    openclaw_workspace_dir: String,
    openclaw_runtime_dir: String,
    mcporter_home_dir: String,
    mcporter_config_path: String,
    backup_dir: String,
    cert_dir: String,
    mcp_status: String,
    web_status: String,
    memory_status: String,
    web_provider: String,
    web_enabled: bool,
    web_base_url: String,
    web_model: String,
    web_api_configured: bool,
    memory_provider: String,
    memory_enabled: bool,
    memory_model: String,
    memory_fallback: String,
    memory_base_url: String,
    memory_local_model_path: String,
    memory_api_configured: bool,
    current_model: String,
    mcp_endpoint_count: usize,
    gateway_token: String,
}

#[derive(Clone, Debug)]
struct SystemSnapshot {
    cpu_load: String,
    memory_used: String,
    disk_used: String,
    uptime: String,
    openclaw_uptime: String,
    cpu_percent: u8,
    memory_percent: u8,
    disk_percent: u8,
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum NavPage {
    Home,
    Config,
    Commands,
    Logs,
}

impl PageConfig {
    fn from_env() -> Self {
        let config = load_runtime_config();
        let (web_provider, web_enabled, web_base_url, web_model, web_api_configured) =
            load_web_search_settings(config.as_ref());
        let (
            memory_provider,
            memory_enabled,
            memory_model,
            memory_fallback,
            memory_base_url,
            memory_local_model_path,
            memory_api_configured,
        ) = load_memory_search_settings(config.as_ref());
        let web_status = provider_status_from_config(
            config.as_ref(),
            &["tools.web.search.provider", "tools.webSearch.provider"],
            &env_value("WEB_SEARCH_PROVIDER", "disabled"),
        );
        let memory_status = provider_status_from_config(
            config.as_ref(),
            &[
                "agents.defaults.memorySearch.provider",
                "agents.defaults.memory.search.provider",
            ],
            &env_value("MEMORY_SEARCH_PROVIDER", "disabled"),
        );
        let current_model = config
            .as_ref()
            .and_then(|v| first_string_path(v, &[
                "agents.defaults.model.primary", // 实际格式：model 是对象，primary 是模型名
                "agents.defaults.llm.model",     // 备用路径
                "agents.defaults.model",         // 旧版本字符串兜底
            ]))
            .unwrap_or_else(|| "未配置".to_string());
        let mcp_endpoint_count = count_mcp_endpoints();
        let gateway_token = config
            .as_ref()
            .and_then(|v| string_path(v, "gateway.auth.token"))
            .unwrap_or_default();
        Self {
            addon_version: env_value("ADDON_VERSION", "unknown"),
            access_mode: env_value("ACCESS_MODE", "lan_https"),
            gateway_mode: env_value("GATEWAY_MODE", "local"),
            gateway_url: env_value("GW_PUBLIC_URL", ""),
            openclaw_version: env_value("OPENCLAW_VERSION", "unknown"),
            https_port: env_value("HTTPS_PORT", "18789"),
            openclaw_config_path: env_path_value(
                "OPENCLAW_CONFIG_PATH",
                "/config/.openclaw/openclaw.json",
            ),
            openclaw_state_dir: env_path_value("OPENCLAW_STATE_DIR", "/config/.openclaw"),
            openclaw_workspace_dir: env_path_value(
                "OPENCLAW_WORKSPACE_DIR",
                "/config/.openclaw/workspace",
            ),
            openclaw_runtime_dir: env_path_value("OPENCLAW_RUNTIME_DIR", "/run/openclaw-rs"),
            mcporter_home_dir: env_path_value("MCPORTER_HOME_DIR", "/config/.mcporter"),
            mcporter_config_path: env_path_value(
                "MCPORTER_CONFIG",
                "/config/.mcporter/mcporter.json",
            ),
            backup_dir: env_path_value("BACKUP_DIR", "/share/openclaw-backup/latest"),
            cert_dir: env_path_value("CERT_DIR", "/config/certs"),
            mcp_status: env_value("MCP_STATUS", "disabled"),
            web_status,
            memory_status,
            web_provider,
            web_enabled,
            web_base_url,
            web_model,
            web_api_configured,
            memory_provider,
            memory_enabled,
            memory_model,
            memory_fallback,
            memory_base_url,
            memory_local_model_path,
            memory_api_configured,
            current_model,
            mcp_endpoint_count,
            gateway_token,
        }
    }
}

fn load_json_file(path: PathBuf) -> Option<serde_json::Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
}

fn runtime_config_path() -> PathBuf {
    env::var("OPENCLAW_CONFIG_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/config/.openclaw/openclaw.json"))
}

fn panel_config_path() -> PathBuf {
    env::var("HAOS_PANEL_CONFIG_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/config/.openclaw/addon-panel.json"))
}

fn merge_json_overlay(base: &mut serde_json::Value, overlay: &serde_json::Value) {
    match (base, overlay) {
        (serde_json::Value::Object(base_obj), serde_json::Value::Object(overlay_obj)) => {
            for (key, value) in overlay_obj {
                if let Some(existing) = base_obj.get_mut(key) {
                    merge_json_overlay(existing, value);
                } else {
                    base_obj.insert(key.clone(), value.clone());
                }
            }
        }
        (base_slot, overlay_value) => {
            *base_slot = overlay_value.clone();
        }
    }
}

fn load_runtime_config() -> Option<serde_json::Value> {
    let mut effective = load_json_file(runtime_config_path()).unwrap_or_else(|| serde_json::json!({}));
    if let Some(overlay) = load_json_file(panel_config_path()) {
        merge_json_overlay(&mut effective, &overlay);
    }
    Some(effective)
}

fn load_panel_config_mutable() -> serde_json::Value {
    load_json_file(panel_config_path()).unwrap_or_else(|| serde_json::json!({}))
}

fn save_panel_config_value(config: &serde_json::Value) -> Result<(), String> {
    let path = panel_config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create config dir: {err}"))?;
    }
    let text =
        serde_json::to_string_pretty(config).map_err(|err| format!("serialize config: {err}"))?;
    fs::write(&path, format!("{text}\n")).map_err(|err| format!("write config: {err}"))
}

fn ensure_object(value: &mut serde_json::Value) -> &mut serde_json::Map<String, serde_json::Value> {
    if !value.is_object() {
        *value = serde_json::Value::Object(serde_json::Map::new());
    }
    value.as_object_mut().expect("object")
}

fn set_config_value_path(
    root: &mut serde_json::Value,
    path: &[&str],
    value: serde_json::Value,
) {
    if path.is_empty() {
        *root = value;
        return;
    }
    let mut current = root;
    for key in &path[..path.len().saturating_sub(1)] {
        let obj = ensure_object(current);
        current = obj
            .entry((*key).to_string())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    }
    ensure_object(current).insert(path[path.len() - 1].to_string(), value);
}

fn remove_config_value_path(root: &mut serde_json::Value, path: &[&str]) {
    fn inner(node: &mut serde_json::Value, path: &[&str]) -> bool {
        if path.is_empty() {
            return false;
        }
        let Some(obj) = node.as_object_mut() else {
            return false;
        };
        if path.len() == 1 {
            obj.remove(path[0]);
            return obj.is_empty();
        }
        if let Some(child) = obj.get_mut(path[0]) {
            if inner(child, &path[1..]) {
                obj.remove(path[0]);
            }
        }
        obj.is_empty()
    }

    let _ = inner(root, path);
}

fn set_or_remove_string_path(root: &mut serde_json::Value, path: &[&str], value: &str) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        remove_config_value_path(root, path);
    } else {
        set_config_value_path(root, path, serde_json::Value::String(trimmed.to_string()));
    }
}

fn set_bool_path(root: &mut serde_json::Value, path: &[&str], value: bool) {
    set_config_value_path(root, path, serde_json::Value::Bool(value));
}

fn parse_csv_values(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .map(|part| part.to_string())
        .collect()
}

fn first_string_path(config: &serde_json::Value, paths: &[&str]) -> Option<String> {
    paths.iter().find_map(|path| string_path(config, path))
}

fn provider_status_from_config(
    config: Option<&serde_json::Value>,
    paths: &[&str],
    fallback: &str,
) -> String {
    config
        .and_then(|value| first_string_path(value, paths))
        .unwrap_or_else(|| fallback.to_string())
}

fn bool_path(config: &serde_json::Value, path: &str) -> Option<bool> {
    let mut current = config;
    for part in path.split('.').filter(|part| !part.is_empty()) {
        current = current.get(part)?;
    }
    current.as_bool()
}

fn path_exists(config: &serde_json::Value, path: &str) -> bool {
    let mut current = config;
    for part in path.split('.').filter(|part| !part.is_empty()) {
        let Some(next) = current.get(part) else {
            return false;
        };
        current = next;
    }
    !current.is_null()
}

fn string_path(config: &serde_json::Value, path: &str) -> Option<String> {
    let mut current = config;
    for part in path.split('.').filter(|part| !part.is_empty()) {
        current = current.get(part)?;
    }
    current
        .as_str()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn web_provider_plugin(provider: &str) -> Option<&'static str> {
    match provider {
        "brave" => Some("brave"),
        "exa" => Some("exa"),
        "firecrawl" => Some("firecrawl"),
        "gemini" => Some("google"),
        "grok" => Some("xai"),
        "kimi" => Some("moonshot"),
        "minimax" => Some("minimax"),
        "ollama" => Some("ollama"),
        "perplexity" => Some("perplexity"),
        "searxng" => Some("searxng"),
        "tavily" => Some("tavily"),
        _ => None,
    }
}

fn web_provider_config_string(
    config: &serde_json::Value,
    provider: &str,
    field: &str,
) -> Option<String> {
    let plugin = web_provider_plugin(provider)?;
    string_path(
        config,
        &format!("plugins.entries.{plugin}.config.webSearch.{field}"),
    )
}

fn web_provider_api_configured(config: &serde_json::Value, provider: &str) -> bool {
    if provider == "duckduckgo" || provider == "ollama" {
        return false;
    }
    if provider == "searxng" {
        return path_exists(config, "plugins.entries.searxng.config.webSearch.baseUrl");
    }
    let Some(plugin) = web_provider_plugin(provider) else {
        return false;
    };
    path_exists(
        config,
        &format!("plugins.entries.{plugin}.config.webSearch.apiKey"),
    )
}

fn load_web_search_settings(config: Option<&serde_json::Value>) -> (String, bool, String, String, bool) {
    let provider = config
        .and_then(|value| string_path(value, "tools.web.search.provider"))
        .unwrap_or_else(|| "auto".to_string());
    let enabled = config
        .and_then(|value| bool_path(value, "tools.web.search.enabled"))
        .unwrap_or(true);
    let base_url = config
        .and_then(|value| web_provider_config_string(value, &provider, "baseUrl"))
        .unwrap_or_default();
    let model = config
        .and_then(|value| web_provider_config_string(value, &provider, "model"))
        .unwrap_or_default();
    let api_configured = config
        .map(|value| web_provider_api_configured(value, &provider))
        .unwrap_or(false);
    (provider, enabled, base_url, model, api_configured)
}

fn load_memory_search_settings(
    config: Option<&serde_json::Value>,
) -> (String, bool, String, String, String, String, bool) {
    let provider = config
        .and_then(|value| string_path(value, "agents.defaults.memorySearch.provider"))
        .unwrap_or_else(|| "auto".to_string());
    let enabled = config
        .and_then(|value| bool_path(value, "agents.defaults.memorySearch.enabled"))
        .unwrap_or(true);
    let model = config
        .and_then(|value| string_path(value, "agents.defaults.memorySearch.model"))
        .unwrap_or_default();
    let fallback = config
        .and_then(|value| string_path(value, "agents.defaults.memorySearch.fallback"))
        .unwrap_or_else(|| "none".to_string());
    let base_url = config
        .and_then(|value| string_path(value, "agents.defaults.memorySearch.remote.baseUrl"))
        .unwrap_or_default();
    let local_model_path = config
        .and_then(|value| string_path(value, "agents.defaults.memorySearch.local.modelPath"))
        .unwrap_or_default();
    let api_configured = config
        .map(|value| path_exists(value, "agents.defaults.memorySearch.remote.apiKey"))
        .unwrap_or(false);
    (provider, enabled, model, fallback, base_url, local_model_path, api_configured)
}

fn env_value(key: &str, fallback: &str) -> String {
    env::var(key).unwrap_or_else(|_| fallback.to_string())
}

fn env_path_value(key: &str, fallback: &str) -> String {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

fn load_mcporter_config() -> Option<serde_json::Value> {
    fs::read_to_string(env_path_value(
        "MCPORTER_CONFIG",
        "/config/.mcporter/mcporter.json",
    ))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
}

fn count_mcp_endpoints() -> usize {
    let Some(value) = load_mcporter_config() else {
        return 0;
    };
    // Try common array fields first
    for key in &["endpoints", "servers", "tools", "mcp_servers", "mcpServers"] {
        if let Some(arr) = value.get(key).and_then(|v| v.as_array()) {
            return arr.len();
        }
    }
    // Top-level array
    if let Some(arr) = value.as_array() {
        return arr.len();
    }
    // Top-level object keys as proxy
    if let Some(obj) = value.as_object() {
        return obj.len();
    }
    0
}

fn mcp_status_display(config: &PageConfig) -> String {
    if config.mcp_endpoint_count > 0 {
        format!("已注册 {} 个端点", config.mcp_endpoint_count)
    } else if config.mcp_status != "disabled" && !config.mcp_status.is_empty() {
        display_value(&config.mcp_status).to_string()
    } else {
        "disabled".to_string()
    }
}

async fn fetch_openclaw_health() -> Option<bool> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(1500))
        .build()
        .ok()?;
    let resp = client
        .get("http://127.0.0.1:48099/readyz")
        .send()
        .await
        .ok()?;
    Some(resp.status().is_success())
}

fn pid_value(name: &str) -> String {
    let path = format!("/run/openclaw-rs/{name}.pid");
    fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "-".to_string())
}

fn display_value(value: &str) -> &str {
    let value = value.trim();
    if value.is_empty() { "disabled" } else { value }
}

const WEB_SEARCH_DOCS_URL: &str = "https://docs.openclaw.ai/tools/web";
const MEMORY_SEARCH_DOCS_URL: &str = "https://docs.openclaw.ai/reference/memory-config";
const MODEL_CONFIG_DOCS_URL: &str = "https://docs.openclaw.ai/gateway/configuration-reference";

const WEB_PROVIDER_META: &[(&str, &str, &str, &str)] = &[
    ("auto", "Auto", WEB_SEARCH_DOCS_URL, ""),
    ("duckduckgo", "DuckDuckGo", WEB_SEARCH_DOCS_URL, "https://duckduckgo.com/"),
    ("brave", "Brave", WEB_SEARCH_DOCS_URL, "https://api.search.brave.com/"),
    ("exa", "Exa", WEB_SEARCH_DOCS_URL, "https://dashboard.exa.ai/"),
    ("firecrawl", "Firecrawl", WEB_SEARCH_DOCS_URL, "https://www.firecrawl.dev/app"),
    ("gemini", "Gemini", WEB_SEARCH_DOCS_URL, "https://aistudio.google.com/app/apikey"),
    ("grok", "Grok / xAI", WEB_SEARCH_DOCS_URL, "https://console.x.ai/"),
    ("kimi", "Kimi / Moonshot", WEB_SEARCH_DOCS_URL, "https://platform.moonshot.cn/console/api-keys"),
    ("minimax", "MiniMax", WEB_SEARCH_DOCS_URL, "https://platform.minimaxi.com/user-center/basic-information/interface-key"),
    ("ollama", "Ollama", WEB_SEARCH_DOCS_URL, "https://ollama.com/"),
    ("perplexity", "Perplexity", WEB_SEARCH_DOCS_URL, "https://www.perplexity.ai/settings/api"),
    ("searxng", "SearXNG", WEB_SEARCH_DOCS_URL, "https://docs.searxng.org/"),
    ("tavily", "Tavily", WEB_SEARCH_DOCS_URL, "https://app.tavily.com/home"),
];

const MEMORY_PROVIDER_META: &[(&str, &str, &str, &str)] = &[
    ("auto", "Auto", MEMORY_SEARCH_DOCS_URL, ""),
    ("openai", "OpenAI", MEMORY_SEARCH_DOCS_URL, "https://platform.openai.com/api-keys"),
    ("gemini", "Gemini", MEMORY_SEARCH_DOCS_URL, "https://aistudio.google.com/app/apikey"),
    ("voyage", "Voyage", MEMORY_SEARCH_DOCS_URL, "https://dash.voyageai.com/api-keys"),
    ("mistral", "Mistral", MEMORY_SEARCH_DOCS_URL, "https://console.mistral.ai/api-keys/"),
    ("bedrock", "AWS Bedrock", MEMORY_SEARCH_DOCS_URL, "https://console.aws.amazon.com/bedrock/"),
    ("ollama", "Ollama", MEMORY_SEARCH_DOCS_URL, "https://ollama.com/"),
    ("local", "Local Model", MEMORY_SEARCH_DOCS_URL, ""),
];

const COMMON_MODEL_OPTIONS: &[&str] = &[
    "openai-codex/gpt-5.4",
    "openai-codex/gpt-5.4-mini",
    "openai/gpt-5.4",
    "openai/gpt-5.4-mini",
    "google/gemini-2.5-pro",
    "google/gemini-2.5-flash",
    "anthropic/claude-opus-4.1",
    "anthropic/claude-sonnet-4.5",
    "openrouter/qwen/qwen-2.5-72b-instruct",
];

fn provider_meta<'a>(
    provider: &str,
    entries: &'a [(&'a str, &'a str, &'a str, &'a str)],
) -> (&'a str, &'a str, &'a str, &'a str) {
    entries
        .iter()
        .copied()
        .find(|(value, _, _, _)| *value == provider)
        .unwrap_or(entries[0])
}

fn select_option_tags(
    selected: &str,
    entries: &[(&str, &str, &str, &str)],
) -> String {
    entries
        .iter()
        .map(|(value, label, docs_url, console_url)| {
            let selected_attr = if *value == selected { " selected" } else { "" };
            format!(
                r#"<option value="{value}" data-docs="{docs}" data-console="{console}"{selected_attr}>{label}</option>"#,
                value = html_attr_escape(value),
                docs = html_attr_escape(docs_url),
                console = html_attr_escape(console_url),
                selected_attr = selected_attr,
                label = label,
            )
        })
        .collect::<Vec<_>>()
        .join("")
}

fn datalist_option_tags(values: &[&str]) -> String {
    values
        .iter()
        .map(|value| format!(r#"<option value="{}"></option>"#, html_attr_escape(value)))
        .collect::<Vec<_>>()
        .join("")
}

fn form_checkbox(id: &str, checked: bool, label: &str, help: &str) -> String {
    let checked_attr = if checked { " checked" } else { "" };
    format!(
        r#"<label class="toggle-row" for="{id}">
  <span class="toggle-copy">
    <span class="toggle-title">{label}</span>
    <span class="toggle-help">{help}</span>
  </span>
  <input type="checkbox" id="{id}"{checked_attr}>
</label>"#,
        id = id,
        checked_attr = checked_attr,
        label = label,
        help = help,
    )
}

fn text_or_placeholder(value: &str, placeholder: &str) -> String {
    if value.trim().is_empty() {
        placeholder.to_string()
    } else {
        value.to_string()
    }
}

fn js_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

fn openclaw_brand_svg(class_name: &str) -> String {
    format!(
        r##"<svg class="{class_name}" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg" role="img" aria-label="OpenClaw logo" preserveAspectRatio="xMidYMid meet">
  <rect x="8" y="8" width="80" height="80" rx="24" fill="#10284c"/>
  <rect x="14" y="14" width="68" height="68" rx="20" fill="#14355f"/>
  <path d="M30 34 19 27 18 41 29 42Z" fill="#60cbff"/>
  <path d="M66 34 77 27 78 41 67 42Z" fill="#60cbff"/>
  <path d="M31 49c0-10 6-18 17-21" stroke="#8be0ff" stroke-width="6" stroke-linecap="round"/>
  <path d="M65 49c0-10-6-18-17-21" stroke="#8be0ff" stroke-width="6" stroke-linecap="round"/>
  <path d="M34 61c7 8 21 8 28 0" stroke="#8be0ff" stroke-width="6" stroke-linecap="round"/>
  <circle cx="48" cy="49" r="10" fill="#eef7ff"/>
  <circle cx="44" cy="46" r="2.6" fill="#14355f"/>
  <circle cx="52" cy="46" r="2.6" fill="#14355f"/>
</svg>"##
    )
}

fn html_attr_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn read_first_line(path: &str) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .and_then(|value| value.lines().next().map(|line| line.trim().to_string()))
}

/// Reads /proc/meminfo once and returns (MemTotal KiB, MemAvailable KiB).
fn parse_meminfo_both() -> (Option<u64>, Option<u64>) {
    let Ok(contents) = fs::read_to_string("/proc/meminfo") else {
        return (None, None);
    };
    let mut total = None;
    let mut available = None;
    for line in contents.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            total = rest.split_whitespace().next().and_then(|v| v.parse::<u64>().ok());
        } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
            available = rest.split_whitespace().next().and_then(|v| v.parse::<u64>().ok());
        }
        if total.is_some() && available.is_some() {
            break;
        }
    }
    (total, available)
}

fn format_bytes_gib(bytes: u64) -> String {
    format!("{:.1} GiB", bytes as f64 / 1024.0 / 1024.0 / 1024.0)
}

fn format_duration(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    if days > 0 {
        format!("{days} 天 {hours} 小时 {minutes} 分")
    } else if hours > 0 {
        format!("{hours} 小时 {minutes} 分")
    } else {
        format!("{minutes} 分")
    }
}

fn process_uptime(pid: &str) -> Option<String> {
    if pid.trim().is_empty() || pid == "-" {
        return None;
    }
    let output = Command::new("ps")
        .args(["-p", pid, "-o", "etimes="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let seconds = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u64>()
        .ok()?;
    Some(format_duration(seconds))
}

/// Runs `df -B1 /config` once and returns (display string, percent).
fn disk_combined() -> (String, u8) {
    let Ok(output) = Command::new("df").args(["-B1", "/config"]).output() else {
        return ("不可用".to_string(), 0);
    };
    if !output.status.success() {
        return ("不可用".to_string(), 0);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let Some(line) = stdout.lines().nth(1) else {
        return ("不可用".to_string(), 0);
    };
    let parts: Vec<&str> = line.split_whitespace().collect();
    let total = parts.get(1).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    let used = parts.get(2).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    let pct_str = parts.get(4).copied().unwrap_or("0%");
    if total == 0 {
        return ("不可用".to_string(), 0);
    }
    let display = format!("{}/{} ({pct_str})", format_bytes_gib(used), format_bytes_gib(total));
    let percent = ((used.saturating_mul(100)) / total).min(100) as u8;
    (display, percent)
}

fn load_percent_snapshot() -> Option<u8> {
    let load_line = read_first_line("/proc/loadavg")?;
    let load = load_line.split_whitespace().next()?.parse::<f64>().ok()?;
    let cpus = std::thread::available_parallelism().ok()?.get() as f64;
    if cpus <= 0.0 {
        return None;
    }
    Some(((load / cpus) * 100.0).round().clamp(0.0, 100.0) as u8)
}

async fn collect_system_snapshot() -> SystemSnapshot {
    tokio::task::spawn_blocking(|| {
        let cpu_load = read_first_line("/proc/loadavg")
            .and_then(|line| line.split_whitespace().next().map(|v| format!("{v} / 1m")))
            .unwrap_or_else(|| "不可用".to_string());
        let cpu_percent = load_percent_snapshot().unwrap_or(0);

        let (mem_total, mem_available) = parse_meminfo_both();
        let memory_used = match (mem_total, mem_available) {
            (Some(total), Some(available)) if total > available => {
                format!(
                    "{}/{}",
                    format_bytes_gib((total - available) * 1024),
                    format_bytes_gib(total * 1024)
                )
            }
            _ => "不可用".to_string(),
        };
        let memory_percent = match (mem_total, mem_available) {
            (Some(total), Some(available)) if total > available && total > 0 => {
                (((total - available) * 100) / total).min(100) as u8
            }
            _ => 0,
        };

        let uptime = read_first_line("/proc/uptime")
            .and_then(|line| {
                line.split_whitespace()
                    .next()
                    .and_then(|v| v.parse::<f64>().ok())
                    .map(|v| format_duration(v as u64))
            })
            .unwrap_or_else(|| "不可用".to_string());

        let openclaw_uptime = process_uptime(&pid_value("openclaw-gateway"))
            .or_else(|| process_uptime(&pid_value("openclaw-node")))
            .unwrap_or_else(|| "不可用".to_string());

        let (disk_used, disk_percent) = disk_combined();

        SystemSnapshot {
            cpu_load,
            memory_used,
            disk_used,
            uptime,
            openclaw_uptime,
            cpu_percent,
            memory_percent,
            disk_percent,
        }
    })
    .await
    .unwrap_or_else(|_| SystemSnapshot {
        cpu_load: "不可用".to_string(),
        memory_used: "不可用".to_string(),
        disk_used: "不可用".to_string(),
        uptime: "不可用".to_string(),
        openclaw_uptime: "不可用".to_string(),
        cpu_percent: 0,
        memory_percent: 0,
        disk_percent: 0,
    })
}

fn nav_link(active: NavPage, current: NavPage, href: &str, label: &str) -> String {
    let class_name = if active == current {
        "nav-link active"
    } else {
        "nav-link"
    };
    format!(r#"<a class="{class_name}" href="{href}">{label}</a>"#)
}

fn action_button(label: &str, command: &str) -> String {
    // Restart / kill commands get a danger tint; everything else gets the action tint.
    let extra_class = if command.contains("restart") || command.contains("kill") {
        " btn-danger"
    } else {
        " btn-action"
    };
    format!(
        r#"<button class="btn{extra_class}" type="button" data-command="{}" onclick="ocRunButton(this)">{label}</button>"#,
        html_attr_escape(command),
    )
}

fn primary_button(label: &str, onclick: &str) -> String {
    format!(r#"<button class="btn primary" type="button" onclick="{onclick}">{label}</button>"#)
}

fn primary_link_button(label: &str, id: &str, onclick: &str) -> String {
    format!(
        r##"<a class="btn primary" id="{id}" href="#" target="_blank" rel="noopener noreferrer" onclick="{onclick}">{label}</a>"##
    )
}

fn secondary_button(label: &str, onclick: &str) -> String {
    format!(r#"<button class="btn secondary" type="button" onclick="{onclick}">{label}</button>"#)
}

fn ghost_button(label: &str, onclick: &str) -> String {
    format!(r#"<button class="btn" type="button" onclick="{onclick}">{label}</button>"#)
}

fn diag_button(label: &str, command: &str) -> String {
    format!(
        r#"<button class="btn btn-diag" type="button" data-command="{}" onclick="ocRunButton(this)">{label}</button>"#,
        html_attr_escape(command),
    )
}

fn sensitive_button(label: &str, command: &str, warning: &str) -> String {
    let cmd_js = html_attr_escape(&js_string(command));
    let warn_js = html_attr_escape(&js_string(warning));
    format!(
        r#"<button class="btn btn-action" type="button" onclick="ocRunSensitive({cmd_js},{warn_js})">{label}</button>"#
    )
}

fn terminal_window_button(label: &str, command: &str) -> String {
    let command = html_attr_escape(&js_string(command));
    format!(
        r#"<button class="btn" type="button" onclick="ocOpenTerminalWindow({})">{label}</button>"#,
        command
    )
}

fn secondary_terminal_window_button(label: &str, command: &str) -> String {
    let command = html_attr_escape(&js_string(command));
    format!(
        r#"<button class="btn secondary" type="button" onclick="ocOpenTerminalWindow({})">{label}</button>"#,
        command
    )
}

fn kv_row(label: &str, value: &str) -> String {
    format!(
        r#"<div class="kv-row"><span class="kv-label">{label}</span><span class="kv-value">{value}</span></div>"#
    )
}

fn stat_tile(label: &str, value: &str, sub: &str) -> String {
    format!(
        r#"<article class="stat-card"><div class="stat-label">{label}</div><div class="stat-value">{value}</div><div class="stat-sub">{sub}</div></article>"#
    )
}

fn summary_strip(title: &str, value: &str, sub: &str, tone: &str) -> String {
    format!(
        r#"<article class="summary-strip-card {tone}"><div class="summary-strip-title">{title}</div><div class="summary-strip-value">{value}</div><div class="summary-strip-sub">{sub}</div></article>"#
    )
}

fn progress_bar(percent: u8, tone: &str) -> String {
    format!(
        r#"<div class="progress-track"><span class="progress-fill {tone}" style="width:{percent}%"></span></div>"#
    )
}

fn resource_card(label: &str, value: &str, percent: u8, tone: &str, sub: &str) -> String {
    format!(
        r#"<article class="resource-card">
  <div class="resource-top">
    <span class="resource-label">{label}</span>
    <span class="resource-percent">{percent}%</span>
  </div>
  <div class="resource-value">{value}</div>
  {bar}
  <div class="resource-sub">{sub}</div>
</article>"#,
        bar = progress_bar(percent, tone),
    )
}

fn time_card(label: &str, value: &str, sub: &str) -> String {
    format!(
        r#"<article class="resource-card">
  <div class="resource-top">
    <span class="resource-label">{label}</span>
  </div>
  <div class="resource-value">{value}</div>
  <div class="resource-sub">{sub}</div>
</article>"#
    )
}

fn hero_action_link(label: &str, href: &str) -> String {
    format!(r#"<a class="btn" href="{href}">{label}</a>"#)
}

fn terminal_card(title: &str, subtitle: &str, button_label: &str) -> String {
    format!(
        r#"<section class="card terminal-card">
  <div class="card-head">
    <div>
      <div class="eyebrow">终端工作区</div>
      <h2>{title}</h2>
      <p class="muted">{subtitle}</p>
    </div>
  </div>
  <div class="terminal-shell">
    <div class="terminal-head">
      <strong>工作区终端</strong>
      <span>这里承载原生 OpenClaw TUI；在 TUI 里输入 <code>!命令</code> 可以执行本机 shell 命令。</span>
    </div>
    <div class="terminal-stage" id="terminalStage">
      <div class="terminal-placeholder">
        <div class="terminal-placeholder-inner">
          <h3>终端按需加载</h3>
          <p>默认不抢占首屏资源。点击上方按钮或任意命令按钮后，会自动连接终端。若打开的是 <code>openclaw tui</code>，普通输入就是 TUI 会话；本机 shell 用 <code>!pwd</code>、<code>!openclaw status</code> 这类前缀命令。</p>
          <button class="btn primary" type="button" onclick="ocLoadTerminal()">{button_label}</button>
        </div>
      </div>
    </div>
  </div>
</section>"#
    )
}

fn service_badge(label: &str, pid: &str) -> String {
    let (state_class, state_text, pid_text) = if pid != "-" {
        ("is-online", "在线", format!("PID {pid}"))
    } else {
        ("is-offline", "待启动", "未检测到 PID".to_string())
    };

    format!(
        r#"<article class="service-badge {state_class}">
  <div class="service-badge-top">
    <span class="service-name"><span class="svc-dot"></span>{label}</span>
    <span class="service-state">{state_text}</span>
  </div>
  <div class="service-meta">{pid_text}</div>
</article>"#
    )
}

fn pid_row(gateway_pid: &str, ingress_pid: &str, ui_pid: &str) -> String {
    format!(
        r#"<div class="service-grid">
  {gateway}
  {ingress}
  {ui}
</div>"#,
        gateway = service_badge("Gateway", gateway_pid),
        ingress = service_badge("Ingress", ingress_pid),
        ui = service_badge("UI", ui_pid),
    )
}

fn home_content(
    config: &PageConfig,
    snapshot: &SystemSnapshot,
    health_ok: Option<bool>,
    pending_devices: usize,
) -> String {
    let gateway_pid = pid_value("openclaw-gateway");
    let ingress_pid = pid_value("ingressd");
    let ui_pid = pid_value("haos-ui");
    let online_count = [gateway_pid.as_str(), ingress_pid.as_str(), ui_pid.as_str()]
    .into_iter()
    .filter(|value| *value != "-")
    .count();

    // Health check result takes priority over PID count when available
    let (health_text, health_sub, health_tone) = match health_ok {
        Some(true) => ("运行正常", "服务健康检查通过", "tone-good"),
        Some(false) => ("响应异常", "健康检查未通过，请查看日志", "tone-danger"),
        None => {
            if online_count >= 3 {
                ("运行正常", "关键进程全部在线", "tone-good")
            } else if online_count >= 2 {
                ("部分在线", "建议查看命令行和日志页", "tone-warn")
            } else {
                ("待检查", "关键进程数量不足", "tone-danger")
            }
        }
    };
    let live_row_class = match health_tone {
        "tone-good" => "is-good",
        "tone-warn" => "is-warn",
        _ => "is-danger",
    };

    format!(
        r#"<div class="page-grid">
  <section class="card hero-card">
    <div class="card-head">
      <div>
        <div class="eyebrow">首页</div>
        <h2>运行状态总览</h2>
        <p class="muted">查看 OpenClaw 当前是否正常运行、各服务进程状态，以及系统资源占用情况。</p>
      </div>
      <div class="header-actions">
        {open_gateway}
        {open_cli}
        {goto_commands}
      </div>
    </div>

    <div class="status-panel">
      <section class="panel-left">
        <div class="live-row {live_row_class}">
          <span class="live-dot"></span>
          <div class="live-copy">
            <strong>{health_text}</strong>
            <p class="muted">{health_sub}</p>
          </div>
        </div>

        <div class="summary-strip status-summary-strip">
          {runtime}
          {https}
          {openclaw_runtime}
        </div>

        <div class="stats-grid status-stats-grid">
          {stat_access}
          {stat_mode}
          {stat_addon}
          {stat_openclaw}
          {stat_model}
        </div>

        {token_section}
        
        {device_notice}
        <div class="note-box">有新设备需要配对时，上方会自动出现通知。也可前往命令行页手动执行 <code>openclaw devices approve --latest</code>。<br>若设备连接时提示 token 错误或被拒绝，请在该设备浏览器中清除此站点的 Cookies 与本地存储，然后重新打开页面。</div>
      </section>

      <aside class="panel-right">
        <div class="panel-title-row">
          <div>
            <div class="eyebrow">进程面板</div>
            <h3>服务与 PID</h3>
          </div>
          {health}
        </div>
        {pid_row}
      </aside>
    </div>
  </section>

  <section class="card">
    <div class="card-head compact">
      <div>
        <div class="eyebrow">资源监控</div>
        <h2>系统资源概览</h2>
      </div>
    </div>
    <div class="resource-grid">
      {resource_cpu}
      {resource_memory}
      {resource_disk}
      {resource_uptime}
      {resource_openclaw_uptime}
    </div>
  </section>
</div>"#,
        health = summary_strip("总状态", health_text, health_sub, health_tone),
        runtime = summary_strip(
            "在线进程",
            &format!("{online_count}/4"),
            "Gateway、Ingress、UI、Action",
            "tone-blue"
        ),
        https = summary_strip(
            "HTTPS 入口",
            &format!(":{}", config.https_port),
            "原生网关默认监听端口",
            "tone-teal"
        ),
        openclaw_runtime = summary_strip(
            "OpenClaw 时长",
            &snapshot.openclaw_uptime,
            "基于 Gateway 或 Node 主进程存活时间",
            "tone-violet"
        ),
        open_gateway = primary_link_button(
            "打开网关",
            "ocGatewayLink",
            "return ocOpenGatewayLink(event, this)"
        ),
        open_cli = secondary_terminal_window_button("OpenClaw CLI", "openclaw tui"),
        goto_commands = hero_action_link("进入命令行", "./commands"),
        stat_access = stat_tile("访问模式", &config.access_mode, "当前插件的访问入口模式"),
        stat_mode = stat_tile(
            "网关模式",
            &config.gateway_mode,
            "当前 OpenClaw 网关运行模式"
        ),
        stat_addon = stat_tile("Add-on 版本", &config.addon_version, "插件发布版本"),
        stat_openclaw = stat_tile("OpenClaw 版本", &config.openclaw_version, "上游运行时版本"),
        stat_model = stat_tile("AI 模型", &config.current_model, "当前 OpenClaw 使用的对话模型"),
        token_section = {
            let tok = &config.gateway_token;
            if tok.is_empty() {
                String::new()
            } else {
                let masked = format!("••••••••{}", &tok[tok.len().saturating_sub(8)..]);
                let tok_escaped = tok.replace('\\', "\\\\").replace('"', "\\\"");
                format!(r#"<div class="token-section">
  <div class="token-header">
    <span class="token-label">Gateway Token</span>
    <span class="token-hint">用于直接访问 OpenClaw API，请勿泄露</span>
  </div>
  <div class="token-row">
    <code class="token-val" id="ocTokenVal">{masked}</code>
    <button class="btn" id="ocTokenToggleBtn" onclick="ocToggleToken()">显示</button>
    <button class="btn btn-action" onclick="ocCopyToken(this)">复制</button>
  </div>
  <script>(function(){{var t="{tok_escaped}";window.ocToggleToken=function(){{var v=document.getElementById("ocTokenVal"),b=document.getElementById("ocTokenToggleBtn");if(b.dataset.vis==="1"){{v.textContent="••••••••"+t.slice(-8);b.textContent="显示";b.dataset.vis="";}}else{{v.textContent=t;b.textContent="隐藏";b.dataset.vis="1";}}}}; window.ocCopyToken=function(btn){{var orig=btn.textContent;function done(){{btn.textContent="已复制 ✓";setTimeout(function(){{btn.textContent=orig;}},1500);}}function fb(){{try{{var ta=document.createElement("textarea");ta.value=t;ta.style.cssText="position:fixed;opacity:0;top:0;left:0;width:1px;height:1px";document.body.appendChild(ta);ta.focus();ta.select();var ok=document.execCommand("copy");document.body.removeChild(ta);if(ok){{done();}}else{{alert("Token: "+t);}}}}catch(e){{alert("Token: "+t);}}}}if(navigator.clipboard){{navigator.clipboard.writeText(t).then(done,fb);}}else{{fb();}}}};}})()</script>
</div>"#, masked=masked, tok_escaped=tok_escaped)
            }
        },
        device_notice = if pending_devices > 0 {
            format!(
                r#"<div class="notice-badge warn">有 {pending_devices} 个设备等待授权配对，请前往命令行页执行 <code>openclaw devices approve --latest</code>。</div>"#
            )
        } else {
            String::new()
        },
        pid_row = pid_row(&gateway_pid, &ingress_pid, &ui_pid),
        resource_cpu = resource_card(
            "CPU 负载",
            &snapshot.cpu_load,
            snapshot.cpu_percent,
            "tone-blue",
            "按 CPU 核心数换算成近似百分比，仅用于首页概览。"
        ),
        resource_memory = resource_card(
            "内存占用",
            &snapshot.memory_used,
            snapshot.memory_percent,
            "tone-teal",
            "适合快速判断容器或宿主机是否有内存压力。"
        ),
        resource_disk = resource_card(
            "磁盘占用",
            &snapshot.disk_used,
            snapshot.disk_percent,
            "tone-gold",
            "重点观察 /config 所在卷，避免状态目录被写满。"
        ),
        resource_uptime = time_card(
            "系统运行时长",
            &snapshot.uptime,
            "适合判断是否刚重启、是否发生过异常恢复。"
        ),
        resource_openclaw_uptime = time_card(
            "OpenClaw 运行时长",
            &snapshot.openclaw_uptime,
            "适合判断 Gateway 是否刚重启，或是否发生过短暂掉线。"
        ),
        live_row_class = live_row_class,
        health_text = health_text,
        health_sub = health_sub,
    )
}

#[allow(dead_code)]
#[allow(dead_code)]
fn config_content(config: &PageConfig) -> String {
    format!(
        r#"<div class="page-grid">
  <section class="card">
    <div class="card-head">
      <div>
        <div class="eyebrow">基础配置</div>
        <h2>配置边界与当前状态</h2>
        <p class="muted">查看插件当前的访问入口、数据目录、服务能力状态，以及 MCP 和设备配对等集成配置。</p>
      </div>
      <div class="header-actions">
        <button class="btn secondary" type="button" onclick="ocOpenTerminalWindow()">新窗口打开终端</button>
        <a class="btn primary" href="./commands">进入命令行</a>
      </div>
    </div>
    <div class="split-grid">
      <section class="soft-card">
        <h3>插件配置页负责</h3>
        <ul class="clean-list">
          <li>访问模式、端口、终端开关、备份目录等插件级参数。</li>
          <li>与 Home Assistant 集成相关的初始化，例如 MCP 和自动配对。</li>
          <li>Brave Search 网页搜索、Gemini 记忆搜索等能力的接入与激活。</li>
        </ul>
      </section>
      <section class="soft-card">
        <h3>OpenClaw 自身负责</h3>
        <ul class="clean-list">
          <li><code>openclaw onboard</code> 登录、默认模型、provider 细节。</li>
          <li>更细粒度的 agent、memory、tool 和 provider 调整。</li>
          <li>需要精确诊断时，建议直接去命令行页执行官方命令。</li>
        </ul>
      </section>
    </div>
  </section>

  <div class="three-up">
    <section class="card">
      <h3>访问与网关</h3>
      <div class="kv-list">
        {access}
        {mode}
        {https}
        {openclaw}
      </div>
    </section>
    <section class="card">
      <h3>持久化目录</h3>
      <div class="kv-list">
        {oc_dir}
        {mcp_dir}
        {backup_dir}
        {cert_dir}
      </div>
    </section>
    <section class="card">
      <h3>能力状态</h3>
      <div class="kv-list">
        {mcp}
        {web}
        {memory}
        {version}
      </div>
    </section>
  </div>

  <section class="card">
    <h3>建议操作</h3>
    <div class="action-row">
      <button class="btn" type="button" onclick="ocOpenGateway()">打开网关</button>
      <button class="btn" type="button" onclick="ocOpenTerminalWindow('openclaw tui')">OpenClaw CLI</button>
      <button class="btn" type="button" onclick="ocOpenTerminalWindow()">新窗口打开终端</button>
      <button class="btn" type="button" onclick="ocRunCommand('openclaw onboard')">初始化向导</button>
      <button class="btn" type="button" onclick="ocRunCommand('openclaw doctor')">运行 doctor</button>
      <button class="btn" type="button" onclick="ocRunCommand('cat /config/.mcporter/mcporter.json')">查看 MCP 配置</button>
    </div>
  </section>
</div>"#,
        access = kv_row("访问模式", &config.access_mode),
        mode = kv_row("网关模式", &config.gateway_mode),
        https = kv_row("HTTPS 端口", &config.https_port),
        openclaw = kv_row("OpenClaw 版本", &config.openclaw_version),
        oc_dir = kv_row("OpenClaw", "/config/.openclaw"),
        mcp_dir = kv_row("MCPorter", "/config/.mcporter"),
        backup_dir = kv_row("备份目录", "/share/openclaw-backup/latest"),
        cert_dir = kv_row("证书目录", "/config/certs"),
        mcp = kv_row("MCP", &mcp_status_display(config)),
        web = kv_row("Web Search", display_value(&config.web_status)),
        memory = kv_row("Memory Search", display_value(&config.memory_status)),
        version = kv_row("Add-on 版本", &config.addon_version),
    )
}

#[allow(dead_code)]
#[allow(dead_code)]
fn commands_content() -> String {
    let setup_actions = [
        ("OpenClaw CLI", "openclaw tui"),
        ("设备列表", "openclaw devices list"),
        ("批准最新配对", "openclaw devices approve --latest"),
        ("初始化向导", "openclaw onboard"),
        ("检查并修复", "openclaw doctor --fix"),
    ]
    .iter()
    .map(|(label, cmd)| {
        if *cmd == "openclaw tui" {
            terminal_window_button(label, cmd)
        } else {
            action_button(label, cmd)
        }
    })
    .collect::<Vec<_>>()
    .join("");

    let diagnostic_actions = [
        ("健康检查", "openclaw health --json"),
        ("运行状态", "openclaw status --deep"),
        ("安全审计", "openclaw security audit --deep"),
        ("记忆状态", "openclaw memory status --deep"),
        ("本机版本", "openclaw --version"),
        (
            "重启网关",
            "curl -fsS -X POST http://127.0.0.1:48099/action/restart",
        ),
    ]
    .iter()
    .map(|(label, cmd)| {
        if cmd.contains("restart") {
            action_button(label, cmd)
        } else {
            diag_button(label, cmd)
        }
    })
    .collect::<Vec<_>>()
    .join("");

    let log_stream_actions = [
        ("跟随日志", "openclaw logs --follow"),
        ("网关日志", "tail -f /tmp/openclaw/openclaw-$(date +%F).log"),
    ]
    .iter()
    .map(|(label, cmd)| terminal_window_button(label, cmd))
    .collect::<Vec<_>>()
    .join("");

    let storage_actions = [
        ("MCP 列表", "mcporter list"),
        ("MCP 配置", "cat /config/.mcporter/mcporter.json"),
        ("备份目录", "ls -la /share/openclaw-backup/latest"),
        (
            "立即备份",
            "set -e; echo '▶ 创建目录…'; mkdir -p /share/openclaw-backup/latest/.openclaw /share/openclaw-backup/latest/.mcporter; echo '▶ 备份 .openclaw…'; rsync -a --delete /config/.openclaw/ /share/openclaw-backup/latest/.openclaw/; echo '▶ 备份 .mcporter…'; rsync -a --delete /config/.mcporter/ /share/openclaw-backup/latest/.mcporter/; echo '✓ 备份完成'",
        ),
    ]
    .iter()
    .map(|(label, cmd)| action_button(label, cmd))
    .collect::<Vec<_>>()
    .join("");

    let token_action = sensitive_button(
        "读取令牌",
        "jq -r '.gateway.auth.token' /config/.openclaw/openclaw.json",
        "此命令会把 auth token 明文输出到终端，请确认当前环境安全后再执行。",
    );

    format!(
        r#"<div class="page-grid">
  <section class="card">
    <div class="card-head">
      <div>
        <div class="eyebrow">命令行</div>
        <h2>命令工作区</h2>
        <p class="muted">这里集中放高频维护动作。按钮显示中文，实际执行的仍然是英文 OpenClaw 命令，便于和官方文档、日志保持一致。</p>
      </div>
      <div class="header-actions">
        {load_terminal}
        {close_terminal}
        {open_window}
        <a class="btn" href="./openclaw-ca.crt" target="_blank" rel="noopener noreferrer">下载 CA 证书</a>
      </div>
    </div>

    <div class="command-section">
      <div class="section-label">配对与初始化</div>
      <div class="action-row">{setup_actions}</div>
    </div>

    <div class="command-section">
      <div class="section-label">诊断与维护</div>
      <div class="action-row">{diagnostic_actions}</div>
    </div>

    <div class="command-section">
      <div class="section-label">日志跟踪（新窗口）</div>
      <div class="action-row">{log_stream_actions}</div>
    </div>

    <div class="command-section">
      <div class="section-label">MCP 与备份</div>
      <div class="action-row">{token_action}{storage_actions}</div>
    </div>

    <div class="command-section">
      <div class="section-label">自定义命令</div>
      <div class="custom-cmd-row">
        <input type="text" class="cmd-input" id="customCmdInput"
               placeholder="输入一次性 shell 命令；如果你想要官方交互式 CLI，请优先点上面的 OpenClaw CLI"
               autocomplete="off" spellcheck="false">
        <button class="btn btn-action" type="button" onclick="ocRunCustomCommand()">运行</button>
      </div>
    </div>
  </section>
  {terminal}
  <script>
    (function() {{
      var inp = document.getElementById("customCmdInput");
      if (inp) inp.addEventListener("keydown", function(e) {{ if (e.key === "Enter") ocRunCustomCommand(); }});
    }})();
  </script>
</div>"#,
        load_terminal = primary_button("加载终端", "ocLoadTerminal()"),
        close_terminal = ghost_button("关闭终端", "ocCloseTerminal()"),
        open_window = secondary_button("新窗口打开终端", "ocOpenTerminalWindow()"),
        setup_actions = setup_actions,
        diagnostic_actions = diagnostic_actions,
        log_stream_actions = log_stream_actions,
        token_action = token_action,
        storage_actions = storage_actions,
        terminal = terminal_card(
            "嵌入式终端",
            "这个区域更适合一次性命令和日志查看；如果你需要官方交互式体验，请使用上面的 OpenClaw CLI 打开原生 TUI。",
            "加载终端",
        ),
    )
}

fn config_content_v2(config: &PageConfig) -> String {
    let openclaw_config_cmd = format!("cat {}", config.openclaw_config_path);
    let panel_config_cmd = format!("cat {}", panel_config_path().display());
    let mcporter_config_cmd = format!("cat {}", config.mcporter_config_path);

    let (web_label, web_docs, web_console, _) =
        provider_meta(&config.web_provider, WEB_PROVIDER_META);
    let (memory_label, memory_docs, memory_console, _) =
        provider_meta(&config.memory_provider, MEMORY_PROVIDER_META);

    let web_api_state = if config.web_api_configured {
        "已配置 API / 凭证"
    } else {
        "未配置 API / 凭证"
    };
    let memory_api_state = if config.memory_api_configured {
        "已配置 API / 凭证"
    } else {
        "未配置 API / 凭证"
    };

    let web_provider_options = select_option_tags(&config.web_provider, WEB_PROVIDER_META);
    let memory_provider_options = select_option_tags(&config.memory_provider, MEMORY_PROVIDER_META);
    let model_datalist = datalist_option_tags(COMMON_MODEL_OPTIONS);
    let web_model_display = text_or_placeholder(&config.web_model, "未单独设置");
    let memory_model_display = text_or_placeholder(&config.memory_model, "未单独设置");

    format!(
        r#"<div class="page-grid">
  <section class="card">
    <div class="card-head">
      <div>
        <div class="eyebrow">配置中心</div>
        <h2>Web Search、Memory Search 与模型选择</h2>
        <p class="muted">这一页按官方文档提供可编辑配置，但不会在后台悄悄改 <code>openclaw.json</code>。只有你点击保存时，才会把配置写入独立的 Add-on 配置文件；重启插件后再合并应用，稳定性更高。</p>
      </div>
      <div class="header-actions">
        <button class="btn" type="button" onclick="ocOpenGateway()">打开网关</button>
        <button class="btn secondary" type="button" onclick="ocOpenTerminalWindow('openclaw tui')">OpenClaw CLI</button>
        <a class="btn primary" href="./commands">进入命令页</a>
      </div>
    </div>
    <div class="notice-badge warn">
      保存后会写入 <code>{panel_config_path}</code>。要真正应用到 OpenClaw，请重启插件；这样不配置的时候不会改动 <code>openclaw.json</code>。
    </div>
  </section>

  <div class="three-up">
    <section class="card">
      <h3>当前状态</h3>
      <div class="kv-list">
        {access}
        {mode}
        {https}
        {model}
      </div>
    </section>
    <section class="card">
      <h3>Web Search 概览</h3>
      <div class="kv-list">
        {web_status}
        {web_provider}
        {web_model}
        {web_api}
      </div>
    </section>
    <section class="card">
      <h3>Memory Search 概览</h3>
      <div class="kv-list">
        {memory_status}
        {memory_provider}
        {memory_model}
        {memory_api}
      </div>
    </section>
  </div>

  <section class="card">
    <div class="card-head">
      <div>
        <div class="eyebrow">Web Search</div>
        <h3>官方 Provider 与网页登录入口</h3>
        <p class="muted">支持官方文档里的常见 provider。选择 provider 后，右侧会给出官方文档和登录 / 控制台入口，方便直接去申请或验证凭证。</p>
      </div>
      <div class="header-actions">
        <a class="btn" id="webDocsLink" href="{web_docs}" target="_blank" rel="noopener noreferrer">官方文档</a>
        <a class="btn secondary" id="webConsoleLink" href="{web_console}" target="_blank" rel="noopener noreferrer"{web_console_style}>登录 / 控制台</a>
      </div>
    </div>
    <div class="config-form">
      {web_enabled_toggle}
      <div class="form-grid">
        <label class="field">
          <span class="field-label">Provider</span>
          <select id="webProvider" data-kind="web" onchange="ocSyncProviderMeta('web')">{web_provider_options}</select>
        </label>
        <label class="field">
          <span class="field-label">当前 Provider 状态</span>
          <input type="text" value="{web_label} / {web_api_state}" readonly>
        </label>
        <label class="field">
          <span class="field-label">Base URL</span>
          <input type="text" id="webBaseUrl" value="{web_base_url}" placeholder="留空使用 provider 默认地址">
        </label>
        <label class="field">
          <span class="field-label">Provider Model</span>
          <input type="text" id="webModel" value="{web_model_value}" placeholder="例如 sonar-pro / grok-3-search">
        </label>
        <label class="field field-span-2">
          <span class="field-label">API Key / Token</span>
          <input type="password" id="webApiKey" value="" placeholder="留空表示保持现有值；如需网页登录，请点右上角登录 / 控制台">
          <span class="field-help">不会回显当前密钥。只有填写新值时才会覆盖。</span>
        </label>
        <label class="field-inline">
          <input type="checkbox" id="webClearApiKey">
          <span>清除当前 Web Search API Key</span>
        </label>
      </div>
      <div class="action-row">
        <button class="btn primary" type="button" onclick="ocSaveWebSearchConfig()">保存 Web Search</button>
        <button class="btn" type="button" onclick="ocOpenTerminalWindow('openclaw doctor')">在终端验证</button>
        <span class="form-status" id="webSaveStatus">保存后需重启插件</span>
      </div>
    </div>
  </section>

  <section class="card">
    <div class="card-head">
      <div>
        <div class="eyebrow">Memory Search</div>
        <h3>Embedding Provider、远端凭证与本地模型路径</h3>
        <p class="muted">这一部分对应官方 <code>agents.defaults.memorySearch</code> 配置。远端 provider 可直接填写 API Key，本地 provider 则填写模型路径。</p>
      </div>
      <div class="header-actions">
        <a class="btn" id="memoryDocsLink" href="{memory_docs}" target="_blank" rel="noopener noreferrer">官方文档</a>
        <a class="btn secondary" id="memoryConsoleLink" href="{memory_console}" target="_blank" rel="noopener noreferrer"{memory_console_style}>登录 / 控制台</a>
      </div>
    </div>
    <div class="config-form">
      {memory_enabled_toggle}
      <div class="form-grid">
        <label class="field">
          <span class="field-label">Provider</span>
          <select id="memoryProvider" data-kind="memory" onchange="ocSyncProviderMeta('memory')">{memory_provider_options}</select>
        </label>
        <label class="field">
          <span class="field-label">当前 Provider 状态</span>
          <input type="text" value="{memory_label} / {memory_api_state}" readonly>
        </label>
        <label class="field">
          <span class="field-label">Embedding Model</span>
          <input type="text" id="memoryModel" value="{memory_model_value}" placeholder="例如 text-embedding-3-large">
        </label>
        <label class="field">
          <span class="field-label">Fallback</span>
          <input type="text" id="memoryFallback" value="{memory_fallback}" placeholder="none / openai / voyage ...">
        </label>
        <div id="memoryRemoteGroup" class="field-group field-span-2">
          <label class="field">
            <span class="field-label">Remote Base URL</span>
            <input type="text" id="memoryBaseUrl" value="{memory_base_url}" placeholder="留空使用 provider 默认地址">
          </label>
          <label class="field">
            <span class="field-label">Remote API Key</span>
            <input type="password" id="memoryApiKey" value="" placeholder="留空表示保持现有值">
            <span class="field-help">不会回显当前密钥。只有填写新值时才会覆盖。</span>
          </label>
          <label class="field-inline">
            <input type="checkbox" id="memoryClearApiKey">
            <span>清除当前 Memory Search API Key</span>
          </label>
        </div>
        <div id="memoryLocalGroup" class="field-group field-span-2">
          <label class="field">
            <span class="field-label">Local Model Path</span>
            <input type="text" id="memoryLocalModelPath" value="{memory_local_model_path}" placeholder="/models/embeddings/...">
          </label>
        </div>
      </div>
      <div class="action-row">
        <button class="btn primary" type="button" onclick="ocSaveMemorySearchConfig()">保存 Memory Search</button>
        <button class="btn" type="button" onclick="ocOpenTerminalWindow('openclaw memory status --deep')">在终端验证</button>
        <span class="form-status" id="memorySaveStatus">保存后需重启插件</span>
      </div>
    </div>
  </section>

  <section class="card">
    <div class="card-head">
      <div>
        <div class="eyebrow">模型</div>
        <h3>对话模型选择</h3>
        <p class="muted">这里写入官方 <code>agents.defaults.model.primary</code> 和 <code>fallbacks</code>。主模型可直接填完整模型 ID，也可以从常用建议里选择。</p>
      </div>
      <div class="header-actions">
        <a class="btn" href="{model_docs}" target="_blank" rel="noopener noreferrer">官方文档</a>
        <button class="btn secondary" type="button" onclick="ocOpenTerminalWindow('openclaw onboard')">打开初始化向导</button>
      </div>
    </div>
    <div class="config-form">
      <div class="form-grid">
        <label class="field field-span-2">
          <span class="field-label">Primary Model</span>
          <input type="text" id="primaryModel" value="{current_model}" list="modelSuggestions" placeholder="例如 openai-codex/gpt-5.4">
          <datalist id="modelSuggestions">{model_datalist}</datalist>
        </label>
        <label class="field field-span-2">
          <span class="field-label">Fallback Models</span>
          <input type="text" id="fallbackModels" value="" placeholder="逗号分隔，例如 openai/gpt-5.4-mini, google/gemini-2.5-flash">
          <span class="field-help">留空表示不设置 fallback。保存时会写成官方的数组结构。</span>
        </label>
      </div>
      <div class="action-row">
        <button class="btn primary" type="button" onclick="ocSaveModelConfig()">保存模型配置</button>
        <button class="btn" type="button" onclick="ocOpenTerminalWindow('openclaw status --deep')">在终端查看当前模型</button>
        <span class="form-status" id="modelSaveStatus">保存后需重启插件</span>
      </div>
    </div>
  </section>

  <section class="card">
    <h3>相关文件</h3>
    <div class="kv-list">
      {oc_config_path}
      {panel_path}
      {mcporter_config_path}
      {cert_dir}
      {runtime_dir}
      {backup_dir}
    </div>
    <div class="action-row">
      <button class="btn" type="button" onclick="ocRunCommand('{openclaw_config_cmd}')">查看 openclaw.json</button>
      <button class="btn" type="button" onclick="ocRunCommand('{panel_config_cmd}')">查看 Add-on 配置文件</button>
      <button class="btn" type="button" onclick="ocRunCommand('{mcporter_config_cmd}')">查看 mcporter.json</button>
    </div>
  </section>
</div>"#,
        panel_config_path = panel_config_path().display(),
        access = kv_row("访问模式", &config.access_mode),
        mode = kv_row("网关模式", &config.gateway_mode),
        https = kv_row("HTTPS 端口", &config.https_port),
        model = kv_row("当前对话模型", &config.current_model),
        web_status = kv_row("Web Search", if config.web_enabled { "enabled" } else { "disabled" }),
        web_provider = kv_row("Provider", &config.web_provider),
        web_model = kv_row("Provider Model", &web_model_display),
        web_api = kv_row("API / 凭证", web_api_state),
        memory_status = kv_row("Memory Search", if config.memory_enabled { "enabled" } else { "disabled" }),
        memory_provider = kv_row("Provider", &config.memory_provider),
        memory_model = kv_row("Embedding Model", &memory_model_display),
        memory_api = kv_row("API / 凭证", memory_api_state),
        web_docs = html_attr_escape(web_docs),
        web_console = html_attr_escape(web_console),
        web_console_style = if web_console.is_empty() { r#" style="display:none""# } else { "" },
        web_enabled_toggle = form_checkbox(
            "webEnabled",
            config.web_enabled,
            "启用 Web Search",
            "对应官方 tools.web.search.enabled",
        ),
        web_provider_options = web_provider_options,
        web_label = web_label,
        web_api_state = web_api_state,
        web_base_url = html_attr_escape(&config.web_base_url),
        web_model_value = html_attr_escape(&config.web_model),
        memory_docs = html_attr_escape(memory_docs),
        memory_console = html_attr_escape(memory_console),
        memory_console_style = if memory_console.is_empty() { r#" style="display:none""# } else { "" },
        memory_enabled_toggle = form_checkbox(
            "memoryEnabled",
            config.memory_enabled,
            "启用 Memory Search",
            "对应官方 agents.defaults.memorySearch.enabled",
        ),
        memory_provider_options = memory_provider_options,
        memory_label = memory_label,
        memory_api_state = memory_api_state,
        memory_model_value = html_attr_escape(&config.memory_model),
        memory_fallback = html_attr_escape(&config.memory_fallback),
        memory_base_url = html_attr_escape(&config.memory_base_url),
        memory_local_model_path = html_attr_escape(&config.memory_local_model_path),
        model_docs = MODEL_CONFIG_DOCS_URL,
        current_model = html_attr_escape(&config.current_model),
        model_datalist = model_datalist,
        oc_config_path = kv_row("OpenClaw 配置", &config.openclaw_config_path),
        panel_path = kv_row("Add-on 配置", &panel_config_path().display().to_string()),
        mcporter_config_path = kv_row("MCPorter 配置", &config.mcporter_config_path),
        cert_dir = kv_row("证书目录", &config.cert_dir),
        runtime_dir = kv_row("运行时目录", &config.openclaw_runtime_dir),
        backup_dir = kv_row("备份目录", &config.backup_dir),
        openclaw_config_cmd = html_attr_escape(&openclaw_config_cmd),
        panel_config_cmd = html_attr_escape(&panel_config_cmd),
        mcporter_config_cmd = html_attr_escape(&mcporter_config_cmd),
    )
}

fn commands_content_v2(config: &PageConfig) -> String {
    let control_actions = [
        ("OpenClaw CLI", "openclaw tui"),
        ("设备列表", "openclaw devices list"),
        ("批准最新配对", "openclaw devices approve --latest"),
        ("初始化向导", "openclaw onboard"),
    ]
    .iter()
    .map(|(label, cmd)| {
        if *cmd == "openclaw tui" {
            terminal_window_button(label, cmd)
        } else {
            action_button(label, cmd)
        }
    })
    .collect::<Vec<_>>()
    .join("");

    let health_actions = [
        (
            "探针状态",
            "curl -fsS http://127.0.0.1:48099/healthz && echo && curl -fsS http://127.0.0.1:48099/readyz",
        ),
        ("JSON 健康", "openclaw health --json"),
        ("运行状态", "openclaw status --deep"),
    ]
    .iter()
    .map(|(label, cmd)| diag_button(label, cmd))
    .collect::<Vec<_>>()
    .join("");

    let maintenance_actions = [
        ("运行 doctor", "openclaw doctor"),
        ("doctor --fix", "openclaw doctor --fix"),
        ("安全审计", "openclaw security audit --deep"),
        ("记忆状态", "openclaw memory status --deep"),
        ("本机版本", "openclaw --version"),
    ]
    .iter()
    .map(|(label, cmd)| diag_button(label, cmd))
    .collect::<Vec<_>>()
    .join("");

    let log_stream_actions = [
        ("跟随日志", "openclaw logs --follow"),
        ("网关日志", "tail -f /tmp/openclaw/openclaw-$(date +%F).log"),
    ]
    .iter()
    .map(|(label, cmd)| terminal_window_button(label, cmd))
    .collect::<Vec<_>>()
    .join("");

    let config_actions = [
        ("MCP 列表", "mcporter list".to_string()),
        ("OpenClaw 配置", format!("cat {}", config.openclaw_config_path)),
        ("MCP 配置", format!("cat {}", config.mcporter_config_path)),
        ("Workspace", format!("ls -la {}", config.openclaw_workspace_dir)),
        ("状态根目录", format!("ls -la {}", config.openclaw_state_dir)),
        ("备份目录", format!("ls -la {}", config.backup_dir)),
        (
            "立即备份",
            format!(
                "set -e; echo '▶ 创建目录…'; mkdir -p {backup}/.openclaw {backup}/.mcporter; echo '▶ 备份 .openclaw…'; rsync -a --delete {oc_state}/ {backup}/.openclaw/; echo '▶ 备份 .mcporter…'; rsync -a --delete {mcp_home}/ {backup}/.mcporter/; echo '✓ 备份完成'",
                backup = config.backup_dir,
                oc_state = config.openclaw_state_dir,
                mcp_home = config.mcporter_home_dir,
            ),
        ),
    ]
    .iter()
    .map(|(label, cmd)| action_button(label, cmd))
    .collect::<Vec<_>>()
    .join("");

    let token_action = sensitive_button(
        "读取令牌",
        &format!("jq -r '.gateway.auth.token' {}", config.openclaw_config_path),
        "此命令会把 auth token 明文输出到终端，请确认当前环境安全后再执行。",
    );
    let restart_action = action_button(
        "重启网关",
        "curl -fsS -X POST http://127.0.0.1:48099/action/restart",
    );

    format!(
        r#"<div class="page-grid">
  <section class="card">
    <div class="card-head">
      <div>
        <div class="eyebrow">命令行</div>
        <h2>命令工作区</h2>
        <p class="muted">这里按官方操作模型重组。<code>OpenClaw CLI</code> 实际打开的是原生 <code>openclaw tui</code>；进入后普通输入发给 Gateway，本机 shell 命令请使用 <code>!命令</code> 前缀。</p>
      </div>
      <div class="header-actions">
        {load_terminal}
        {close_terminal}
        {open_window}
        <a class="btn" href="./openclaw-ca.crt" target="_blank" rel="noopener noreferrer">下载 CA 证书</a>
      </div>
    </div>

    <div class="command-section">
      <div class="section-label">控制台与配对</div>
      <div class="action-row">{control_actions}</div>
      <div class="mini-tip">TUI 示例：直接输入问题开始会话；输入 <code>!openclaw status</code>、<code>!ha addons logs openclaw_assistant_rust</code> 执行本机命令。</div>
    </div>

    <div class="command-section">
      <div class="section-label">状态与健康</div>
      <div class="action-row">{health_actions}</div>
    </div>

    <div class="command-section">
      <div class="section-label">维护与审计</div>
      <div class="action-row">{maintenance_actions}</div>
    </div>

    <div class="command-section">
      <div class="section-label">日志流（新窗口）</div>
      <div class="action-row">{log_stream_actions}</div>
    </div>

    <div class="command-section">
      <div class="section-label">配置与状态目录</div>
      <div class="action-row">{token_action}{config_actions}</div>
    </div>

    <div class="command-section">
      <div class="section-label">网关控制</div>
      <div class="action-row">{restart_action}</div>
    </div>

    <div class="command-section">
      <div class="section-label">自定义命令</div>
      <div class="custom-cmd-row">
        <input type="text" class="cmd-input" id="customCmdInput"
               placeholder="输入一次性 shell 命令；如果你想要官方交互式 CLI，请优先点上面的 OpenClaw CLI"
               autocomplete="off" spellcheck="false">
        <button class="btn btn-action" type="button" onclick="ocRunCustomCommand()">运行</button>
      </div>
    </div>
  </section>
  {terminal}
  <script>
    (function() {{
      var inp = document.getElementById("customCmdInput");
      if (inp) inp.addEventListener("keydown", function(e) {{ if (e.key === "Enter") ocRunCustomCommand(); }});
    }})();
  </script>
</div>"#,
        load_terminal = primary_button("加载终端", "ocLoadTerminal()"),
        close_terminal = ghost_button("关闭终端", "ocCloseTerminal()"),
        open_window = secondary_button("新窗口打开终端", "ocOpenTerminalWindow()"),
        control_actions = control_actions,
        health_actions = health_actions,
        maintenance_actions = maintenance_actions,
        log_stream_actions = log_stream_actions,
        token_action = token_action,
        config_actions = config_actions,
        restart_action = restart_action,
        terminal = terminal_card(
            "嵌入式终端",
            "这个区域更适合一次性命令和日志查看；如果你需要官方交互式体验，请使用上面的 OpenClaw CLI 打开原生 TUI。",
            "加载终端",
        ),
    )
}

fn commands_content_native(_config: &PageConfig) -> String {
    let entry_actions = [
        terminal_window_button("OpenClaw CLI", "openclaw tui"),
        primary_link_button(
            "鍘熺敓 Gateway",
            "ocGatewayLinkCmd",
            "return ocOpenGatewayLink(event, this)",
        ),
        terminal_window_button("鍒濆鍖栧悜瀵?", "openclaw onboard"),
    ]
    .join("");

    let status_actions = [
        diag_button("鍋ュ悍妫€鏌?", "openclaw health --json"),
        diag_button("杩愯鐘舵€?", "openclaw status --deep"),
        diag_button(
            "鏈湴探针",
            "curl -fsS http://127.0.0.1:48099/healthz && echo && curl -fsS http://127.0.0.1:48099/readyz",
        ),
    ]
    .join("");

    let pair_actions = [
        action_button("璁惧鍒楄〃", "openclaw devices list"),
        action_button("鎵瑰噯鏈€鏂伴厤瀵?", "openclaw devices approve --latest"),
    ]
    .join("");

    let maintenance_actions = [
        diag_button("杩愯 doctor", "openclaw doctor"),
        diag_button("doctor --fix", "openclaw doctor --fix"),
        diag_button("瀹夊叏瀹¤", "openclaw security audit --deep"),
        diag_button("璁板繂鐘舵€?", "openclaw memory status --deep"),
        diag_button("鏈満鐗堟湰", "openclaw --version"),
    ]
    .join("");

    let log_actions = [
        terminal_window_button("璺熼殢鏃ュ織", "openclaw logs --follow"),
        terminal_window_button("缃戝叧鏃ュ織", "tail -f /tmp/openclaw/openclaw-$(date +%F).log"),
    ]
    .join("");

    format!(
        r#"<div class="page-grid">
  <section class="card">
    <div class="card-head">
      <div>
        <div class="eyebrow">鍛戒护琛?/div>
        <h2>鍘熺敓鍏ュ彛</h2>
        <p class="muted">杩欎釜椤甸潰鍙繚鐣欐洿鎺ヨ繎瀹樻柟鐨勫叆鍙ｃ€?code>OpenClaw CLI</code> 鎵撳紑鐨勬槸鍘熺敓 <code>openclaw tui</code>锛涘湪 TUI 閲岃緭鍏?code>!鍛戒护</code> 鍙互鎵ц鏈満 shell 鍛戒护銆?/p>
      </div>
      <div class="header-actions">
        {load_terminal}
        {close_terminal}
        {open_window}
        <a class="btn" href="./openclaw-ca.crt" target="_blank" rel="noopener noreferrer">涓嬭浇 CA 璇佷功</a>
      </div>
    </div>

    <div class="command-section">
      <div class="section-label">鍘熺敓鍏ュ彛</div>
      <div class="action-row">{entry_actions}</div>
      <div class="mini-tip">TUI 绀轰緥锛氳緭鍏?code>!openclaw status</code> 鎴?code>!ha addons logs openclaw_assistant_rust</code> 鎵ц鏈満鍛戒护銆?/div>
    </div>

    <div class="command-section">
      <div class="section-label">鐘舵€佷笌鍋ュ悍</div>
      <div class="action-row">{status_actions}</div>
    </div>

    <div class="command-section">
      <div class="section-label">璁惧涓庨厤瀵?/div>
      <div class="action-row">{pair_actions}</div>
    </div>

    <div class="command-section">
      <div class="section-label">缁存姢涓庡璁?/div>
      <div class="action-row">{maintenance_actions}</div>
    </div>

    <div class="command-section">
      <div class="section-label">鏃ュ織娴侊紙鏂扮獥鍙ｏ級</div>
      <div class="action-row">{log_actions}</div>
    </div>
  </section>
  {terminal}
</div>"#,
        load_terminal = primary_button("鍔犺浇缁堢", "ocLoadTerminal()"),
        close_terminal = ghost_button("鍏抽棴缁堢", "ocCloseTerminal()"),
        open_window = secondary_button("鏂扮獥鍙ｆ墦寮€缁堢", "ocOpenTerminalWindow()"),
        entry_actions = entry_actions,
        status_actions = status_actions,
        pair_actions = pair_actions,
        maintenance_actions = maintenance_actions,
        log_actions = log_actions,
        terminal = terminal_card(
            "宓屽叆寮忕粓绔?",
            "杩欎釜鍖哄煙閫傚悎涓€娆℃€у懡浠ゅ拰鏃ュ織鏌ョ湅锛屽鏋滀綘闇€瑕佹洿鍘熺敓鐨勪氦浜掑紡浣撻獙锛岃浣跨敤涓婇潰鐨?OpenClaw CLI 鎵撳紑 TUI銆?",
            "鍔犺浇缁堢",
        ),
    )
}

fn logs_content() -> String {
    let log_actions = [
        ("跟随日志", "openclaw logs --follow"),
        ("网关日志", "tail -f /tmp/openclaw/openclaw-$(date +%F).log"),
        ("运行 doctor", "openclaw doctor"),
        ("doctor --fix", "openclaw doctor --fix"),
        ("状态深查", "openclaw status --deep"),
    ]
    .iter()
    .map(|(label, cmd)| action_button(label, cmd))
    .collect::<Vec<_>>()
    .join("");

    format!(
        r#"<div class="page-grid">
  <section class="card">
    <div class="card-head">
      <div>
        <div class="eyebrow">日志</div>
        <h2>日志与诊断</h2>
        <p class="muted">当你需要持续观察运行输出、排查报错、或者确认修复结果时，就来这一页。上面的按钮会把常用日志命令直接送到下面的日志终端里执行。</p>
      </div>
    </div>

    <div class="action-row">{log_actions}</div>
  </section>

  {terminal}
</div>"#,
        log_actions = log_actions,
        terminal = terminal_card(
            "日志终端",
            "点击上方按钮执行命令，输出结果会在这里显示。",
            "加载日志终端",
        ),
    )
}

fn force_chinese_ui(mut html: String) -> String {
    let replacements = [
        ("OpenClaw 路 ", "OpenClaw - "),
        ("棣栭〉", "首页"),
        ("鍩虹閰嶇疆", "基础配置"),
        ("鍛戒护琛?", "命令行"),
        ("鏃ュ織", "日志"),
        ("涓嶅彲鐢?", "不可用"),
        ("鍛戒护琛屽伐浣滃尯", "命令工作区"),
        ("閰嶇疆涓績", "配置中心"),
        ("Web Search銆丮emory Search 涓庢ā鍨嬮€夋嫨", "Web Search、Memory Search 与模型选择"),
        ("杩愯鐘舵€佹€昏", "运行状态总览"),
        (
            "鏌ョ湅 OpenClaw 褰撳墠鏄惁姝ｅ父杩愯銆佸悇鏈嶅姟杩涚▼鐘舵€侊紝浠ュ強绯荤粺璧勬簮鍗犵敤鎯呭喌銆?",
            "查看 OpenClaw 当前是否正常运行、各项服务进程状态，以及系统资源占用情况。",
        ),
        ("杩愯姝ｅ父", "运行正常"),
        ("鍋ュ悍妫€鏌ラ€氳繃", "健康检查通过"),
        ("鍝嶅簲寮傚父", "响应异常"),
        ("鍋ュ悍妫€鏌ユ湭閫氳繃锛岃鏌ョ湅鏃ュ織", "健康检查未通过，请查看日志"),
        ("鍏抽敭杩涚▼鍏ㄩ儴鍦ㄧ嚎", "关键进程全部在线"),
        ("閮ㄥ垎鍦ㄧ嚎", "部分在线"),
        ("寤鸿鏌ョ湅鍛戒护琛屽拰鏃ュ織椤?", "建议查看命令行和日志页"),
        ("寰呮鏌?", "待检查"),
        ("鍏抽敭杩涚▼鏁伴噺涓嶈冻", "关键进程数量不足"),
        ("杩涚▼闈㈡澘", "进程面板"),
        ("鏈嶅姟涓?PID", "服务与 PID"),
        ("鍦ㄧ嚎", "在线"),
        ("寰呭惎鍔?", "未启动"),
        ("鏈娴嬪埌 PID", "未检测到 PID"),
        ("鍦ㄧ嚎杩涚▼", "在线进程"),
        ("Gateway銆両ngress銆乁I銆丄ction", "Gateway、Ingress、UI"),
        ("鍘熺敓缃戝叧榛樿鐩戝惉绔彛", "原生网关默认监听端口"),
        ("鎵撳紑缃戝叧", "打开网关"),
        ("杩涘叆鍛戒护琛?", "进入命令行"),
        ("璁块棶妯″紡", "访问模式"),
        ("缃戝叧妯″紡", "网关模式"),
        ("Add-on 鐗堟湰", "Add-on 版本"),
        ("OpenClaw 鐗堟湰", "OpenClaw 版本"),
        ("AI 妯″瀷", "AI 模型"),
        ("鐢ㄤ簬鐩存帴璁块棶 OpenClaw API锛岃鍕挎硠闇?", "用于直接访问 OpenClaw API，请勿泄露。"),
        ("缁堢宸ヤ綔鍖?", "终端工作区"),
        ("宸ヤ綔鍖虹粓绔?", "工作区终端"),
        ("缁堢鎸夐渶鍔犺浇", "终端按需加载"),
        ("閸樼喓鏁撻崗銉ュ經", "原生入口"),
        ("閻樿埖鈧椒绗岄崑銉ユ倣", "状态与探针"),
        ("鐠佹儳顦稉搴ㄥ帳鐎?", "设备配对"),
        ("缂佸瓨濮㈡稉搴☆吀鐠?", "维护与诊断"),
        ("閺冦儱绻斿ù渚婄礄閺傛壆鐛ラ崣锝忕礆", "日志与跟踪"),
        ("閸旂姾娴囩紒鍫㈩伂", "加载终端"),
        ("閸忔娊妫寸紒鍫㈩伂", "关闭终端"),
        ("閸嬨儱鎮嶅Λ鈧弻?", "健康检查"),
        ("鏉╂劘顢戦悩鑸碘偓?", "运行状态"),
        ("閺堫剙婀存帰閽?", "本地探针"),
        ("鐠佹儳顦崚妤勩€?", "查看设备"),
        ("閹电懓鍣張鈧弬浼村帳鐎?", "批准最新配对"),
        ("鏉╂劘顢?doctor", "运行 doctor"),
        ("鐎瑰鍙忕€孤ゎ吀", "安全审计"),
        ("鐠佹澘绻傞悩鑸碘偓?", "记忆状态"),
        ("閺堫剚婧€閻楀牊婀?", "版本信息"),
        ("鐠虹喖娈㈤弮銉ョ箶", "跟随日志"),
        ("缂冩垵鍙伴弮銉ョ箶", "网关日志"),
        ("鏃ュ織涓庤瘖鏂?", "日志与诊断"),
        ("OpenClawHAOSAddon-Rust", "OpenClaw HAOS 面板"),
    ];

    for (from, to) in replacements {
        html = html.replace(from, to);
    }
    html
}

fn render_shell(
    config: &PageConfig,
    page: NavPage,
    title: &str,
    subtitle: &str,
    content: &str,
) -> Html<String> {
    let nav_home = nav_link(page, NavPage::Home, "./", "首页");
    let nav_config = nav_link(page, NavPage::Config, "./config", "基础配置");
    let nav_commands = nav_link(page, NavPage::Commands, "./commands", "命令行");
    let nav_logs = nav_link(page, NavPage::Logs, "./logs", "日志");
    let gateway_url = js_string(&config.gateway_url);

    Html(force_chinese_ui(format!(
        r#"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>OpenClaw · {title}</title>
  <style>
    :root {{
      --bg:#f0f4fb; --panel:#ffffff; --line:#e2eaf6; --text:#1a2b42; --muted:#64748b;
      --blue:#2563eb; --blue-dim:#eff6ff;
      --shadow:0 1px 4px rgba(15,23,42,.06),0 4px 16px rgba(15,23,42,.07);
      --shadow-md:0 4px 20px rgba(15,23,42,.12); --r:16px;
    }}
    *{{ box-sizing:border-box; margin:0; }}
    html{{ scroll-behavior:smooth; }}
    body{{
      color:var(--text); font-family:"Segoe UI","PingFang SC","Microsoft YaHei",sans-serif;
      background:linear-gradient(160deg,#eaf1ff 0%,#f0f4fb 40%,#f5f0ff 100%);
      background-attachment:fixed; min-height:100vh; font-size:15px;
    }}
    .app-header{{
      position:sticky; top:0; z-index:100; height:60px;
      display:flex; align-items:center; padding:0 28px; gap:16px;
      background:linear-gradient(135deg,#0d1b38 0%,#111f3d 60%,#152240 100%);
      border-bottom:1px solid rgba(255,255,255,.06);
      box-shadow:0 1px 0 rgba(255,255,255,.04),0 4px 24px rgba(0,0,0,.36);
    }}
    .app-brand{{ display:flex; align-items:center; gap:10px; flex:0 0 auto; text-decoration:none; color:inherit; }}
    .app-brand-badge{{
      width:32px; height:32px; display:grid; place-items:center;
      border-radius:9px; background:linear-gradient(180deg,#1a3f70 0%,#102850 100%);
      box-shadow:0 2px 8px rgba(0,0,0,.32); flex:0 0 auto;
    }}
    .brand-mark{{ display:block; width:24px; height:24px; }}
    .app-brand-text{{ display:flex; flex-direction:column; gap:1px; }}
    .app-brand-name{{ color:#e0ecff; font-size:14px; font-weight:900; letter-spacing:-.01em; line-height:1; }}
    .app-brand-sub{{ color:#5a7ba8; font-size:11px; font-weight:700; line-height:1; }}
    .app-nav{{ display:flex; align-items:center; gap:2px; flex:1; overflow-x:auto; }}
    .nav-link{{
      display:inline-flex; align-items:center; height:34px; padding:0 14px;
      border-radius:8px; text-decoration:none; color:#8aacd4;
      font-size:13px; font-weight:700; white-space:nowrap;
      transition:color .14s,background .14s;
    }}
    .nav-link:hover{{ color:#d4e6ff; background:rgba(255,255,255,.07); }}
    .nav-link.active{{ color:#60a5fa; background:rgba(96,165,250,.14); }}
    .app-meta{{ flex:0 0 auto; }}
    .version-chip{{
      display:inline-flex; align-items:center; height:26px; padding:0 10px;
      border-radius:999px; border:1px solid rgba(255,255,255,.1);
      background:rgba(255,255,255,.05); color:#6a8fb5; font-size:11px; font-weight:800;
    }}
    .page-header{{
      max-width:1460px; margin:0 auto; padding:28px 24px 22px;
      border-bottom:1px solid rgba(148,180,240,.22);
      margin-bottom:0;
    }}
    .page-eyebrow{{ color:#7a9dc0; font-size:11px; font-weight:900; letter-spacing:.14em; text-transform:uppercase; margin-bottom:6px; }}
    .page-title{{ font-size:26px; font-weight:900; letter-spacing:-.03em; color:#0f1e38; line-height:1.15; margin-bottom:6px; }}
    .page-sub{{ font-size:14px; line-height:1.75; max-width:640px; }}
    .wrap{{ max-width:1460px; margin:0 auto; padding:24px 24px 56px; }}
    .card{{
      border:1px solid rgba(210,226,248,.85); border-radius:var(--r); background:rgba(255,255,255,.92);
      box-shadow:0 2px 8px rgba(15,23,42,.05),0 8px 28px rgba(15,23,42,.07);
      padding:22px 24px; transition:box-shadow .22s,transform .22s;
      backdrop-filter:blur(2px);
    }}
    .card:hover{{ box-shadow:0 4px 16px rgba(15,23,42,.09),0 16px 40px rgba(15,23,42,.1); transform:translateY(-1px); }}
    .card h2{{ margin:0 0 8px; font-size:21px; line-height:1.2; letter-spacing:-.025em; font-weight:900; }}
    .card h3{{ margin:0 0 8px; font-size:16px; line-height:1.2; font-weight:800; }}
    .eyebrow{{ color:#7a9ab8; font-size:11px; font-weight:900; letter-spacing:.1em; text-transform:uppercase; margin-bottom:6px; }}
    .muted{{ color:var(--muted); line-height:1.75; margin:0; }}
    .btn{{
      min-height:36px; display:inline-flex; align-items:center; justify-content:center;
      padding:7px 15px; border-radius:10px; border:1px solid #d1ddef; background:#fff;
      color:var(--text); text-decoration:none; font:700 13px/1 inherit; cursor:pointer;
      transition:transform .13s,box-shadow .13s,border-color .13s,background .13s; white-space:nowrap;
    }}
    .btn:hover{{ transform:translateY(-1px); box-shadow:0 4px 12px rgba(37,99,235,.12); border-color:#b0c8e8; background:#f8fbff; }}
    .btn:active{{ transform:none; }}
    .btn.primary{{ color:#fff; border-color:transparent; background:linear-gradient(135deg,#2563eb,#1d4ed8); box-shadow:0 2px 8px rgba(37,99,235,.28); }}
    .btn.primary:hover{{ box-shadow:0 4px 16px rgba(37,99,235,.38); }}
    .btn.secondary{{ color:#fff; border-color:transparent; background:linear-gradient(135deg,#7c3aed,#6d28d9); box-shadow:0 2px 8px rgba(124,58,237,.28); }}
    .btn.secondary:hover{{ box-shadow:0 4px 16px rgba(124,58,237,.38); }}
    .btn.btn-action{{ color:#1e40af; border-color:#bfdbfe; background:linear-gradient(135deg,#eff6ff,#dbeafe); }}
    .btn.btn-action:hover{{ background:linear-gradient(135deg,#dbeafe,#bfdbfe); border-color:#93c5fd; box-shadow:0 4px 12px rgba(37,99,235,.14); }}
    .btn.btn-diag{{ color:#065f46; border-color:#a7f3d0; background:linear-gradient(135deg,#ecfdf5,#d1fae5); }}
    .btn.btn-diag:hover{{ background:linear-gradient(135deg,#d1fae5,#a7f3d0); border-color:#6ee7b7; box-shadow:0 4px 12px rgba(16,185,129,.14); }}
    .btn.btn-danger{{ color:#991b1b; border-color:#fecaca; background:linear-gradient(135deg,#fff5f5,#fee2e2); }}
    .btn.btn-danger:hover{{ background:linear-gradient(135deg,#fee2e2,#fecaca); border-color:#fca5a5; box-shadow:0 4px 12px rgba(239,68,68,.16); }}
    .page-grid{{ display:grid; gap:16px; }}
    .summary-strip{{ display:grid; grid-template-columns:repeat(auto-fit,minmax(190px,1fr)); gap:12px; }}
    .two-col{{ display:grid; grid-template-columns:minmax(0,1.18fr) minmax(300px,.92fr); gap:16px; align-items:start; }}
    .three-up{{ display:grid; grid-template-columns:repeat(3,minmax(0,1fr)); gap:14px; }}
    .split-grid{{ display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); gap:14px; }}
    .feature-grid{{ display:grid; grid-template-columns:repeat(auto-fit,minmax(190px,1fr)); gap:12px; }}
    .resource-grid{{ display:grid; grid-template-columns:repeat(auto-fit,minmax(190px,1fr)); gap:12px; }}
    .header-actions,.action-row,.pill-row,.toolbar-grid{{ display:flex; gap:10px; flex-wrap:wrap; }}
    .summary-strip-card{{
      position:relative; overflow:hidden; border:1px solid var(--line);
      border-radius:14px; background:#fff; box-shadow:var(--shadow); padding:15px 17px;
    }}
    .summary-strip-card::before{{
      content:""; position:absolute; left:0; top:0; bottom:0;
      width:3px; background:#c9d8ee; border-radius:3px 0 0 3px;
    }}
    .summary-strip-card.tone-good::before{{ background:#22c55e; }}
    .summary-strip-card.tone-warn::before{{ background:#f59e0b; }}
    .summary-strip-card.tone-danger::before{{ background:#ef4444; }}
    .summary-strip-card.tone-blue::before{{ background:#3b82f6; }}
    .summary-strip-card.tone-teal::before{{ background:#06b6d4; }}
    .summary-strip-card.tone-violet::before{{ background:#8b5cf6; }}
    .summary-strip-title{{ color:#7a94b4; font-size:11px; font-weight:900; letter-spacing:.08em; text-transform:uppercase; margin-bottom:7px; }}
    .summary-strip-value{{ font-size:20px; line-height:1.1; font-weight:900; letter-spacing:-.03em; margin-bottom:5px; }}
    .summary-strip-sub{{ color:#7a94b4; font-size:12px; line-height:1.6; }}
    .card-head{{ display:flex; justify-content:space-between; align-items:flex-start; gap:16px; margin-bottom:16px; }}
    .card-head.compact{{ margin-bottom:16px; }}
    .stats-grid{{ display:grid; grid-template-columns:repeat(auto-fit,minmax(150px,1fr)); gap:12px; margin:14px 0; }}
    .stat-card,.info-tile,.soft-card,.resource-card{{ border:1px solid var(--line); border-radius:14px; background:#fff; padding:15px; }}
    .stat-card{{ min-height:118px; display:flex; flex-direction:column; justify-content:space-between; }}
    .stat-label,.resource-label{{ color:var(--muted); font-size:12px; font-weight:800; }}
    .stat-value,.resource-value{{ font-size:19px; font-weight:900; line-height:1.15; letter-spacing:-.02em; word-break:break-word; margin:8px 0 3px; }}
    .stat-sub,.resource-sub,.info-tile p,.soft-card p{{ color:#7a94b4; font-size:12px; line-height:1.65; margin:0; }}
    .status-panel{{ display:grid; grid-template-columns:minmax(0,1.25fr) minmax(270px,.75fr); gap:16px; align-items:start; }}
    .status-summary-strip{{ margin-bottom:12px; }}
    .status-stats-grid{{ margin:0 0 12px; }}
    .live-row{{
      display:flex; align-items:flex-start; gap:12px; margin-bottom:14px;
      padding:13px 15px; border-radius:12px; border:1px solid #dfe8f8;
      background:linear-gradient(180deg,#fafcff 0%,#f3f8ff 100%);
    }}
    .live-row strong{{ display:block; font-size:16px; line-height:1.2; margin-bottom:2px; }}
    .live-copy p{{ margin:0; color:var(--muted); font-size:13px; }}
    .live-dot{{
      width:9px; height:9px; border-radius:999px; background:#22c55e;
      flex:0 0 auto; margin-top:5px; box-shadow:0 0 0 3px rgba(34,197,94,.18);
      animation:dot-pulse 2.6s ease-in-out infinite;
    }}
    @keyframes dot-pulse{{
      0%,100%{{ box-shadow:0 0 0 3px rgba(34,197,94,.18); }}
      50%{{ box-shadow:0 0 0 6px rgba(34,197,94,.07); }}
    }}
    .live-row.is-warn .live-dot{{ background:#f59e0b; animation:dot-pulse-warn 2.6s ease-in-out infinite; }}
    @keyframes dot-pulse-warn{{
      0%,100%{{ box-shadow:0 0 0 3px rgba(245,158,11,.18); }}
      50%{{ box-shadow:0 0 0 6px rgba(245,158,11,.07); }}
    }}
    .live-row.is-danger .live-dot{{ background:#ef4444; box-shadow:0 0 0 3px rgba(239,68,68,.18); animation:none; }}
    .pill-inline,.pid-chip,.pill{{ display:inline-flex; align-items:center; gap:6px; border-radius:999px; padding:6px 12px; background:#eff6ff; color:#2563eb; font-size:12px; font-weight:800; }}
    .pill{{ justify-content:space-between; min-width:150px; }}
    .panel-left,.panel-right{{ border:1px solid var(--line); border-radius:14px; background:#fff; padding:16px; }}
    .panel-right{{ background:linear-gradient(180deg,#f9fbff 0%,#f3f7ff 100%); }}
    .panel-title-row{{ display:flex; justify-content:space-between; align-items:flex-start; gap:12px; margin-bottom:12px; }}
    .panel-title-row h3{{ margin:0; }}
    .service-grid{{ display:grid; gap:9px; }}
    .service-badge{{
      border:1px solid #dce8f4; border-radius:11px; background:#fff; padding:10px 12px;
      transition:border-color .15s,box-shadow .15s;
    }}
    .service-badge.is-online{{ background:linear-gradient(135deg,#f0fdf6 0%,#f8fdfb 100%); border-color:#bbf7d0; }}
    .service-badge.is-offline{{ background:linear-gradient(135deg,#fff5f5 0%,#fffafa 100%); border-color:#fecaca; }}
    .service-badge-top{{ display:flex; justify-content:space-between; align-items:center; gap:8px; margin-bottom:4px; }}
    .service-name{{ font-size:13px; font-weight:900; color:#1e3a5f; display:flex; align-items:center; gap:6px; }}
    .svc-dot{{ width:7px; height:7px; border-radius:999px; flex:0 0 auto; background:#d1d5db; }}
    .service-badge.is-online .svc-dot{{ background:#22c55e; box-shadow:0 0 0 2px rgba(34,197,94,.2); }}
    .service-badge.is-offline .svc-dot{{ background:#ef4444; }}
    .service-state{{ display:inline-flex; align-items:center; min-height:22px; padding:0 8px; border-radius:999px; font-size:11px; font-weight:900; }}
    .service-badge.is-online .service-state{{ background:#dcfce7; color:#15803d; }}
    .service-badge.is-offline .service-state{{ background:#fee2e2; color:#b91c1c; }}
    .service-meta{{ color:#6b88a8; font-size:12px; font-weight:700; }}
    .note-box{{ padding:12px 14px; border-radius:12px; background:#f8fafc; border:1px solid #e2eaf6; color:#4d6784; font-size:13px; line-height:1.75; }}
    .notice-badge{{ padding:10px 14px; border-radius:10px; font-size:13px; font-weight:700; line-height:1.65; margin-bottom:8px; }}
    .notice-badge.warn{{ background:#fffbeb; border:1px solid #fcd34d; color:#92400e; }}
    .token-section{{ padding:12px 14px; border-radius:12px; background:#f0f7ff; border:1px solid #bfdbfe; margin-bottom:10px; }}
    .token-header{{ display:flex; align-items:baseline; gap:10px; margin-bottom:8px; }}
    .token-label{{ font-size:12px; font-weight:800; color:#1e40af; text-transform:uppercase; letter-spacing:.04em; }}
    .token-hint{{ font-size:12px; color:#6b88a8; }}
    .token-row{{ display:flex; align-items:center; gap:8px; }}
    .token-val{{ flex:1; font-family:monospace; font-size:13px; background:#fff; border:1px solid #bfdbfe; border-radius:8px; padding:6px 10px; color:#1e3a5f; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; min-width:0; }}
    .custom-cmd-row{{ display:flex; gap:10px; align-items:center; }}
    .cmd-input{{
      flex:1; height:38px; padding:0 14px; border-radius:10px;
      border:1px solid #d1ddef; background:#fff;
      font:500 13px/1 "Segoe UI","PingFang SC","Microsoft YaHei",sans-serif;
      color:#1a2b42; outline:none; transition:border-color .15s,box-shadow .15s;
    }}
    .cmd-input:focus{{ border-color:#60a5fa; box-shadow:0 0 0 3px rgba(96,165,250,.18); }}
    .cmd-input::placeholder{{ color:#94a3b8; }}
    .config-form{{ display:grid; gap:16px; }}
    .form-grid{{ display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); gap:14px; }}
    .field{{ display:grid; gap:6px; }}
    .field-group{{ display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); gap:14px; }}
    .field-span-2{{ grid-column:1 / -1; }}
    .field-label,.toggle-title{{ color:#27466a; font-size:12px; font-weight:900; letter-spacing:.02em; }}
    .field-help,.toggle-help{{ color:#7a94b4; font-size:12px; line-height:1.6; }}
    .field input,.field select{{
      width:100%; min-height:40px; padding:0 13px; border-radius:10px;
      border:1px solid #d1ddef; background:#fff; color:#1a2b42;
      font:500 13px/1 "Segoe UI","PingFang SC","Microsoft YaHei",sans-serif;
      outline:none; transition:border-color .15s, box-shadow .15s;
    }}
    .field input:focus,.field select:focus{{ border-color:#60a5fa; box-shadow:0 0 0 3px rgba(96,165,250,.18); }}
    .field input[readonly]{{ background:#f8fbff; color:#56718f; }}
    .toggle-row,.field-inline{{
      display:flex; align-items:center; justify-content:space-between; gap:12px;
      padding:12px 14px; border-radius:12px; border:1px solid var(--line); background:#f8fbff;
    }}
    .toggle-copy{{ display:grid; gap:3px; }}
    .field-inline{{ justify-content:flex-start; }}
    .field-inline input,.toggle-row input{{ width:18px; height:18px; accent-color:#2563eb; }}
    .provider-links{{ display:flex; gap:10px; flex-wrap:wrap; }}
    .form-status{{ display:inline-flex; align-items:center; min-height:36px; color:#5b708f; font-size:13px; font-weight:700; }}
    .form-status.ok{{ color:#166534; }}
    .form-status.err{{ color:#b91c1c; }}
    .resource-card{{ display:flex; flex-direction:column; gap:9px; }}
    .resource-top{{ display:flex; justify-content:space-between; gap:12px; align-items:center; }}
    .resource-percent{{ color:#27466a; font-size:12px; font-weight:900; }}
    .progress-track{{ width:100%; height:7px; border-radius:999px; overflow:hidden; background:#eaf0fb; }}
    .progress-fill{{ display:block; height:100%; border-radius:999px; background:linear-gradient(90deg,#3b82f6,#60a5fa); transition:width .4s ease; }}
    .progress-fill.tone-blue{{ background:linear-gradient(90deg,#2563eb,#60a5fa); }}
    .progress-fill.tone-teal{{ background:linear-gradient(90deg,#0891b2,#22d3ee); }}
    .progress-fill.tone-gold{{ background:linear-gradient(90deg,#d97706,#fbbf24); }}
    .progress-fill.tone-violet{{ background:linear-gradient(90deg,#7c3aed,#a78bfa); }}
    .clean-list{{ margin:0; padding-left:18px; color:var(--muted); line-height:1.8; font-size:13px; }}
    .kv-list{{ display:grid; gap:9px; }}
    .kv-row{{ display:flex; justify-content:space-between; gap:16px; padding-bottom:9px; border-bottom:1px solid var(--line); }}
    .kv-row:last-child{{ border-bottom:0; padding-bottom:0; }}
    .kv-label{{ color:var(--muted); font-size:13px; white-space:nowrap; }}
    .kv-value{{ font-size:13px; font-weight:800; text-align:right; word-break:break-word; }}
    .command-section{{ margin-top:14px; }}
    .section-label{{ color:#7a9ab8; font-size:11px; font-weight:900; letter-spacing:.08em; text-transform:uppercase; margin-bottom:8px; }}
    .ghost-field{{ min-height:38px; display:inline-flex; align-items:center; padding:0 12px; border-radius:9px; border:1px solid var(--line); background:#f8fbff; color:#67809e; font-size:13px; white-space:nowrap; }}
    .ghost-field.wide{{ min-width:200px; }}
    .terminal-card{{ padding-bottom:20px; }}
    .terminal-shell{{ border-radius:14px; border:1px solid #1e3356; background:#0d1929; overflow:hidden; }}
    .terminal-head{{ display:flex; justify-content:space-between; gap:12px; padding:11px 16px; border-bottom:1px solid #1e3356; color:#c8d8f4; font-size:13px; font-weight:800; }}
    .terminal-head span{{ color:#8aaad4; font-weight:600; }}
    .terminal-stage,.terminal-placeholder,iframe{{ min-height:520px; }}
    .terminal-placeholder{{ display:grid; place-items:center; text-align:center; padding:28px; color:#8aaad4; }}
    .terminal-placeholder-inner{{ max-width:360px; }}
    .terminal-placeholder h3{{ margin:0 0 10px; color:#c8d8f4; font-size:19px; }}
    .terminal-placeholder p{{ margin:0 0 16px; line-height:1.8; font-size:13px; }}
    iframe{{ display:block; width:100%; border:0; background:#0d1929; }}
    code{{ padding:2px 6px; border-radius:6px; background:#eff6ff; color:#1d4ed8; font-family:Consolas,"SFMono-Regular",monospace; font-size:.88em; }}
    @media (max-width:1100px){{
      .two-col,.three-up,.split-grid,.status-panel{{ grid-template-columns:1fr; }}
    }}
    @media (max-width:720px){{
      .summary-strip,.feature-grid,.resource-grid{{ grid-template-columns:repeat(2,1fr); }}
      .stats-grid{{ grid-template-columns:repeat(2,1fr); }}
    }}
    @media (max-width:480px){{
      .wrap{{ padding:14px 14px 36px; }} .page-header{{ padding:16px 14px 0; }}
      .app-header{{ padding:0 14px; gap:10px; }} .app-brand-sub{{ display:none; }}
      .summary-strip,.feature-grid,.resource-grid{{ grid-template-columns:1fr; }}
      .terminal-stage,.terminal-placeholder,iframe{{ min-height:400px; }}
    }}
  </style>
</head>
<body>
  <header class="app-header">
    <a class="app-brand" href="./">
      <div class="app-brand-badge">{brand_svg}</div>
      <div class="app-brand-text">
        <span class="app-brand-name">OpenClaw</span>
        <span class="app-brand-sub">HAOS Add-on</span>
      </div>
    </a>
    <nav class="app-nav">{nav_home}{nav_config}{nav_commands}{nav_logs}</nav>
    <div class="app-meta"><span class="version-chip">{addon_version}</span></div>
  </header>
  <div class="page-header">
    <div class="page-eyebrow">OpenClaw Assistant</div>
    <h1 class="page-title">{title}</h1>
    <p class="page-sub muted">{subtitle}</p>
  </div>
  <div class="wrap">
    {content}
  </div>
  <script>
    const configuredGatewayUrl = {gateway_url};
    const httpsPort = {https_port};
    const terminalState = {{ loaded:false, loading:false, pendingCommand:null }};
    const initialTerminalStageHtml = document.getElementById("terminalStage") ? document.getElementById("terminalStage").innerHTML : "";
    let gatewayTokenValue = "";
    function appUrl(relativePath) {{ return new URL(relativePath, location.href).toString(); }}
    async function loadPanel(url, targetId) {{
      const target = document.getElementById(targetId);
      if (!target) return;
      const ctrl = new AbortController();
      const timer = setTimeout(() => ctrl.abort(), 8000);
      try {{
        const response = await fetch(url, {{ credentials: "same-origin", signal: ctrl.signal }});
        clearTimeout(timer);
        if (!response.ok) throw new Error(`HTTP ${{response.status}}`);
        target.innerHTML = await response.text();
      }} catch (error) {{
        clearTimeout(timer);
        if (error.name !== "AbortError") {{
          target.innerHTML = `<p class="muted">面板加载失败：${{error.message}}</p>`;
        }}
      }}
    }}
    function refreshPanels() {{
      if (document.visibilityState !== "visible") return;
      if (document.getElementById("healthPanel")) loadPanel(appUrl("./partials/health"), "healthPanel");
      if (document.getElementById("diagPanel")) loadPanel(appUrl("./partials/diag"), "diagPanel");
    }}
    function nativeGatewayUrl() {{
      if (configuredGatewayUrl && configuredGatewayUrl.trim() !== "") return configuredGatewayUrl;
      return `https://${{location.hostname}}:${{httpsPort}}/`;
    }}
    function withTokenHash(url, token) {{
      if (!url || !token) return url;
      return String(url).replace(/#.*$/, '') + '#token=' + encodeURIComponent(token);
    }}
    function syncGatewayLink(anchor) {{
      const link = anchor || document.getElementById("ocGatewayLink");
      if (!link) return;
      link.href = nativeGatewayUrl();
    }}
    async function fetchGatewayToken() {{
      if (gatewayTokenValue) return gatewayTokenValue;
      const response = await fetch(appUrl('./token'), {{ credentials: 'same-origin', cache: 'no-cache' }});
      if (!response.ok) throw new Error(`token-${{response.status}}`);
      const text = (await response.text()).trim();
      if (!text) throw new Error('empty-token');
      gatewayTokenValue = text;
      return gatewayTokenValue;
    }}
    async function waitForGatewayControlReady(timeoutMs = 150000) {{
      const deadline = Date.now() + timeoutMs;
      let stable = 0;
      while (Date.now() < deadline) {{
        try {{
          const response = await fetch(appUrl('./control-readyz'), {{ credentials: 'same-origin', cache: 'no-cache' }});
          if (response.ok) {{
            stable += 1;
            if (stable >= 2) {{
              await new Promise((resolve) => window.setTimeout(resolve, 2500));
              return true;
            }}
          }} else {{
            stable = 0;
          }}
        }} catch (_) {{
          stable = 0;
        }}
        await new Promise((resolve) => window.setTimeout(resolve, 3000));
      }}
      return false;
    }}
    function focusTerminal() {{
      const shell = document.querySelector(".terminal-shell");
      if (shell) shell.scrollIntoView({{ behavior: "smooth", block: "start" }});
      const frame = document.getElementById("termFrame");
      if (frame && frame.contentWindow) frame.contentWindow.postMessage({{ type: "openclaw-focus-terminal" }}, "*");
    }}
    function ensureTerminalLoaded() {{
      if (terminalState.loaded || terminalState.loading) return;
      const stage = document.getElementById("terminalStage");
      if (!stage) return;
      terminalState.loading = true;
      stage.innerHTML = `<iframe id="termFrame" src="${{appUrl('./terminal/')}}" title="terminal"></iframe>`;
      const frame = document.getElementById("termFrame");
      frame.addEventListener("load", function () {{
        terminalState.loading = false;
        terminalState.loaded = true;
        focusTerminal();
        if (terminalState.pendingCommand) {{
          const next = terminalState.pendingCommand;
          terminalState.pendingCommand = null;
          window.setTimeout(() => injectTerminalCommand(next), 180);
        }}
      }}, {{ once: true }});
    }}
    function injectTerminalCommand(command) {{
      if (!command) return;
      if (!terminalState.loaded) {{
        terminalState.pendingCommand = command;
        ensureTerminalLoaded();
        return;
      }}
      const frame = document.getElementById("termFrame");
      if (!frame || !frame.contentWindow) return;
      frame.contentWindow.postMessage({{ type: "openclaw-run-command", command }}, "*");
    }}
    window.ocCloseTerminal = function () {{
      const stage = document.getElementById("terminalStage");
      if (!stage) return;
      stage.innerHTML = initialTerminalStageHtml;
      terminalState.loaded = false;
      terminalState.loading = false;
      terminalState.pendingCommand = null;
    }};
    function writeGatewayLoadingPage(popup) {{
      if (!popup || !popup.document) return;
      popup.document.open();
      popup.document.write(`<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>OpenClaw Gateway</title>
  <style>
    :root {{
      color-scheme: light;
    }}
    body {{
      margin: 0;
      min-height: 100vh;
      display: flex;
      align-items: center;
      justify-content: center;
      background: linear-gradient(180deg, #eaf1ff 0%, #f4f7ff 100%);
      color: #16345f;
      font-family: "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif;
    }}
    .card {{
      width: min(440px, calc(100vw - 40px));
      padding: 28px 26px;
      border-radius: 22px;
      border: 1px solid rgba(22, 52, 95, .12);
      background: rgba(255,255,255,.95);
      box-shadow: 0 20px 48px rgba(18, 40, 75, .12);
      text-align: center;
    }}
    h1 {{
      margin: 0 0 10px;
      font-size: 24px;
    }}
    p {{
      margin: 0;
      color: #58729a;
      line-height: 1.7;
      font-size: 14px;
    }}
  </style>
</head>
<body>
  <div class="card">
    <h1>正在打开 OpenClaw Gateway</h1>
    <p>正在等待原生控制台就绪，请稍候几秒。</p>
  </div>
</body>
</html>`);
      popup.document.close();
      try {{ popup.opener = null; }} catch (_) {{}}
    }}
    window.ocOpenGateway = async function () {{
      const popup = window.open("", "_blank");
      writeGatewayLoadingPage(popup);
      const targetUrl = nativeGatewayUrl();
      await waitForGatewayControlReady();
      let finalUrl = targetUrl;
      try {{
        const token = await fetchGatewayToken();
        finalUrl = withTokenHash(targetUrl, token);
      }} catch (_) {{}}
      if (popup) {{
        popup.location.replace(finalUrl);
      }} else {{
        window.open(finalUrl, "_blank", "noopener,noreferrer");
      }}
    }};
    window.ocOpenGatewayLink = async function (event, anchor) {{
      if (event) event.preventDefault();
      syncGatewayLink(anchor);
      await window.ocOpenGateway();
      return false;
    }};
    window.ocOpenTerminalWindow = function (command) {{
      const targetUrl = new URL(appUrl("./terminal/"));
      if (typeof command === "string" && command.trim()) targetUrl.searchParams.set("command", command);
      window.open(targetUrl.toString(), "_blank", "noopener,noreferrer");
    }};
    window.ocLoadTerminal = function () {{ ensureTerminalLoaded(); window.setTimeout(focusTerminal, 120); }};
    window.ocRunCommand = function (command) {{ injectTerminalCommand(command || ""); }};
    window.ocRunButton = function (button) {{
      if (!button) return;
      const command = button.getAttribute("data-command") || "";
      injectTerminalCommand(command);
    }};
    window.ocRunSensitive = function (command, warning) {{
      if (!confirm(warning)) return;
      injectTerminalCommand(command);
    }};
    window.ocRunCustomCommand = function () {{
      const inp = document.getElementById("customCmdInput");
      if (!inp || !inp.value.trim()) return;
      injectTerminalCommand(inp.value.trim());
      inp.value = "";
    }};
    function ocSetFormStatus(id, text, ok) {{
      const el = document.getElementById(id);
      if (!el) return;
      el.textContent = text || "";
      el.classList.remove("ok", "err");
      if (ok === true) el.classList.add("ok");
      if (ok === false) el.classList.add("err");
    }}
    async function ocPostJson(url, payload) {{
      const response = await fetch(appUrl(url), {{
        method: "POST",
        credentials: "same-origin",
        headers: {{ "Content-Type": "application/json" }},
        body: JSON.stringify(payload)
      }});
      return response.json();
    }}
    function ocSyncProviderMeta(kind) {{
      const select = document.getElementById(kind + "Provider");
      if (!select) return;
      const option = select.options[select.selectedIndex];
      const docsHref = option ? (option.dataset.docs || "") : "";
      const consoleHref = option ? (option.dataset.console || "") : "";
      const docsLink = document.getElementById(kind + "DocsLink");
      const consoleLink = document.getElementById(kind + "ConsoleLink");
      if (docsLink && docsHref) docsLink.href = docsHref;
      if (consoleLink) {{
        if (consoleHref) {{
          consoleLink.href = consoleHref;
          consoleLink.style.display = "";
        }} else {{
          consoleLink.removeAttribute("href");
          consoleLink.style.display = "none";
        }}
      }}
      if (kind === "memory") {{
        const isLocal = select.value === "local";
        const localGroup = document.getElementById("memoryLocalGroup");
        const remoteGroup = document.getElementById("memoryRemoteGroup");
        if (localGroup) localGroup.style.display = isLocal ? "grid" : "none";
        if (remoteGroup) remoteGroup.style.display = isLocal ? "none" : "grid";
      }}
    }}
    window.ocSaveWebSearchConfig = async function () {{
      ocSetFormStatus("webSaveStatus", "正在保存…");
      try {{
        const data = await ocPostJson("./action/config-web-search", {{
          enabled: !!document.getElementById("webEnabled")?.checked,
          provider: document.getElementById("webProvider")?.value || "auto",
          model: document.getElementById("webModel")?.value || "",
          base_url: document.getElementById("webBaseUrl")?.value || "",
          api_key: document.getElementById("webApiKey")?.value || "",
          clear_api_key: !!document.getElementById("webClearApiKey")?.checked
        }});
        ocSetFormStatus("webSaveStatus", data.message || "已保存", !!data.ok);
        if (data.ok) window.setTimeout(function() {{ location.reload(); }}, 800);
      }} catch (error) {{
        ocSetFormStatus("webSaveStatus", "保存失败：" + (error.message || error), false);
      }}
    }};
    window.ocSaveMemorySearchConfig = async function () {{
      ocSetFormStatus("memorySaveStatus", "正在保存…");
      try {{
        const data = await ocPostJson("./action/config-memory-search", {{
          enabled: !!document.getElementById("memoryEnabled")?.checked,
          provider: document.getElementById("memoryProvider")?.value || "auto",
          model: document.getElementById("memoryModel")?.value || "",
          fallback: document.getElementById("memoryFallback")?.value || "",
          base_url: document.getElementById("memoryBaseUrl")?.value || "",
          api_key: document.getElementById("memoryApiKey")?.value || "",
          clear_api_key: !!document.getElementById("memoryClearApiKey")?.checked,
          local_model_path: document.getElementById("memoryLocalModelPath")?.value || ""
        }});
        ocSetFormStatus("memorySaveStatus", data.message || "已保存", !!data.ok);
        if (data.ok) window.setTimeout(function() {{ location.reload(); }}, 800);
      }} catch (error) {{
        ocSetFormStatus("memorySaveStatus", "保存失败：" + (error.message || error), false);
      }}
    }};
    window.ocSaveModelConfig = async function () {{
      ocSetFormStatus("modelSaveStatus", "正在保存…");
      try {{
        const data = await ocPostJson("./action/config-model", {{
          primary_model: document.getElementById("primaryModel")?.value || "",
          fallback_models: document.getElementById("fallbackModels")?.value || ""
        }});
        ocSetFormStatus("modelSaveStatus", data.message || "已保存", !!data.ok);
        if (data.ok) window.setTimeout(function() {{ location.reload(); }}, 800);
      }} catch (error) {{
        ocSetFormStatus("modelSaveStatus", "保存失败：" + (error.message || error), false);
      }}
    }};
    document.addEventListener("visibilitychange", refreshPanels);
    syncGatewayLink();
    ocSyncProviderMeta("web");
    ocSyncProviderMeta("memory");
    window.setInterval(refreshPanels, 45000);
    window.setTimeout(refreshPanels, 120);
  </script>
  <script>
    (function() {{
      var banner = document.getElementById("pairing-banner");
      if (!banner) return;

      function renderBanner(pairs) {{
        if (!pairs || pairs.length === 0) {{
          banner.style.display = "none";
          banner.innerHTML = "";
          return;
        }}
        var html = '<div class="notice-badge warn" style="margin-bottom:8px">';
        html += '<strong>有 ' + pairs.length + ' 台设备请求配对</strong>';
        html += '<div style="margin-top:8px;display:flex;flex-wrap:wrap;gap:8px;">';
        pairs.forEach(function(p) {{
          html += '<span style="display:inline-flex;align-items:center;gap:6px;background:#fff;border:1px solid #fcd34d;border-radius:8px;padding:4px 10px;font-size:13px;">';
          html += '<span>' + escapeHtml(p.deviceName) + '</span>';
          html += '<button class="btn btn-action" style="padding:2px 10px;font-size:12px;" onclick="ocApprovePair(\'' + escapeHtml(p.requestId) + '\',this)">批准</button>';
          html += '</span>';
        }});
        html += '</div></div>';
        banner.innerHTML = html;
        banner.style.display = "block";
      }}

      function escapeHtml(str) {{
        return String(str).replace(/[&<>"']/g, function(c) {{
          return ({{"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"}})[c];
        }});
      }}

      window.ocApprovePair = function(requestId, btn) {{
        btn.disabled = true;
        btn.textContent = "处理中…";
        fetch(appUrl('./action/pair-approve'), {{
          method: "POST",
          headers: {{"Content-Type": "application/json"}},
          body: JSON.stringify({{request_id: requestId}})
        }})
        .then(function(r) {{ return r.json(); }})
        .then(function(data) {{
          if (data.ok) {{
            btn.closest("span").innerHTML = '<span style="color:#15803d;font-weight:700;">✓ ' + escapeHtml(data.message) + '</span>';
          }} else {{
            btn.disabled = false;
            btn.textContent = "重试";
            btn.title = data.message;
          }}
        }})
        .catch(function() {{
          btn.disabled = false;
          btn.textContent = "重试";
        }});
      }};

      var es = new EventSource(appUrl('./events/pairing'));
      es.addEventListener("pairing", function(e) {{
        try {{ renderBanner(JSON.parse(e.data)); }} catch(ex) {{}}
      }});
      es.onerror = function() {{
        // SSE 断开后浏览器会自动重连，无需手动处理
      }};
    }})();
  </script>
</body>
</html>"#,
        nav_home = nav_home,
        nav_config = nav_config,
        nav_commands = nav_commands,
        nav_logs = nav_logs,
        brand_svg = openclaw_brand_svg("brand-mark"),
        title = title,
        subtitle = subtitle,
        addon_version = config.addon_version,
        content = content,
        gateway_url = gateway_url,
        https_port = config.https_port,
    )))
}

// ─── 配对轮询后台任务 ────────────────────────────────────────────────────────

async fn pairing_poll_task(pairing: PairingState) {
    wait_for_pairing_backend_ready().await;

    // 成功后用 10s 间隔；失败时指数退避（最长 120s），成功后重置。
    const POLL_SECS: u64 = 10;
    const MAX_BACKOFF_SECS: u64 = 120;
    let mut backoff = POLL_SECS;

    loop {
        let token = PageConfig::from_env().gateway_token;
        if !token.is_empty() {
            match gateway_ws::list_pending_pairs(&token).await {
                Some(new_pairs) => {
                    backoff = POLL_SECS; // 成功后重置间隔
                    let changed = {
                        let old = pairing.pairs.read().await;
                        old.len() != new_pairs.len()
                            || old.iter().zip(new_pairs.iter()).any(|(a, b)| a.request_id != b.request_id)
                    };
                    if changed {
                        *pairing.pairs.write().await = new_pairs;
                        let _ = pairing.notify.send(());
                    }
                }
                None => {
                    // 失败（gateway 未就绪）：指数退避，减少 gateway 日志噪音
                    backoff = (backoff * 2).min(MAX_BACKOFF_SECS);
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(backoff)).await;
    }
}

fn gateway_internal_port_from_env() -> u16 {
    env::var("GATEWAY_INTERNAL_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(18790)
}

fn browser_control_port_from_gateway_port(gateway_port: u16) -> u16 {
    gateway_port.saturating_add(2)
}

async fn wait_for_pairing_backend_ready() {
    // 先等 gateway 主进程起来，再用本地 TCP 探针等待 browser/acpx 控制面 ready，
    // 避免 device.pair.list 在 127.0.0.1:18790 上过早发起 WebSocket 连接。
    const INITIAL_DELAY_SECS: u64 = 20;
    const POLL_SECS: u64 = 5;
    const PROBE_TIMEOUT_MS: u64 = 800;
    const STABLE_SUCCESSES: u8 = 2;
    const SETTLE_SECS: u64 = 4;

    tokio::time::sleep(Duration::from_secs(INITIAL_DELAY_SECS)).await;

    let target = format!(
        "127.0.0.1:{}",
        browser_control_port_from_gateway_port(gateway_internal_port_from_env())
    );
    let mut consecutive_successes = 0u8;

    loop {
        let ready = timeout(Duration::from_millis(PROBE_TIMEOUT_MS), TcpStream::connect(&target))
            .await
            .map(|result| result.is_ok())
            .unwrap_or(false);

        if ready {
            consecutive_successes = consecutive_successes.saturating_add(1);
            if consecutive_successes >= STABLE_SUCCESSES {
                tokio::time::sleep(Duration::from_secs(SETTLE_SECS)).await;
                return;
            }
        } else {
            consecutive_successes = 0;
        }

        tokio::time::sleep(Duration::from_secs(POLL_SECS)).await;
    }
}

// ─── SSE：配对事件推送 ────────────────────────────────────────────────────────

async fn wait_for_pairing_backend_ready_on_demand(timeout_limit: Duration) -> bool {
    const POLL_SECS: u64 = 5;
    const PROBE_TIMEOUT_MS: u64 = 800;
    const STABLE_SUCCESSES: u8 = 2;
    const SETTLE_SECS: u64 = 4;
    let deadline = tokio::time::Instant::now() + timeout_limit;
    let target = format!(
        "127.0.0.1:{}",
        browser_control_port_from_gateway_port(gateway_internal_port_from_env())
    );
    let mut consecutive_successes = 0u8;

    while tokio::time::Instant::now() < deadline {
        let ready = timeout(Duration::from_millis(PROBE_TIMEOUT_MS), TcpStream::connect(&target))
            .await
            .map(|result| result.is_ok())
            .unwrap_or(false);

        if ready {
            consecutive_successes = consecutive_successes.saturating_add(1);
            if consecutive_successes >= STABLE_SUCCESSES {
                tokio::time::sleep(Duration::from_secs(SETTLE_SECS)).await;
                return true;
            }
        } else {
            consecutive_successes = 0;
        }

        tokio::time::sleep(Duration::from_secs(POLL_SECS)).await;
    }

    false
}

async fn pairing_sse(
    State(state): State<AppState>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let pairing = state.pairing.clone();
    let initial_pairs = pairing.pairs.read().await.clone();
    let token = PageConfig::from_env().gateway_token;

    let initial = build_pairing_event(&initial_pairs);
    let s = stream::once(async move { Ok(initial) }).chain(stream::unfold(
        (pairing, initial_pairs, token, false, 10u64),
        |(pairing, last_pairs, token, ready, backoff)| async move {
            let mut next_pairs = last_pairs.clone();
            let mut next_ready = ready;
            let mut next_backoff = backoff;

            tokio::time::sleep(Duration::from_secs(next_backoff)).await;

            if !token.is_empty() {
                if next_ready || wait_for_pairing_backend_ready_on_demand(Duration::from_secs(150)).await {
                    next_ready = true;
                    match gateway_ws::list_pending_pairs(&token).await {
                        Some(fresh_pairs) => {
                            next_backoff = if fresh_pairs.is_empty() { 30 } else { 10 };
                            next_pairs = fresh_pairs.clone();
                            *pairing.pairs.write().await = fresh_pairs;
                        }
                        None => {
                            next_backoff = (next_backoff * 2).min(120);
                        }
                    }
                } else {
                    next_backoff = 30;
                }
            }

            let event = build_pairing_event(&next_pairs);
            Some((Ok(event), (pairing, next_pairs, token, next_ready, next_backoff)))
        },
    ));

    Sse::new(s).keep_alive(
        axum::response::sse::KeepAlive::new().interval(Duration::from_secs(25)),
    )
}

fn build_pairing_event(pairs: &[PendingPair]) -> Event {
    let data: Vec<serde_json::Value> = pairs
        .iter()
        .map(|p| serde_json::json!({ "requestId": p.request_id, "deviceName": p.device_name }))
        .collect();
    Event::default()
        .event("pairing")
        .data(serde_json::to_string(&data).unwrap_or_else(|_| "[]".to_string()))
}

// ─── POST /action/pair-approve ────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct ApproveRequest {
    request_id: String,
}

#[derive(serde::Deserialize)]
struct SaveWebSearchRequest {
    enabled: bool,
    provider: String,
    model: String,
    base_url: String,
    api_key: String,
    clear_api_key: bool,
}

#[derive(serde::Deserialize)]
struct SaveMemorySearchRequest {
    enabled: bool,
    provider: String,
    model: String,
    fallback: String,
    base_url: String,
    api_key: String,
    clear_api_key: bool,
    local_model_path: String,
}

#[derive(serde::Deserialize)]
struct SaveModelRequest {
    primary_model: String,
    fallback_models: String,
}

fn clear_known_web_provider_overrides(config: &mut serde_json::Value) {
    for (provider, _, _, _) in WEB_PROVIDER_META {
        let Some(plugin) = web_provider_plugin(provider) else {
            continue;
        };
        remove_config_value_path(
            config,
            &["plugins", "entries", plugin, "config", "webSearch", "apiKey"],
        );
        remove_config_value_path(
            config,
            &["plugins", "entries", plugin, "config", "webSearch", "baseUrl"],
        );
        remove_config_value_path(
            config,
            &["plugins", "entries", plugin, "config", "webSearch", "model"],
        );
    }
}

fn apply_web_search_overlay(config: &mut serde_json::Value, body: &SaveWebSearchRequest) {
    set_bool_path(config, &["tools", "web", "search", "enabled"], body.enabled);
    if body.provider.trim().is_empty() || body.provider.trim() == "auto" {
        remove_config_value_path(config, &["tools", "web", "search", "provider"]);
    } else {
        set_or_remove_string_path(config, &["tools", "web", "search", "provider"], &body.provider);
    }
    clear_known_web_provider_overrides(config);
    if let Some(plugin) = web_provider_plugin(body.provider.trim()) {
        set_or_remove_string_path(
            config,
            &["plugins", "entries", plugin, "config", "webSearch", "baseUrl"],
            &body.base_url,
        );
        set_or_remove_string_path(
            config,
            &["plugins", "entries", plugin, "config", "webSearch", "model"],
            &body.model,
        );
        if body.clear_api_key {
            remove_config_value_path(
                config,
                &["plugins", "entries", plugin, "config", "webSearch", "apiKey"],
            );
        } else if !body.api_key.trim().is_empty() {
            set_or_remove_string_path(
                config,
                &["plugins", "entries", plugin, "config", "webSearch", "apiKey"],
                &body.api_key,
            );
        }
    }
}

fn apply_memory_search_overlay(config: &mut serde_json::Value, body: &SaveMemorySearchRequest) {
    set_bool_path(
        config,
        &["agents", "defaults", "memorySearch", "enabled"],
        body.enabled,
    );
    if body.provider.trim().is_empty() || body.provider.trim() == "auto" {
        remove_config_value_path(config, &["agents", "defaults", "memorySearch", "provider"]);
    } else {
        set_or_remove_string_path(
            config,
            &["agents", "defaults", "memorySearch", "provider"],
            &body.provider,
        );
    }
    set_or_remove_string_path(
        config,
        &["agents", "defaults", "memorySearch", "model"],
        &body.model,
    );
    if body.fallback.trim().is_empty() || body.fallback.trim() == "none" {
        remove_config_value_path(config, &["agents", "defaults", "memorySearch", "fallback"]);
    } else {
        set_or_remove_string_path(
            config,
            &["agents", "defaults", "memorySearch", "fallback"],
            &body.fallback,
        );
    }
    if body.provider.trim() == "local" {
        set_or_remove_string_path(
            config,
            &["agents", "defaults", "memorySearch", "local", "modelPath"],
            &body.local_model_path,
        );
        remove_config_value_path(
            config,
            &["agents", "defaults", "memorySearch", "remote", "baseUrl"],
        );
        if body.clear_api_key {
            remove_config_value_path(
                config,
                &["agents", "defaults", "memorySearch", "remote", "apiKey"],
            );
        }
    } else {
        remove_config_value_path(
            config,
            &["agents", "defaults", "memorySearch", "local", "modelPath"],
        );
        set_or_remove_string_path(
            config,
            &["agents", "defaults", "memorySearch", "remote", "baseUrl"],
            &body.base_url,
        );
        if body.clear_api_key {
            remove_config_value_path(
                config,
                &["agents", "defaults", "memorySearch", "remote", "apiKey"],
            );
        } else if !body.api_key.trim().is_empty() {
            set_or_remove_string_path(
                config,
                &["agents", "defaults", "memorySearch", "remote", "apiKey"],
                &body.api_key,
            );
        }
    }
}

fn apply_model_overlay(config: &mut serde_json::Value, body: &SaveModelRequest) {
    set_or_remove_string_path(
        config,
        &["agents", "defaults", "model", "primary"],
        &body.primary_model,
    );
    let fallbacks = parse_csv_values(&body.fallback_models);
    if fallbacks.is_empty() {
        remove_config_value_path(config, &["agents", "defaults", "model", "fallbacks"]);
    } else {
        set_config_value_path(
            config,
            &["agents", "defaults", "model", "fallbacks"],
            serde_json::Value::Array(
                fallbacks
                    .into_iter()
                    .map(serde_json::Value::String)
                    .collect(),
            ),
        );
    }
}

async fn pair_approve(
    State(state): State<AppState>,
    Json(body): Json<ApproveRequest>,
) -> impl IntoResponse {
    let token = PageConfig::from_env().gateway_token;
    if token.is_empty() {
        return Json(serde_json::json!({ "ok": false, "message": "Gateway token 未配置" }));
    }
    let (ok, message) = gateway_ws::approve_pair(&token, &body.request_id).await;
    // 批准后立即刷新配对列表
    if ok {
        if let Some(new_pairs) = gateway_ws::list_pending_pairs(&token).await {
            *state.pairing.pairs.write().await = new_pairs;
        }
    }
    Json(serde_json::json!({ "ok": ok, "message": message }))
}

// ─── 页面 handlers ────────────────────────────────────────────────────────────

async fn save_web_search_config(Json(body): Json<SaveWebSearchRequest>) -> impl IntoResponse {
    let mut config = load_panel_config_mutable();
    apply_web_search_overlay(&mut config, &body);
    match save_panel_config_value(&config) {
        Ok(()) => Json(serde_json::json!({
            "ok": true,
            "message": "Web Search 配置已保存，重启插件后会应用到 OpenClaw",
            "restart_required": true
        })),
        Err(err) => Json(serde_json::json!({ "ok": false, "message": err })),
    }
}

async fn save_memory_search_config(Json(body): Json<SaveMemorySearchRequest>) -> impl IntoResponse {
    let mut config = load_panel_config_mutable();
    apply_memory_search_overlay(&mut config, &body);
    match save_panel_config_value(&config) {
        Ok(()) => Json(serde_json::json!({
            "ok": true,
            "message": "Memory Search 配置已保存，重启插件后会应用到 OpenClaw",
            "restart_required": true
        })),
        Err(err) => Json(serde_json::json!({ "ok": false, "message": err })),
    }
}

async fn save_model_config(Json(body): Json<SaveModelRequest>) -> impl IntoResponse {
    let mut config = load_panel_config_mutable();
    apply_model_overlay(&mut config, &body);
    match save_panel_config_value(&config) {
        Ok(()) => Json(serde_json::json!({
            "ok": true,
            "message": "模型配置已保存，重启插件后会应用到 OpenClaw",
            "restart_required": true
        })),
        Err(err) => Json(serde_json::json!({ "ok": false, "message": err })),
    }
}

async fn index(State(state): State<AppState>) -> impl IntoResponse {
    let config = PageConfig::from_env();
    let guard = state.cache.read().await;
    let (snapshot, health_ok) = if let Some(c) = guard.as_ref() {
        let result = (c.snapshot.clone(), c.health_ok);
        drop(guard);
        result
    } else {
        drop(guard);
        tokio::join!(collect_system_snapshot(), fetch_openclaw_health())
    };
    let pending_devices = state.pairing.pairs.read().await.len();
    render_shell(
        &config,
        NavPage::Home,
        "OpenClawHAOSAddon-Rust",
        "查看 OpenClaw 当前是否正常运行、各服务进程状态，以及系统资源占用情况。",
        &home_content(&config, &snapshot, health_ok, pending_devices),
    )
}

async fn config_page(State(state): State<AppState>) -> impl IntoResponse {
    let _ = state;
    let config = PageConfig::from_env();
    render_shell(
        &config,
        NavPage::Config,
        "基础配置",
        "查看插件当前的访问方式、数据目录位置，以及 Web 搜索、记忆搜索等能力的启用状态。",
        &config_content_v2(&config),
    )
}

async fn commands_page(State(state): State<AppState>) -> impl IntoResponse {
    let _ = state;
    let config = PageConfig::from_env();
    render_shell(
        &config,
        NavPage::Commands,
        "命令行工作区",
        "在这里重启服务、批准设备配对、执行诊断，或直接打开终端操作。",
        &commands_content_native(&config),
    )
}

async fn logs_page(State(state): State<AppState>) -> impl IntoResponse {
    let _ = state;
    let config = PageConfig::from_env();
    render_shell(
        &config,
        NavPage::Logs,
        "日志与诊断",
        "查看 OpenClaw 运行日志、执行诊断命令，快速定位异常原因。",
        &logs_content(),
    )
}

async fn health_partial(State(state): State<AppState>) -> impl IntoResponse {
    let _ = state;
    let config = PageConfig::from_env();
    let display_gateway_pid = tokio::task::spawn_blocking(|| {
        let gateway_pid = pid_value("openclaw-gateway");
        if gateway_pid != "-" {
            gateway_pid
        } else {
            pid_value("openclaw-node")
        }
    })
    .await
    .unwrap_or_else(|_| "-".to_string());

    Html(force_chinese_ui(format!(
        r#"<div class="eyebrow">运行摘要</div>
<h2>服务状态</h2>
<div class="kv-list">
  {access}
  {mode}
  {addon}
  {openclaw}
  {gateway_pid}
</div>"#,
        access = kv_row("访问模式", &config.access_mode),
        mode = kv_row("网关模式", &config.gateway_mode),
        addon = kv_row("Add-on 版本", &config.addon_version),
        openclaw = kv_row("OpenClaw 版本", &config.openclaw_version),
        gateway_pid = kv_row("Gateway PID", &display_gateway_pid),
    )))
}

async fn diag_partial(State(state): State<AppState>) -> impl IntoResponse {
    let _ = state;
    let config = PageConfig::from_env();
    Html(force_chinese_ui(format!(
        r#"<div class="eyebrow">能力摘要</div>
<h2>快速诊断</h2>
<div class="kv-list">
  {mcp}
  {web}
  {memory}
  {https}
</div>"#,
        mcp = kv_row("MCP", &mcp_status_display(&config)),
        web = kv_row("Web Search", display_value(&config.web_status)),
        memory = kv_row("Memory Search", display_value(&config.memory_status)),
        https = kv_row("HTTPS 端口", &config.https_port),
    )))
}

#[tokio::main]
async fn main() {
    let cache: Arc<RwLock<Option<CachedSnapshot>>> = Arc::new(RwLock::new(None));
    let pairing = PairingState::new();
    let cache_bg = cache.clone();
    let pairing_bg = pairing.clone();
    tokio::spawn(async move {
        loop {
            let (snapshot, health_ok) = tokio::join!(
                collect_system_snapshot(),
                fetch_openclaw_health(),
            );
            // pending_devices 由 pairing_poll_task 维护，这里读缓存即可
            let pending_devices = pairing_bg.pairs.read().await.len();
            *cache_bg.write().await = Some(CachedSnapshot { snapshot, health_ok, pending_devices });
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
    let app_state = AppState { cache, pairing };

    let app = Router::new()
        .route("/", get(index))
        .route("/config", get(config_page))
        .route("/commands", get(commands_page))
        .route("/logs", get(logs_page))
        .route("/partials/health", get(health_partial))
        .route("/partials/diag", get(diag_partial))
        .route("/events/pairing", get(pairing_sse))
        .route("/action/pair-approve", post(pair_approve))
        .route("/action/config-web-search", post(save_web_search_config))
        .route("/action/config-memory-search", post(save_memory_search_config))
        .route("/action/config-model", post(save_model_config))
        .with_state(app_state);

    let port = env::var("UI_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(48101);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind ui listener");
    println!("haos-ui: listening on http://{addr}");
    axum::serve(listener, app).await.expect("serve ui");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_page_config() -> PageConfig {
        PageConfig {
            addon_version: "2026.04.03.8".to_string(),
            access_mode: "lan_https".to_string(),
            gateway_mode: "local".to_string(),
            gateway_url: String::new(),
            openclaw_version: "2026.4.2".to_string(),
            https_port: "18789".to_string(),
            openclaw_config_path: "/config/.openclaw/openclaw.json".to_string(),
            openclaw_state_dir: "/config/.openclaw".to_string(),
            openclaw_workspace_dir: "/config/.openclaw/workspace".to_string(),
            openclaw_runtime_dir: "/run/openclaw-rs".to_string(),
            mcporter_home_dir: "/config/.mcporter".to_string(),
            mcporter_config_path: "/config/.mcporter/mcporter.json".to_string(),
            backup_dir: "/share/openclaw-backup/latest".to_string(),
            cert_dir: "/config/certs".to_string(),
            mcp_status: "enabled".to_string(),
            web_status: "firecrawl".to_string(),
            web_provider: "firecrawl".to_string(),
            web_enabled: true,
            web_base_url: "https://api.firecrawl.dev".to_string(),
            web_model: "firecrawl-search-v1".to_string(),
            web_api_configured: true,
            memory_status: "x_search".to_string(),
            memory_provider: "openai".to_string(),
            memory_enabled: true,
            memory_model: "text-embedding-3-large".to_string(),
            memory_fallback: "none".to_string(),
            memory_base_url: String::new(),
            memory_local_model_path: String::new(),
            memory_api_configured: true,
            current_model: "gpt-4o".to_string(),
            mcp_endpoint_count: 0,
            gateway_token: String::new(),
        }
    }

    #[test]
    fn commands_page_uses_supervisor_restart_endpoint() {
        let html = commands_content_v2(&sample_page_config());

        assert!(html.contains("curl -fsS -X POST http://127.0.0.1:48099/action/restart"));
        assert!(!html.contains("openclaw gateway restart"));
    }

    #[test]
    fn commands_page_uses_real_npm_and_pairing_commands() {
        let html = commands_content_v2(&sample_page_config());

        assert!(html.contains("openclaw tui"));
        assert!(html.contains("ocOpenTerminalWindow(&quot;openclaw tui&quot;)"));
        assert!(html.contains("openclaw --version"));
        assert!(!html.contains("npm view openclaw version"));
        assert!(html.contains("openclaw devices approve --latest"));
        assert!(!html.contains("https://registry.npmjs.org/openclaw/latest"));
        assert!(!html.contains("onclick=\"ocRunCommand('openclaw tui')\""));
        assert!(html.contains("ocRunSensitive"));
        assert!(html.contains("ocRunCustomCommand"));
    }

    #[test]
    fn commands_page_surfaces_probe_and_config_paths() {
        let html = commands_content_v2(&sample_page_config());

        assert!(html.contains("http://127.0.0.1:48099/healthz"));
        assert!(html.contains("http://127.0.0.1:48099/readyz"));
        assert!(html.contains("/config/.openclaw/openclaw.json"));
        assert!(html.contains("/config/.mcporter/mcporter.json"));
        assert!(html.contains("/config/.openclaw/workspace"));
    }

    #[test]
    fn native_commands_page_stays_close_to_official_entrypoints() {
        let html = commands_content_native(&sample_page_config());

        assert!(html.contains("openclaw tui"));
        assert!(html.contains("openclaw onboard"));
        assert!(html.contains("openclaw health --json"));
        assert!(html.contains("openclaw status --deep"));
        assert!(!html.contains("ocRunCustomCommand"));
        assert!(!html.contains("/config/.openclaw/openclaw.json"));
        assert!(!html.contains("rsync -a --delete"));
    }

    #[test]
    fn config_page_exposes_web_memory_and_model_forms() {
        let html = config_content_v2(&sample_page_config());

        assert!(html.contains("ocSaveWebSearchConfig()"));
        assert!(html.contains("ocSaveMemorySearchConfig()"));
        assert!(html.contains("ocSaveModelConfig()"));
        assert!(html.contains("https://docs.openclaw.ai/tools/web"));
        assert!(html.contains("https://docs.openclaw.ai/reference/memory-config"));
        assert!(html.contains("id=\"modelSuggestions\""));
    }

    #[test]
    fn apply_model_overlay_writes_primary_and_fallbacks() {
        let mut config = serde_json::json!({});
        apply_model_overlay(
            &mut config,
            &SaveModelRequest {
                primary_model: "openai-codex/gpt-5.4".to_string(),
                fallback_models: "openai/gpt-5.4-mini, google/gemini-2.5-flash".to_string(),
            },
        );

        assert_eq!(
            string_path(&config, "agents.defaults.model.primary").as_deref(),
            Some("openai-codex/gpt-5.4")
        );
        assert_eq!(
            config["agents"]["defaults"]["model"]["fallbacks"]
                .as_array()
                .map(|values| values.len()),
            Some(2)
        );
    }

    #[test]
    fn render_shell_includes_fixed_aspect_brand_logo() {
        let config = sample_page_config();

        let Html(html) = render_shell(&config, NavPage::Home, "title", "subtitle", "<div></div>");

        assert!(html.contains("class=\"brand-mark\""));
        assert!(html.contains("preserveAspectRatio=\"xMidYMid meet\""));
        assert!(html.contains("./control-readyz"));
    }

    #[test]
    fn browser_control_port_tracks_gateway_port() {
        assert_eq!(18792, browser_control_port_from_gateway_port(18790));
    }

    #[test]
    fn provider_status_prefers_live_config_paths() {
        let config = serde_json::json!({
            "tools": {
                "web": {
                    "search": {
                        "provider": "firecrawl"
                    }
                }
            },
            "agents": {
                "defaults": {
                    "memorySearch": {
                        "provider": "openai"
                    }
                }
            }
        });

        let web = provider_status_from_config(
            Some(&config),
            &["tools.web.search.provider", "tools.webSearch.provider"],
            "disabled",
        );
        let memory = provider_status_from_config(
            Some(&config),
            &[
                "agents.defaults.memorySearch.provider",
                "agents.defaults.memory.search.provider",
            ],
            "disabled",
        );

        assert_eq!(web, "firecrawl");
        assert_eq!(memory, "openai");
    }

    #[test]
    fn service_badge_shows_state_and_pid() {
        let online = service_badge("Gateway", "1234");
        let offline = service_badge("Gateway", "-");

        assert!(online.contains("在线"));
        assert!(online.contains("PID 1234"));
        assert!(offline.contains("待启动"));
        assert!(offline.contains("未检测到 PID"));
    }
}
