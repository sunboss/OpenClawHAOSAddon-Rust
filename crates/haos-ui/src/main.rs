use axum::{
    Router,
    extract::State,
    response::{Html, IntoResponse},
    routing::get,
};
use std::{env, fs, net::SocketAddr, process::Command, sync::Arc};

#[derive(Clone)]
struct AppState {
    config: Arc<PageConfig>,
}

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
        Self {
            addon_version: env_value("ADDON_VERSION", "unknown"),
            access_mode: env_value("ACCESS_MODE", "lan_https"),
            gateway_mode: env_value("GATEWAY_MODE", "local"),
            gateway_url: env_value("GW_PUBLIC_URL", ""),
            openclaw_version: env_value("OPENCLAW_VERSION", "unknown"),
            https_port: env_value("HTTPS_PORT", "18789"),
            mcp_status: env_value("MCP_STATUS", "disabled"),
            web_status: env_value("WEB_SEARCH_PROVIDER", "disabled"),
            memory_status: env_value("MEMORY_SEARCH_PROVIDER", "disabled"),
        }
    }
}

fn env_value(key: &str, fallback: &str) -> String {
    env::var(key).unwrap_or_else(|_| fallback.to_string())
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

fn html_attr_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('\'', "&#39;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn read_first_line(path: &str) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .and_then(|value| value.lines().next().map(|line| line.trim().to_string()))
}

fn parse_meminfo_kib(key: &str) -> Option<u64> {
    let contents = fs::read_to_string("/proc/meminfo").ok()?;
    for line in contents.lines() {
        if let Some(rest) = line.strip_prefix(key) {
            let value = rest.split_whitespace().next()?.parse::<u64>().ok()?;
            return Some(value);
        }
    }
    None
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

fn disk_snapshot() -> String {
    let output = Command::new("df").args(["-h", "/config"]).output();
    if let Ok(output) = output {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(line) = stdout.lines().nth(1) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 5 {
                    return format!("{}/{} ({})", parts[2], parts[1], parts[4]);
                }
            }
        }
    }
    "不可用".to_string()
}

fn disk_percent_snapshot() -> Option<u8> {
    let output = Command::new("df").args(["-B1", "/config"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.lines().nth(1)?;
    let parts: Vec<&str> = line.split_whitespace().collect();
    let total = parts.get(1)?.parse::<u64>().ok()?;
    let used = parts.get(2)?.parse::<u64>().ok()?;
    if total == 0 {
        return None;
    }
    Some(((used.saturating_mul(100)) / total).min(100) as u8)
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

fn collect_system_snapshot() -> SystemSnapshot {
    let cpu_load = read_first_line("/proc/loadavg")
        .and_then(|line| line.split_whitespace().next().map(|v| format!("{v} / 1m")))
        .unwrap_or_else(|| "不可用".to_string());
    let cpu_percent = load_percent_snapshot().unwrap_or(0);

    let memory_used = match (parse_meminfo_kib("MemTotal:"), parse_meminfo_kib("MemAvailable:")) {
        (Some(total), Some(available)) if total > available => {
            let used = (total - available) * 1024;
            let total_bytes = total * 1024;
            format!("{}/{}", format_bytes_gib(used), format_bytes_gib(total_bytes))
        }
        _ => "不可用".to_string(),
    };

    let memory_percent = match (parse_meminfo_kib("MemTotal:"), parse_meminfo_kib("MemAvailable:")) {
        (Some(total), Some(available)) if total > available && total > 0 => {
            (((total - available) * 100) / total).min(100) as u8
        }
        _ => 0,
    };

    let uptime = read_first_line("/proc/uptime")
        .and_then(|line| {
            line.split_whitespace()
                .next()
                .and_then(|value| value.parse::<f64>().ok())
                .map(|value| format_duration(value as u64))
        })
        .unwrap_or_else(|| "不可用".to_string());

    let openclaw_uptime = process_uptime(&pid_value("openclaw-gateway"))
        .or_else(|| process_uptime(&pid_value("openclaw-node")))
        .unwrap_or_else(|| "不可用".to_string());

    SystemSnapshot {
        cpu_load,
        memory_used,
        disk_used: disk_snapshot(),
        uptime,
        openclaw_uptime,
        cpu_percent,
        memory_percent,
        disk_percent: disk_percent_snapshot().unwrap_or(0),
    }
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
    format!(
        r#"<button class="btn" type="button" data-command="{}" onclick="ocRunButton(this)">{label}</button>"#,
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

fn pid_row(gateway_pid: &str, ingress_pid: &str, ui_pid: &str, action_pid: &str) -> String {
    format!(
        r#"<div class="pid-grid pid-grid-row">
  <span class="pid-chip">Gateway {gateway_pid}</span>
  <span class="pid-chip">Ingress {ingress_pid}</span>
  <span class="pid-chip">UI {ui_pid}</span>
  <span class="pid-chip">Action {action_pid}</span>
</div>"#
    )
}

fn home_content(config: &PageConfig) -> String {
    let gateway_pid = pid_value("openclaw-gateway");
    let ingress_pid = pid_value("ingressd");
    let ui_pid = pid_value("haos-ui");
    let action_pid = pid_value("actiond");
    let snapshot = collect_system_snapshot();
    let online_count = [
        gateway_pid.as_str(),
        ingress_pid.as_str(),
        ui_pid.as_str(),
        action_pid.as_str(),
    ]
    .into_iter()
    .filter(|value| *value != "-")
    .count();

    let (health_text, health_sub, health_tone) = if online_count >= 4 {
        ("运行正常", "关键进程全部在线", "tone-good")
    } else if online_count >= 2 {
        ("部分在线", "建议查看命令行和日志页", "tone-warn")
    } else {
        ("待检查", "关键进程数量不足", "tone-danger")
    };

    format!(
        r#"<div class="page-grid">
  <section class="summary-strip">
    {health}
    {runtime}
    {https}
    {openclaw_runtime}
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

  <section class="card hero-card">
    <div class="card-head">
      <div>
        <div class="eyebrow">首页</div>
        <h2>运行状态总览</h2>
        <p class="muted">首页只保留运行状态、版本信息、资源监控和高频入口。命令和日志拆出去后，首屏更轻，也更适合长期维护。</p>
      </div>
      <div class="header-actions">
        {open_gateway}
        {goto_commands}
      </div>
    </div>

    <div class="stats-grid">
      {stat_access}
      {stat_mode}
      {stat_addon}
      {stat_openclaw}
    </div>

    <div class="note-box">如果你需要处理设备授权，请进入命令行页执行授权命令。常用命令是 <code>openclaw devices list</code> 和 <code>openclaw devices approve --latest</code>。</div>

    {pid_row}
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
        goto_commands = hero_action_link("进入命令行", "./commands"),
        stat_access = stat_tile("访问模式", &config.access_mode, "当前插件的访问入口模式"),
        stat_mode = stat_tile("网关模式", &config.gateway_mode, "当前 OpenClaw 网关运行模式"),
        stat_addon = stat_tile("Add-on 版本", &config.addon_version, "插件发布版本"),
        stat_openclaw = stat_tile("OpenClaw 版本", &config.openclaw_version, "上游运行时版本"),
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
        resource_uptime = resource_card(
            "系统运行时长",
            &snapshot.uptime,
            100,
            "tone-violet",
            "适合判断是否刚重启、是否发生过异常恢复。"
        ),
        resource_openclaw_uptime = resource_card(
            "OpenClaw 运行时长",
            &snapshot.openclaw_uptime,
            100,
            "tone-blue",
            "适合判断 Gateway 是否刚重启，或是否发生过短暂掉线。"
        ),
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
        <p class="muted">如果你想确认当前访问方式、数据保存位置、备份目录和能力开关，就看这一页。这里更适合核对状态和理解配置含义，不需要在命令行里来回查找。</p>
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
          <li>Brave Search、Gemini Memory 这类便捷开箱能力的首次初始化。</li>
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
        mcp = kv_row("MCP", display_value(&config.mcp_status)),
        web = kv_row("Web Search", display_value(&config.web_status)),
        memory = kv_row("Memory Search", display_value(&config.memory_status)),
        version = kv_row("Add-on 版本", &config.addon_version),
    )
}

fn commands_content() -> String {
    let setup_actions = [
        ("设备列表", "openclaw devices list"),
        ("批准最新配对", "openclaw devices approve --latest"),
        ("初始化向导", "openclaw onboard"),
        ("检查并修复", "openclaw doctor --fix"),
    ]
    .iter()
    .map(|(label, cmd)| action_button(label, cmd))
    .collect::<Vec<_>>()
    .join("");

    let diagnostic_actions = [
        ("健康检查", "openclaw health --json"),
        ("运行状态", "openclaw status --deep"),
        ("跟随日志", "openclaw logs --follow"),
        ("网关日志", "tail -f /tmp/openclaw/openclaw-$(date +%F).log"),
        ("安全审计", "openclaw security audit --deep"),
        ("记忆状态", "openclaw memory status --deep"),
        ("重启网关", "curl -fsS -X POST http://127.0.0.1:48100/action/restart"),
        (
            "检查 npm 版本",
            "npm view openclaw version",
        ),
    ]
    .iter()
    .map(|(label, cmd)| action_button(label, cmd))
    .collect::<Vec<_>>()
    .join("");

    let storage_actions = [
        ("读取令牌", "jq -r '.gateway.auth.token' /config/.openclaw/openclaw.json"),
        ("MCP 列表", "mcporter list"),
        ("MCP 配置", "cat /config/.mcporter/mcporter.json"),
        ("备份目录", "ls -la /share/openclaw-backup/latest"),
        (
            "立即备份",
            "mkdir -p /share/openclaw-backup/latest/.openclaw /share/openclaw-backup/latest/.mcporter && rsync -a --delete /config/.openclaw/ /share/openclaw-backup/latest/.openclaw/ && rsync -a --delete /config/.mcporter/ /share/openclaw-backup/latest/.mcporter/",
        ),
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
      <div class="section-label">MCP 与备份</div>
      <div class="action-row">{storage_actions}</div>
    </div>
  </section>
  {terminal}
</div>"#,
        load_terminal = primary_button("打开终端", "ocLoadTerminal()"),
        close_terminal = ghost_button("关闭终端", "ocCloseTerminal()"),
        open_window = secondary_button("新窗口打开终端", "ocOpenTerminalWindow()"),
        setup_actions = setup_actions,
        diagnostic_actions = diagnostic_actions,
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
            "先点击上面的日志按钮，再在这里继续观察输出。适合长时间盯日志、复制报错和回看修复后的变化。",
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
  <title>OpenClawHAOSAddon-Rust</title>
  <style>
    :root {{
      --panel:#ffffff; --line:#d8e2ee; --text:#1f2f42; --muted:#5f748d; --blue:#1f8ceb;
      --blue-2:#5c7cff; --teal:#14a3c7; --shadow:0 3px 14px rgba(31,47,66,.08); --shell:#10192f;
      --shell-line:#273451;
    }}
    * {{ box-sizing:border-box; }}
    html {{ scroll-behavior:smooth; }}
    body {{
      margin:0; color:var(--text); font-family:"Segoe UI","PingFang SC","Microsoft YaHei",sans-serif;
      background:radial-gradient(circle at top right, rgba(55,104,242,.15), transparent 24%), linear-gradient(180deg,#edf3ff 0%,#f9fbff 100%);
    }}
    .page-accent {{ height:40px; background:linear-gradient(90deg,#6072e4 0%,#6879ea 100%); }}
    .wrap {{ max-width:1460px; margin:0 auto; padding:0 24px 40px; }}
    .masthead {{ margin-top:-18px; border:1px solid rgba(221,231,246,.95); border-radius:0 0 28px 28px; background:linear-gradient(180deg,rgba(255,255,255,.96) 0%,rgba(244,248,255,.92) 100%); box-shadow:var(--shadow); overflow:hidden; backdrop-filter:blur(8px); }}
    .top-shell {{ background:rgba(255,255,255,.92); border-bottom:1px solid rgba(216,226,238,.95); overflow:hidden; }}
    .nav-tabs {{ display:flex; gap:2px; overflow-x:auto; }}
    .nav-link {{ min-width:112px; min-height:58px; display:inline-flex; align-items:center; justify-content:center; padding:0 22px; text-decoration:none; color:#29415f; font-size:15px; font-weight:800; border-bottom:3px solid transparent; background:#fff; }}
    .nav-link:hover {{ color:var(--blue); background:#f7fbff; }}
    .nav-link.active {{ color:var(--blue); background:#f7fbff; border-bottom-color:var(--blue); }}
    .hero {{ padding:30px 30px 24px; display:flex; justify-content:space-between; gap:20px; align-items:flex-start; }}
    .hero h1 {{ margin:0 0 12px; font-size:44px; line-height:1.06; letter-spacing:-.03em; font-weight:900; }}
    .hero p {{ margin:0; max-width:920px; color:var(--muted); font-size:16px; line-height:1.78; }}
    .hero-chip {{ flex:0 0 auto; min-height:52px; display:inline-flex; align-items:center; justify-content:center; padding:0 22px; border-radius:999px; border:1px solid #c8d8f1; background:linear-gradient(180deg,#fff 0%,#edf4ff 100%); color:#213f63; font-size:15px; font-weight:900; white-space:nowrap; box-shadow:0 8px 22px rgba(55,104,242,.10); }}
    .page-grid {{ display:grid; gap:20px; }}
    .summary-strip {{ display:grid; grid-template-columns:repeat(auto-fit,minmax(220px,1fr)); gap:14px; }}
    .two-col {{ display:grid; grid-template-columns:minmax(0,1.18fr) minmax(360px,.92fr); gap:20px; align-items:start; }}
    .three-up {{ display:grid; grid-template-columns:repeat(3,minmax(0,1fr)); gap:18px; }}
    .split-grid {{ display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); gap:18px; }}
    .feature-grid {{ display:grid; grid-template-columns:repeat(auto-fit,minmax(220px,1fr)); gap:14px; }}
    .resource-grid {{ display:grid; grid-template-columns:repeat(auto-fit,minmax(220px,1fr)); gap:14px; }}
    .card {{ border:1px solid var(--line); border-radius:24px; background:var(--panel); box-shadow:var(--shadow); padding:26px 28px; }}
    .card h2 {{ margin:0 0 10px; font-size:30px; line-height:1.14; letter-spacing:-.02em; }}
    .card h3 {{ margin:0 0 10px; font-size:20px; line-height:1.2; }}
    .summary-strip-card {{ position:relative; overflow:hidden; border:1px solid var(--line); border-radius:20px; background:#fff; box-shadow:var(--shadow); padding:20px 22px; }}
    .summary-strip-card::before {{ content:""; position:absolute; left:0; top:0; bottom:0; width:4px; background:#c9d8ee; }}
    .summary-strip-card::after {{ content:""; position:absolute; inset:0; background:linear-gradient(180deg, rgba(255,255,255,.1) 0%, rgba(244,248,255,.55) 100%); pointer-events:none; }}
    .summary-strip-card.tone-good::before {{ background:#32b87a; }}
    .summary-strip-card.tone-warn::before {{ background:#e2ad2b; }}
    .summary-strip-card.tone-danger::before {{ background:#e16060; }}
    .summary-strip-card.tone-blue::before {{ background:#1f8ceb; }}
    .summary-strip-card.tone-teal::before {{ background:#14a3c7; }}
    .summary-strip-card.tone-violet::before {{ background:#7357ff; }}
    .summary-strip-title {{ color:#6e84a3; font-size:12px; font-weight:900; letter-spacing:.08em; text-transform:uppercase; margin-bottom:10px; }}
    .summary-strip-value {{ font-size:26px; line-height:1.06; font-weight:900; letter-spacing:-.03em; margin-bottom:8px; }}
    .summary-strip-sub {{ color:#66809f; font-size:13px; line-height:1.65; }}
    .card-head {{ display:flex; justify-content:space-between; align-items:flex-start; gap:18px; margin-bottom:14px; }}
    .card-head.compact {{ margin-bottom:18px; }}
    .eyebrow {{ color:#6c83a4; font-size:12px; font-weight:900; letter-spacing:.08em; text-transform:uppercase; margin-bottom:8px; }}
    .muted {{ color:var(--muted); line-height:1.78; margin:0; }}
    .header-actions,.action-row,.pill-row,.toolbar-grid {{ display:flex; gap:12px; flex-wrap:wrap; }}
    .btn {{ min-height:44px; display:inline-flex; align-items:center; justify-content:center; padding:10px 18px; border-radius:999px; border:1px solid #cfdcec; background:#fff; color:var(--text); text-decoration:none; font-weight:800; cursor:pointer; transition:transform .16s ease, box-shadow .16s ease, border-color .16s ease, background-color .16s ease; }}
    .btn:hover {{ transform:translateY(-1px); box-shadow:0 8px 18px rgba(31,140,235,.10); border-color:#a8c5e4; background:#f9fbff; }}
    .btn.primary {{ color:#fff; border-color:transparent; background:linear-gradient(135deg,#1893f8,#0f76e8); }}
    .btn.secondary {{ color:#fff; border-color:transparent; background:linear-gradient(135deg,#7357ff,#8a63ff); }}
    .stats-grid {{ display:grid; grid-template-columns:repeat(4,minmax(0,1fr)); gap:14px; margin:20px 0; }}
    .stat-card, .info-tile, .soft-card, .resource-card {{ border:1px solid var(--line); border-radius:20px; background:#fff; padding:18px; }}
    .stat-card {{ min-height:146px; display:flex; flex-direction:column; justify-content:space-between; }}
    .stat-label,.resource-label {{ color:var(--muted); font-size:13px; font-weight:800; }}
    .stat-value, .resource-value {{ font-size:24px; font-weight:900; line-height:1.1; letter-spacing:-.03em; word-break:break-word; }}
    .stat-sub, .resource-sub, .info-tile p, .soft-card p {{ color:#607a99; font-size:13px; line-height:1.7; margin:0; }}
    .status-panel {{ display:grid; grid-template-columns:minmax(0,1fr) 360px; gap:18px; align-items:start; }}
    .live-row {{ display:flex; align-items:center; gap:12px; font-size:18px; margin-bottom:14px; }}
    .live-dot {{ width:10px; height:10px; border-radius:999px; background:#22b572; box-shadow:0 0 0 6px rgba(34,181,114,.12); }}
    .pill-inline,.pid-chip,.pill {{ display:inline-flex; align-items:center; gap:8px; border-radius:999px; padding:8px 14px; background:#eef3ff; color:#2a54d8; font-weight:800; }}
    .pill {{ justify-content:space-between; min-width:150px; }}
    .panel-left,.panel-right {{ border:1px solid var(--line); border-radius:20px; background:#fff; padding:20px; }}
    .pid-grid {{ display:flex; flex-wrap:wrap; gap:10px; margin-top:14px; }}
    .pid-grid-row {{ border:1px dashed #d5e2f2; border-radius:18px; padding:14px 16px; background:#fbfdff; }}
    .note-box {{ padding:14px 16px; border-radius:16px; background:#f6f9fc; color:#4d6784; line-height:1.75; }}
    .resource-card {{ display:flex; flex-direction:column; gap:12px; }}
    .resource-top {{ display:flex; justify-content:space-between; gap:12px; align-items:center; }}
    .resource-percent {{ color:#27466a; font-size:13px; font-weight:900; }}
    .progress-track {{ width:100%; height:10px; border-radius:999px; overflow:hidden; background:#eaf0fb; border:1px solid #d9e4f7; }}
    .progress-fill {{ display:block; height:100%; border-radius:999px; background:linear-gradient(90deg,#3868f2,#6b7df0); }}
    .progress-fill.tone-blue {{ background:linear-gradient(90deg,#3868f2,#5f82ff); }}
    .progress-fill.tone-teal {{ background:linear-gradient(90deg,#16a5a4,#48c7c3); }}
    .progress-fill.tone-gold {{ background:linear-gradient(90deg,#d59f19,#f2c35a); }}
    .progress-fill.tone-violet {{ background:linear-gradient(90deg,#6a4cf4,#9e59f7); }}
    .clean-list {{ margin:0; padding-left:18px; color:var(--muted); line-height:1.8; }}
    .kv-list {{ display:grid; gap:12px; }}
    .kv-row {{ display:flex; justify-content:space-between; gap:18px; padding-bottom:12px; border-bottom:1px solid var(--line); }}
    .kv-row:last-child {{ border-bottom:0; padding-bottom:0; }}
    .kv-label {{ color:var(--muted); white-space:nowrap; }}
    .kv-value {{ font-weight:800; text-align:right; word-break:break-word; }}
    .command-section {{ margin-top:18px; }}
    .section-label {{ color:#7087a8; font-size:12px; font-weight:900; letter-spacing:.08em; text-transform:uppercase; margin-bottom:10px; }}
    .ghost-field {{ min-height:48px; display:inline-flex; align-items:center; padding:0 16px; border-radius:16px; border:1px solid var(--line); background:#fbfdff; color:#67809e; white-space:nowrap; }}
    .ghost-field.wide {{ min-width:240px; }}
    .terminal-card {{ padding-bottom:22px; }}
    .terminal-shell {{ border-radius:24px; border:1px solid #24375c; background:var(--shell); overflow:hidden; }}
    .terminal-head {{ display:flex; justify-content:space-between; gap:12px; padding:14px 18px; border-bottom:1px solid var(--shell-line); color:#dbe7ff; font-weight:800; }}
    .terminal-head span {{ color:#b9cae7; font-weight:600; }}
    .terminal-stage,.terminal-placeholder,iframe {{ min-height:560px; }}
    .terminal-placeholder {{ display:grid; place-items:center; text-align:center; padding:28px; color:#d5dff3; }}
    .terminal-placeholder-inner {{ max-width:420px; }}
    .terminal-placeholder h3 {{ margin:0 0 12px; color:#eff5ff; font-size:24px; }}
    .terminal-placeholder p {{ margin:0 0 18px; line-height:1.8; }}
    iframe {{ display:block; width:100%; border:0; background:var(--shell); }}
    code {{ padding:2px 6px; border-radius:8px; background:#eef4ff; color:#27466a; font-family:Consolas,"SFMono-Regular",monospace; }}
    @media (max-width:1180px) {{
      .summary-strip,.two-col,.three-up,.split-grid,.stats-grid,.feature-grid,.status-panel,.resource-grid {{ grid-template-columns:1fr; }}
    }}
    @media (max-width:760px) {{
      .wrap {{ padding:0 14px 30px; }} .hero {{ flex-direction:column; padding:24px 20px 20px; }} .hero h1 {{ font-size:34px; }}
      .nav-link {{ min-width:92px; padding:0 16px; }} .card {{ padding:20px; }}
      .terminal-stage,.terminal-placeholder,iframe {{ min-height:440px; }}
    }}
  </style>
</head>
<body>
  <div class="page-accent"></div>
  <div class="wrap">
    <section class="masthead">
      <section class="top-shell"><nav class="nav-tabs">{nav_home}{nav_config}{nav_commands}{nav_logs}</nav></section>
      <section class="hero">
        <div><h1>{title}</h1><p>{subtitle}</p></div>
        <div class="hero-chip">插件 {addon_version}</div>
      </section>
    </section>
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
    window.ocOpenTerminalWindow = function () {{ window.open(appUrl("./terminal/"), "_blank", "noopener,noreferrer"); }};
    window.ocLoadTerminal = function () {{ ensureTerminalLoaded(); window.setTimeout(focusTerminal, 120); }};
    window.ocRunCommand = function (command) {{ injectTerminalCommand(command || ""); }};
    window.ocRunButton = function (button) {{
      if (!button) return;
      const command = button.getAttribute("data-command") || "";
      injectTerminalCommand(command);
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
        title = title,
        subtitle = subtitle,
        addon_version = config.addon_version,
        content = content,
        gateway_url = gateway_url,
        https_port = config.https_port,
    ))
}

async fn index(State(state): State<AppState>) -> impl IntoResponse {
    let config = &state.config;
    render_shell(
        config,
        NavPage::Home,
        "OpenClawHAOSAddon-Rust",
        "首页专门负责运行状态、资源监控和高频入口。把配置、命令和日志拆出去后，整体更轻，也更适合长期维护。",
        &home_content(config),
    )
}

async fn config_page(State(state): State<AppState>) -> impl IntoResponse {
    let config = &state.config;
    render_shell(
        config,
        NavPage::Config,
        "基础配置",
        "这一页用来查看插件当前怎么运行、数据保存在什么位置，以及哪些能力已经启用。需要核对配置时先看这里，会比直接翻日志和命令更直观。",
        &config_content(config),
    )
}

async fn commands_page(State(state): State<AppState>) -> impl IntoResponse {
    let config = &state.config;
    render_shell(
        config,
        NavPage::Commands,
        "命令行工作区",
        "这一页保留高频控制按钮和终端。按钮显示中文，实际执行仍然是英文 OpenClaw 命令。",
        &commands_content(),
    )
}

async fn logs_page(State(state): State<AppState>) -> impl IntoResponse {
    let config = &state.config;
    render_shell(
        config,
        NavPage::Logs,
        "日志与诊断",
        "日志和诊断独立成页后，首页更轻，命令页也不会再被长输出挤满，排查问题时路径更清楚。",
        &logs_content(),
    )
}

async fn health_partial(State(state): State<AppState>) -> impl IntoResponse {
    let config = &state.config;
    let gateway_pid = pid_value("openclaw-gateway");
    let node_pid = pid_value("openclaw-node");
    let display_gateway_pid = if gateway_pid != "-" { gateway_pid } else { node_pid };

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
    let config = &state.config;
    Html(format!(
        r#"<div class="eyebrow">能力摘要</div>
<h2>快速诊断</h2>
<div class="kv-list">
  {mcp}
  {web}
  {memory}
  {https}
</div>"#,
        mcp = kv_row("MCP", display_value(&config.mcp_status)),
        web = kv_row("Web Search", display_value(&config.web_status)),
        memory = kv_row("Memory Search", display_value(&config.memory_status)),
        https = kv_row("HTTPS 端口", &config.https_port),
    ))
}

#[tokio::main]
async fn main() {
    let app_state = AppState {
        config: Arc::new(PageConfig::from_env()),
    };

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
