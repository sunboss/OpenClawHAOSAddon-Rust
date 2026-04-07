use axum::{
    Router,
    extract::State,
    response::{Html, IntoResponse},
    routing::get,
};
use std::{env, fs, net::SocketAddr, path::PathBuf, process::Command};

#[derive(Clone)]
struct AppState;

#[derive(Clone, Debug)]
struct PageConfig {
    addon_version: String,
    access_mode: String,
    gateway_mode: String,
    gateway_url: String,
    openclaw_version: String,
    https_port: String,
    mcp_status: String,
    web_status: String,
    memory_status: String,
    current_model: String,
    mcp_endpoint_count: usize,
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
            .and_then(|v| first_string_path(v, &["agents.defaults.model"]))
            .unwrap_or_else(|| "未配置".to_string());
        let mcp_endpoint_count = count_mcp_endpoints();
        Self {
            addon_version: env_value("ADDON_VERSION", "unknown"),
            access_mode: env_value("ACCESS_MODE", "lan_https"),
            gateway_mode: env_value("GATEWAY_MODE", "local"),
            gateway_url: env_value("GW_PUBLIC_URL", ""),
            openclaw_version: env_value("OPENCLAW_VERSION", "unknown"),
            https_port: env_value("HTTPS_PORT", "18789"),
            mcp_status: env_value("MCP_STATUS", "disabled"),
            web_status,
            memory_status,
            current_model,
            mcp_endpoint_count,
        }
    }
}

fn load_runtime_config() -> Option<serde_json::Value> {
    fs::read_to_string(runtime_config_path())
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
}

fn runtime_config_path() -> PathBuf {
    env::var("OPENCLAW_CONFIG_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/config/.openclaw/openclaw.json"))
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

fn env_value(key: &str, fallback: &str) -> String {
    env::var(key).unwrap_or_else(|_| fallback.to_string())
}

fn load_mcporter_config() -> Option<serde_json::Value> {
    fs::read_to_string("/config/.mcporter/mcporter.json")
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

fn count_pending_devices() -> usize {
    let Ok(output) = Command::new("openclaw").args(["devices", "list"]).output() else {
        return 0;
    };
    if !output.status.success() {
        return 0;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .filter(|line| line.to_lowercase().contains("pending"))
        .count()
}

async fn fetch_openclaw_health() -> Option<bool> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .ok()?;
    let resp = client
        .get("http://127.0.0.1:48100/health")
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return Some(false);
    }
    let json: serde_json::Value = resp.json().await.ok()?;
    json.get("ok").and_then(|v| v.as_bool())
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
      <span>左侧按钮发送的命令会直接注入到这里执行。</span>
    </div>
    <div class="terminal-stage" id="terminalStage">
      <div class="terminal-placeholder">
        <div class="terminal-placeholder-inner">
          <h3>终端按需加载</h3>
          <p>默认不抢占首屏资源。点击上方按钮或任意命令按钮后，会自动连接终端并继续执行。</p>
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

fn pid_row(gateway_pid: &str, ingress_pid: &str, ui_pid: &str, action_pid: &str) -> String {
    format!(
        r#"<div class="service-grid">
  {gateway}
  {ingress}
  {ui}
  {action}
</div>"#,
        gateway = service_badge("Gateway", gateway_pid),
        ingress = service_badge("Ingress", ingress_pid),
        ui = service_badge("UI", ui_pid),
        action = service_badge("Action", action_pid),
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
    let action_pid = pid_value("actiond");
    let online_count = [
        gateway_pid.as_str(),
        ingress_pid.as_str(),
        ui_pid.as_str(),
        action_pid.as_str(),
    ]
    .into_iter()
    .filter(|value| *value != "-")
    .count();

    // Health check result takes priority over PID count when available
    let (health_text, health_sub, health_tone) = match health_ok {
        Some(true) => ("运行正常", "服务健康检查通过", "tone-good"),
        Some(false) => ("响应异常", "健康检查未通过，请查看日志", "tone-danger"),
        None => {
            if online_count >= 4 {
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

        {device_notice}
        <div class="note-box">如果你需要处理设备授权，请进入命令行页执行授权命令。常用命令是 <code>openclaw devices list</code> 和 <code>openclaw devices approve --latest</code>。</div>
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
        open_gateway = primary_button("打开网关", "ocOpenGateway()"),
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
        device_notice = if pending_devices > 0 {
            format!(
                r#"<div class="notice-badge warn">有 {pending_devices} 个设备等待授权配对，请前往命令行页执行 <code>openclaw devices approve --latest</code>。</div>"#
            )
        } else {
            String::new()
        },
        pid_row = pid_row(&gateway_pid, &ingress_pid, &ui_pid, &action_pid),
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
            "curl -fsS -X POST http://127.0.0.1:48100/action/restart",
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
               placeholder="输入任意命令，回车或点击运行…"
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
        load_terminal = primary_button("打开终端", "ocLoadTerminal()"),
        close_terminal = ghost_button("关闭终端", "ocCloseTerminal()"),
        open_window = secondary_button("新窗口打开终端", "ocOpenTerminalWindow()"),
        setup_actions = setup_actions,
        diagnostic_actions = diagnostic_actions,
        log_stream_actions = log_stream_actions,
        token_action = token_action,
        storage_actions = storage_actions,
        terminal = terminal_card(
            "嵌入式终端",
            "左侧所有按钮都会把命令直接送到这里执行。需要长时间操作时，建议切到新窗口终端。",
            "加载终端",
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

    Html(format!(
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
    .custom-cmd-row{{ display:flex; gap:10px; align-items:center; }}
    .cmd-input{{
      flex:1; height:38px; padding:0 14px; border-radius:10px;
      border:1px solid #d1ddef; background:#fff;
      font:500 13px/1 "Segoe UI","PingFang SC","Microsoft YaHei",sans-serif;
      color:#1a2b42; outline:none; transition:border-color .15s,box-shadow .15s;
    }}
    .cmd-input:focus{{ border-color:#60a5fa; box-shadow:0 0 0 3px rgba(96,165,250,.18); }}
    .cmd-input::placeholder{{ color:#94a3b8; }}
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
      try {{
        const response = await fetch(url, {{ credentials: "same-origin" }});
        if (!response.ok) throw new Error(`HTTP ${{response.status}}`);
        target.innerHTML = await response.text();
      }} catch (error) {{
        target.innerHTML = `<p class="muted">面板加载失败：${{error.message}}</p>`;
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
    async function fetchGatewayToken() {{
      if (gatewayTokenValue) return gatewayTokenValue;
      const response = await fetch(appUrl('./token'), {{ credentials: 'same-origin', cache: 'no-cache' }});
      if (!response.ok) throw new Error(`token-${{response.status}}`);
      const text = (await response.text()).trim();
      if (!text) throw new Error('empty-token');
      gatewayTokenValue = text;
      return gatewayTokenValue;
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
    window.ocOpenGateway = function () {{
      const targetUrl = nativeGatewayUrl();
      fetchGatewayToken()
        .then(function (token) {{
          window.open(withTokenHash(targetUrl, token), "_blank", "noopener,noreferrer");
        }})
        .catch(function () {{
          window.open(targetUrl, "_blank", "noopener,noreferrer");
        }});
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
    document.addEventListener("visibilitychange", refreshPanels);
    window.setInterval(refreshPanels, 45000);
    window.setTimeout(refreshPanels, 120);
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
    ))
}

async fn index(State(state): State<AppState>) -> impl IntoResponse {
    let _ = state;
    let config = PageConfig::from_env();
    let (snapshot, health_ok, pending_devices) = tokio::join!(
        collect_system_snapshot(),
        fetch_openclaw_health(),
        async {
            tokio::time::timeout(
                std::time::Duration::from_secs(3),
                tokio::task::spawn_blocking(count_pending_devices),
            )
            .await
            .ok()
            .and_then(|r| r.ok())
            .unwrap_or(0)
        },
    );
    let pending_devices = pending_devices;
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
        &config_content(&config),
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
        &commands_content(),
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
    let gateway_pid = pid_value("openclaw-gateway");
    let node_pid = pid_value("openclaw-node");
    let display_gateway_pid = if gateway_pid != "-" {
        gateway_pid
    } else {
        node_pid
    };

    Html(format!(
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
    ))
}

async fn diag_partial(State(state): State<AppState>) -> impl IntoResponse {
    let _ = state;
    let config = PageConfig::from_env();
    Html(format!(
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
    ))
}

#[tokio::main]
async fn main() {
    let app_state = AppState;

    let app = Router::new()
        .route("/", get(index))
        .route("/config", get(config_page))
        .route("/commands", get(commands_page))
        .route("/logs", get(logs_page))
        .route("/partials/health", get(health_partial))
        .route("/partials/diag", get(diag_partial))
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

    #[test]
    fn commands_page_uses_supervisor_restart_endpoint() {
        let html = commands_content();

        assert!(html.contains("curl -fsS -X POST http://127.0.0.1:48100/action/restart"));
        assert!(!html.contains("openclaw gateway restart"));
    }

    #[test]
    fn commands_page_uses_real_npm_and_pairing_commands() {
        let html = commands_content();

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
    fn render_shell_includes_fixed_aspect_brand_logo() {
        let config = PageConfig {
            addon_version: "2026.04.03.8".to_string(),
            access_mode: "lan_https".to_string(),
            gateway_mode: "local".to_string(),
            gateway_url: String::new(),
            openclaw_version: "2026.4.2".to_string(),
            https_port: "18789".to_string(),
            mcp_status: "enabled".to_string(),
            web_status: "firecrawl".to_string(),
            memory_status: "x_search".to_string(),
            current_model: "gpt-4o".to_string(),
            mcp_endpoint_count: 0,
        };

        let Html(html) = render_shell(&config, NavPage::Home, "title", "subtitle", "<div></div>");

        assert!(html.contains("class=\"brand-mark\""));
        assert!(html.contains("preserveAspectRatio=\"xMidYMid meet\""));
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
