use axum::{
    Router,
    extract::State,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json,
};
use std::{env, fs, net::SocketAddr, path::PathBuf, process::Command, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::RwLock,
    time::timeout,
};

#[derive(Clone)]
struct CachedSnapshot {
    snapshot: SystemSnapshot,
    health_ok: Option<bool>,
}

#[derive(Clone)]
struct AppState {
    cache: Arc<RwLock<Option<CachedSnapshot>>>,
}

#[derive(Clone, Debug)]
struct PageConfig {
    addon_version: String,
    access_mode: String,
    gateway_mode: String,
    gateway_url: String,
    openclaw_version: String,
    https_port: String,
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
        let current_model = config
            .as_ref()
            .and_then(|v| first_string_path(v, &[
                "agents.defaults.model.primary", // 实际格式：model 是对象，primary 是模型名
                "agents.defaults.llm.model",     // 备用路径
                "agents.defaults.model",         // 旧版本字符串兜底
            ]))
            .unwrap_or_else(|| "未配置".to_string());
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

async fn fetch_openclaw_health() -> Option<bool> {
    let mut stream = timeout(Duration::from_millis(1500), TcpStream::connect("127.0.0.1:48099"))
        .await
        .ok()?
        .ok()?;
    timeout(
        Duration::from_millis(1500),
        stream.write_all(b"GET /readyz HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"),
    )
    .await
    .ok()?
    .ok()?;

    let mut response = String::new();
    timeout(Duration::from_millis(1500), stream.read_to_string(&mut response))
        .await
        .ok()?
        .ok()?;

    Some(response.starts_with("HTTP/1.1 200") || response.starts_with("HTTP/1.0 200"))
}

fn read_first_line(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().and_then(|content| {
        content
            .lines()
            .next()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
    })
}

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

fn pid_value(name: &str) -> String {
    let path = format!("/run/openclaw-rs/{name}.pid");
    fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "-".to_string())
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

fn access_mode_label(mode: &str) -> &'static str {
    match mode {
        "local_only" => "仅本机访问",
        "lan_https" => "局域网 HTTPS",
        "lan_reverse_proxy" => "局域网反向代理",
        "tailnet_https" => "Tailnet HTTPS",
        "custom" => "自定义",
        _ => "当前模式",
    }
}

fn access_mode_help(mode: &str) -> &'static str {
    match mode {
        "local_only" => "只建议在本机或受控隧道下访问，适合保守部署。",
        "lan_https" => "这是当前 HAOS 最常见的推荐模式：浏览器通过 HTTPS 打开 Gateway，满足官方 secure context 要求。",
        "lan_reverse_proxy" => "适合你已有上层反向代理和证书，由上层统一做 HTTPS 与域名入口。",
        "tailnet_https" => "适合通过 Tailscale / Tailnet 远程访问，优先保证安全上下文和设备身份。",
        "custom" => "表示你正在使用自定义入口，请重点确认最终访问地址仍满足官方 HTTPS / localhost 要求。",
        _ => "访问模式决定浏览器如何进入 Gateway，也决定是否满足官方设备身份与 HTTPS 要求。",
    }
}

fn gateway_mode_help(mode: &str) -> &'static str {
    match mode {
        "remote" => "当前 Add-on 主要作为 HA 壳与入口，真正的 Gateway 在远端运行。",
        _ => "当前 Add-on 在本机启动并管理 OpenClaw Gateway，本页展示的状态和资源信息都以本机运行时为准。",
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

fn nav_link(active: NavPage, current: NavPage, href: &str, label: &str) -> String {
    let class_name = if active == current {
        "nav-link active"
    } else {
        "nav-link"
    };
    format!(r#"<a class="{class_name}" href="{href}">{label}</a>"#)
}

fn primary_link_button(label: &str, id: &str, onclick: &str) -> String {
    format!(
        r##"<a class="btn primary" id="{id}" href="#" target="_blank" rel="noopener noreferrer" onclick="{onclick}">{label}</a>"##
    )
}

fn terminal_window_button(label: &str, command: &str) -> String {
    let command = html_attr_escape(&js_string(command));
    format!(
        r#"<button class="btn secondary" type="button" onclick="ocOpenTerminalWindow({command})">{label}</button>"#
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

        <section class="card">
          <div class="card-head compact">
            <div>
              <div class="eyebrow">首次安装</div>
              <h3>推荐路径</h3>
            </div>
          </div>
          <ol class="clean-list">
            <li>先确认上方总状态为“运行正常”，再点击“打开网关”。</li>
            <li>首次进入原生 Control UI 时，按官方引导完成初始化、登录或模型配置。</li>
            <li>如果网页提示权限、身份或 token 问题，优先检查 HTTPS 入口，然后再重新打开页面。</li>
          </ol>
          <div class="mini-tip">如果你需要进一步初始化模型、Web Search 或 Memory Search，请继续到“基础配置”页完成保存，然后重启插件让配置生效。</div>
        </section>

        <div class="note-box">
          <strong>{access_title}</strong><br>
          {access_help}<br>
          <span class="muted">{gateway_mode_help}</span>
        </div>

        <div class="note-box">设备配对已收回原生入口处理。建议优先在原生 Control UI 中完成批准，或前往命令行页手动执行 <code>openclaw devices approve --latest</code>。<br>若设备连接时提示 token 错误或被拒绝，请在该设备浏览器中清除此站点的 Cookies 与本地存储，然后重新打开页面。</div>
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
            &format!("{online_count}/3"),
            "Gateway、Ingress、UI",
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
        open_cli = terminal_window_button("OpenClaw CLI", ""),
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
        access_title = access_mode_label(&config.access_mode),
        access_help = access_mode_help(&config.access_mode),
        gateway_mode_help = gateway_mode_help(&config.gateway_mode),
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

fn config_content_v2(config: &PageConfig) -> String {
    let panel_config_path = panel_config_path().display().to_string();
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
          <button class="btn secondary" type="button" onclick="ocOpenTerminalWindow('')">OpenClaw CLI</button>
          <a class="btn primary" href="./commands">进入命令页</a>
        </div>
    </div>
    <div class="notice-badge warn">
      保存后会写入 <code>{panel_config_path}</code>。要真正应用到 OpenClaw，请重启插件；这样不配置的时候不会改动 <code>openclaw.json</code>。
    </div>
    <div class="mini-tip">推荐顺序：先确认“打开网关”能进入原生页面，再在这里保存配置，最后重启插件并回到 Gateway 验证效果。</div>
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
      <h3>访问模式说明</h3>
      <div class="kv-list">
        {access_label}
        {access_help}
        {gateway_help}
      </div>
    </section>
    <section class="card">
      <h3>首次配置路径</h3>
      <ul class="clean-list">
        <li>先打开原生 Gateway，确认页面能正常进入。</li>
        <li>如果需要联网搜索、记忆检索或自定义模型，再回到这里保存配置。</li>
        <li>保存后重启插件，再回到 Gateway 做实际验证。</li>
      </ul>
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
        <button class="btn" type="button" onclick="ocOpenTerminalWindow('!openclaw doctor')">在终端验证</button>
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
        <button class="btn" type="button" onclick="ocOpenTerminalWindow('!openclaw memory status --deep')">在终端验证</button>
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
        <a class="btn secondary" href="https://docs.openclaw.ai/onboard" target="_blank" rel="noopener noreferrer">初始化文档</a>
        <button class="btn secondary" type="button" onclick="ocOpenTerminalWindow('!openclaw onboard')">打开初始化向导</button>
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
        <button class="btn" type="button" onclick="ocOpenTerminalWindow('!openclaw status --deep')">在终端查看当前模型</button>
        <span class="form-status" id="modelSaveStatus">保存后需重启插件</span>
      </div>
    </div>
  </section>

</div>"#,
        panel_config_path = panel_config_path,
        access = kv_row("访问模式", &config.access_mode),
        mode = kv_row("网关模式", &config.gateway_mode),
        https = kv_row("HTTPS 端口", &config.https_port),
        model = kv_row("当前对话模型", &config.current_model),
        access_label = kv_row("当前模式", access_mode_label(&config.access_mode)),
        access_help = kv_row("适用说明", access_mode_help(&config.access_mode)),
        gateway_help = kv_row("网关运行", gateway_mode_help(&config.gateway_mode)),
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
    )
}

fn commands_content_native(_config: &PageConfig) -> String {
    format!(
        r##"<div class="page-grid">
  <section class="card">
    <div class="card-head">
      <div>
        <div class="eyebrow">命令行</div>
        <h2>原生命令参考</h2>
        <p class="muted">这里保留官方命令参考，并提供一个最轻量的原生 TUI 终端入口。打开后会直接进入 <code>openclaw tui</code>，需要执行本机命令时请使用 <code>!命令</code>。</p>
      </div>
      <div class="header-actions">
        <a class="btn" href="https://docs.openclaw.ai/tui" target="_blank" rel="noopener noreferrer">TUI 文档</a>
        <a class="btn" href="https://docs.openclaw.ai/cli/index" target="_blank" rel="noopener noreferrer">CLI 文档</a>
        <button class="btn secondary" type="button" onclick="ocOpenTerminalWindow('')">OpenClaw CLI</button>
        <a class="btn primary" href="#" id="ocGatewayLinkCmd" target="_blank" rel="noopener noreferrer" onclick="return ocOpenGatewayLink(event, this)">打开网关</a>
        <a class="btn" href="./openclaw-ca.crt" target="_blank" rel="noopener noreferrer">下载 CA 证书</a>
      </div>
    </div>

    <div class="command-section">
      <div class="section-label">官方入口</div>
      <ul class="clean-list">
        <li><code>openclaw tui</code>：进入官方 TUI。</li>
        <li><code>openclaw onboard</code>：执行首次初始化向导。</li>
        <li><code>openclaw --version</code>：查看当前运行时版本。</li>
      </ul>
      <div class="action-row">
        <button class="btn primary" type="button" onclick="ocOpenTerminalWindow('')">打开 OpenClaw CLI</button>
        <button class="btn" type="button" onclick="ocOpenTerminalWindow('!openclaw onboard')">运行初始化向导</button>
      </div>
    </div>

    <div class="command-section">
      <div class="section-label">状态与健康</div>
      <ul class="clean-list">
        <li><code>openclaw health --json</code></li>
        <li><code>openclaw status --deep</code></li>
        <li><code>curl -fsS http://127.0.0.1:48099/healthz</code></li>
        <li><code>curl -fsS http://127.0.0.1:48099/readyz</code></li>
      </ul>
      <div class="action-row">
        <button class="btn" type="button" onclick="ocOpenTerminalWindow('!openclaw health --json')">在终端执行健康检查</button>
        <button class="btn" type="button" onclick="ocOpenTerminalWindow('!openclaw status --deep')">在终端查看运行状态</button>
      </div>
    </div>

    <div class="command-section">
      <div class="section-label">维护与诊断</div>
      <ul class="clean-list">
        <li><code>openclaw doctor</code></li>
        <li><code>openclaw doctor --fix</code></li>
        <li><code>openclaw security audit --deep</code></li>
        <li><code>openclaw memory status --deep</code></li>
      </ul>
      <div class="action-row">
        <button class="btn" type="button" onclick="ocOpenTerminalWindow('!openclaw doctor')">运行 doctor</button>
        <button class="btn" type="button" onclick="ocOpenTerminalWindow('!openclaw doctor --fix')">运行 doctor --fix</button>
      </div>
      <div class="mini-tip">终端默认直接进入原生 <code>openclaw tui</code>；这里的按钮会把命令按官方 <code>!命令</code> 方式送进去。</div>
    </div>

    <div class="command-section">
      <div class="section-label">日志命令</div>
      <ul class="clean-list">
        <li><code>openclaw logs --follow</code></li>
        <li><code>tail -f /tmp/openclaw/openclaw-$(date +%F).log</code></li>
        <li><code>ha addons logs openclaw_assistant_rust</code></li>
      </ul>
      <div class="action-row">
        <button class="btn" type="button" onclick="ocOpenTerminalWindow('!openclaw logs --follow')">在终端跟随日志</button>
      </div>
    </div>
  </section>
</div>"##,
    )
}

fn logs_content() -> String {
    format!(
        r##"<div class="page-grid">
  <section class="card">
    <div class="card-head">
      <div>
        <div class="eyebrow">日志</div>
        <h2>日志与诊断</h2>
        <p class="muted">这里保留最常用的日志和诊断命令。你可以直接打开上面的终端，它会进入原生 <code>openclaw tui</code>；也可以在 Home Assistant 的 Terminal &amp; SSH 或 SSH 会话里执行相同命令。</p>
      </div>
      <div class="header-actions">
        <button class="btn secondary" type="button" onclick="ocOpenTerminalWindow('!openclaw logs --follow')">打开日志终端</button>
      </div>
    </div>

    <ul class="clean-list">
      <li><code>openclaw logs --follow</code></li>
      <li><code>tail -f /tmp/openclaw/openclaw-$(date +%F).log</code></li>
      <li><code>openclaw doctor</code></li>
      <li><code>openclaw doctor --fix</code></li>
      <li><code>openclaw status --deep</code></li>
      <li><code>ha addons logs openclaw_assistant_rust</code></li>
    </ul>
    <div class="mini-tip">如果你要直接看日志或执行维护命令，可以打开上面的终端；如果你只需要原生控制台，请优先点击首页的“打开网关”。</div>
  </section>
</div>"##,
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
    let gatewayTokenValue = "";
    function appUrl(relativePath) {{ return new URL(relativePath, location.href).toString(); }}
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
    async function waitForGatewayReady(timeoutMs = 150000) {{
      const deadline = Date.now() + timeoutMs;
      let stable = 0;
      while (Date.now() < deadline) {{
        try {{
          const response = await fetch(appUrl('./readyz'), {{ credentials: 'same-origin', cache: 'no-cache' }});
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
      await waitForGatewayReady();
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
      if (typeof command === "string" && command.trim()) {{
        targetUrl.searchParams.set("command", command);
      }}
      window.open(targetUrl.toString(), "_blank", "noopener,noreferrer");
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
    syncGatewayLink();
    ocSyncProviderMeta("web");
    ocSyncProviderMeta("memory");
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
    render_shell(
        &config,
        NavPage::Home,
        "OpenClawHAOSAddon-Rust",
        "查看 OpenClaw 当前是否正常运行、各服务进程状态，以及系统资源占用情况。",
        &home_content(&config, &snapshot, health_ok),
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
        "命令参考",
        "在这里查看官方命令、健康检查和维护指令，然后到 Home Assistant 的 Terminal & SSH 或其它本机 shell 中执行。",
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

#[tokio::main]
async fn main() {
    let cache: Arc<RwLock<Option<CachedSnapshot>>> = Arc::new(RwLock::new(None));
    let cache_bg = cache.clone();
    tokio::spawn(async move {
        loop {
            let (snapshot, health_ok) = tokio::join!(
                collect_system_snapshot(),
                fetch_openclaw_health(),
            );
            *cache_bg.write().await = Some(CachedSnapshot { snapshot, health_ok });
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
    let app_state = AppState { cache };

    let app = Router::new()
        .route("/", get(index))
        .route("/config", get(config_page))
        .route("/commands", get(commands_page))
        .route("/logs", get(logs_page))
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
            web_provider: "firecrawl".to_string(),
            web_enabled: true,
            web_base_url: "https://api.firecrawl.dev".to_string(),
            web_model: "firecrawl-search-v1".to_string(),
            web_api_configured: true,
            memory_provider: "openai".to_string(),
            memory_enabled: true,
            memory_model: "text-embedding-3-large".to_string(),
            memory_fallback: "none".to_string(),
            memory_base_url: String::new(),
            memory_local_model_path: String::new(),
            memory_api_configured: true,
            current_model: "gpt-4o".to_string(),
            gateway_token: String::new(),
        }
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
        assert!(!html.contains("相关文件"));
        assert!(!html.contains("查看 openclaw.json"));
        assert!(!html.contains("查看 Add-on 配置文件"));
        assert!(!html.contains("查看 mcporter.json"));
        assert!(!html.contains("证书目录"));
        assert!(!html.contains("运行时目录"));
        assert!(!html.contains("备份目录"));
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
        assert!(html.contains("./readyz"));
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


