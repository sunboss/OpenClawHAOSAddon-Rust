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

fn runtime_pid() -> String {
    let gateway = pid_value("openclaw-gateway");
    if gateway != "-" {
        gateway
    } else {
        pid_value("openclaw-node")
    }
}

fn runtime_kind() -> &'static str {
    if pid_value("openclaw-gateway") != "-" {
        "gateway"
    } else if pid_value("openclaw-node") != "-" {
        "node"
    } else {
        "gateway"
    }
}

fn display_value(value: &str) -> &str {
    let value = value.trim();
    if value.is_empty() { "disabled" } else { value }
}

fn js_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
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
            return rest.split_whitespace().next()?.parse::<u64>().ok();
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
        format!("{days}d {hours}h {minutes}m")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

fn process_uptime(pid: &str) -> Option<String> {
    if pid == "-" || pid.trim().is_empty() {
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

fn collect_snapshot() -> SystemSnapshot {
    let cpu_load = read_first_line("/proc/loadavg")
        .and_then(|line| line.split_whitespace().next().map(|v| format!("{v} / 1m")))
        .unwrap_or_else(|| "Unavailable".to_string());

    let memory_used = match (
        parse_meminfo_kib("MemTotal:"),
        parse_meminfo_kib("MemAvailable:"),
    ) {
        (Some(total), Some(available)) if total > available => {
            let used = (total - available) * 1024;
            let total_bytes = total * 1024;
            format!(
                "{}/{}",
                format_bytes_gib(used),
                format_bytes_gib(total_bytes)
            )
        }
        _ => "Unavailable".to_string(),
    };

    let disk_used = Command::new("df")
        .args(["-h", "/config"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let line = stdout.lines().nth(1)?;
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 5 {
                Some(format!("{}/{} ({})", parts[2], parts[1], parts[4]))
            } else {
                None
            }
        })
        .unwrap_or_else(|| "Unavailable".to_string());

    let uptime = read_first_line("/proc/uptime")
        .and_then(|line| {
            line.split_whitespace()
                .next()
                .and_then(|value| value.parse::<f64>().ok())
        })
        .map(|value| format_duration(value as u64))
        .unwrap_or_else(|| "Unavailable".to_string());

    let openclaw_uptime =
        process_uptime(&runtime_pid()).unwrap_or_else(|| "Unavailable".to_string());

    SystemSnapshot {
        cpu_load,
        memory_used,
        disk_used,
        uptime,
        openclaw_uptime,
    }
}

fn nav_link(current: NavPage, page: NavPage, href: &str, label: &str) -> String {
    let class_name = if current == page {
        "nav-link active"
    } else {
        "nav-link"
    };
    format!(r#"<a class="{class_name}" href="{href}">{label}</a>"#)
}

fn kv_row(label: &str, value: &str) -> String {
    format!(
        r#"<div class="kv-row"><span>{}</span><strong>{}</strong></div>"#,
        escape_html(label),
        escape_html(value)
    )
}

fn button_row(entries: &[(&str, &str)]) -> String {
    entries
        .iter()
        .map(|(label, command)| {
            format!(
                r#"<button class="btn" type="button" data-command="{}" onclick="ocRunButton(this)">{}</button>"#,
                escape_html(command),
                escape_html(label)
            )
        })
        .collect::<Vec<_>>()
        .join("")
}

fn terminal_card(title: &str, subtitle: &str, button_label: &str) -> String {
    format!(
        r#"<section class="card">
  <div class="card-head">
    <div>
      <div class="eyebrow">Terminal</div>
      <h2>{}</h2>
      <p class="muted">{}</p>
    </div>
    <div class="actions">
      <button class="btn primary" type="button" onclick="ocLoadTerminal()">{}</button>
      <button class="btn" type="button" onclick="ocOpenTerminalWindow()">New window</button>
    </div>
  </div>
  <div class="terminal-shell">
    <div class="terminal-head">Embedded shell</div>
    <div id="terminalStage" class="terminal-stage">
      <div class="terminal-placeholder">
        <p>The terminal loads only when you need it.</p>
      </div>
    </div>
  </div>
</section>"#,
        escape_html(title),
        escape_html(subtitle),
        escape_html(button_label),
    )
}

fn home_content(config: &PageConfig) -> String {
    let snapshot = collect_snapshot();
    let runtime_pid = runtime_pid();
    let ingress_pid = pid_value("ingressd");
    let ui_pid = pid_value("haos-ui");
    let action_pid = pid_value("actiond");

    format!(
        r#"<div class="grid">
  <section class="card">
    <div class="card-head">
      <div>
        <div class="eyebrow">Overview</div>
        <h2>Runtime health</h2>
        <p class="muted">A quick view of service status, versions, and resource pressure.</p>
      </div>
      <div class="actions">
        <button class="btn primary" type="button" onclick="ocOpenGateway()">Open gateway</button>
        <a class="btn" href="./commands">Commands</a>
        <button class="btn" type="button" onclick="ocOpenTerminalWindow()">Terminal</button>
      </div>
    </div>
    <div class="grid two">
      <div class="panel">
        {}
        {}
        {}
        {}
        {}
      </div>
      <div class="panel">
        {}
        {}
        {}
        {}
        {}
        {}
      </div>
    </div>
  </section>
  <section class="card">
    <div class="card-head"><div><div class="eyebrow">Live panels</div><h2>Service summary</h2></div></div>
    <div class="grid two">
      <section id="healthPanel" class="panel" aria-live="polite"><p class="muted">Loading service summary…</p></section>
      <section id="diagPanel" class="panel" aria-live="polite"><p class="muted">Loading capability summary…</p></section>
    </div>
  </section>
</div>"#,
        kv_row("Access mode", &config.access_mode),
        kv_row("Gateway mode", &config.gateway_mode),
        kv_row("Add-on version", &config.addon_version),
        kv_row("OpenClaw version", &config.openclaw_version),
        kv_row("Runtime kind", runtime_kind()),
        kv_row("CPU load", &snapshot.cpu_load),
        kv_row("Memory usage", &snapshot.memory_used),
        kv_row("Disk usage", &snapshot.disk_used),
        kv_row("System uptime", &snapshot.uptime),
        kv_row("OpenClaw uptime", &snapshot.openclaw_uptime),
        kv_row(
            "Managed PIDs",
            &format!(
                "{}={} ingress={} ui={} action={}",
                runtime_kind(),
                runtime_pid,
                ingress_pid,
                ui_pid,
                action_pid
            ),
        ),
    )
}

fn config_content(config: &PageConfig) -> String {
    format!(
        r#"<div class="grid">
  <section class="card">
    <div class="card-head">
      <div>
        <div class="eyebrow">Config</div>
        <h2>Managed settings</h2>
        <p class="muted">These are the settings and directories the add-on manages directly.</p>
      </div>
      <div class="actions">
        <button class="btn" type="button" onclick="ocOpenGateway()">Open gateway</button>
        <button class="btn" type="button" onclick="ocOpenTerminalWindow()">Open terminal</button>
      </div>
    </div>
    <div class="grid two">
      <div class="panel">
        {}
        {}
        {}
        {}
      </div>
      <div class="panel">
        {}
        {}
        {}
        {}
        {}
      </div>
    </div>
  </section>
</div>"#,
        kv_row("Access mode", &config.access_mode),
        kv_row("Gateway mode", &config.gateway_mode),
        kv_row("HTTPS port", &config.https_port),
        kv_row("Gateway public URL", display_value(&config.gateway_url)),
        kv_row("OpenClaw config", "/config/.openclaw"),
        kv_row("mcporter home", "/config/.mcporter"),
        kv_row("Backup dir", "/share/openclaw-backup/latest"),
        kv_row("MCP", display_value(&config.mcp_status)),
        kv_row("Web search", display_value(&config.web_status)),
    )
}

fn commands_content() -> String {
    let setup = button_row(&[
        ("List devices", "openclaw devices list"),
        (
            "Approve latest pairing",
            "openclaw devices approve --latest",
        ),
        ("Run onboard", "openclaw onboard"),
        ("Doctor --fix", "openclaw doctor --fix"),
    ]);
    let diagnostics = button_row(&[
        ("Health JSON", "openclaw health --json"),
        ("Status --deep", "openclaw status --deep"),
        ("Follow logs", "openclaw logs --follow"),
        (
            "Gateway log file",
            "tail -f /tmp/openclaw/openclaw-$(date +%F).log",
        ),
        (
            "Restart managed runtime",
            "curl -fsS -X POST http://127.0.0.1:48100/action/restart",
        ),
        ("Latest npm version", "npm view openclaw version"),
    ]);
    let storage = button_row(&[
        (
            "Read gateway token",
            "jq -r '.gateway.auth.token' /config/.openclaw/openclaw.json",
        ),
        ("List MCP entries", "mcporter list"),
        ("Show MCP config", "cat /config/.mcporter/mcporter.json"),
        ("Show backup dir", "ls -la /share/openclaw-backup/latest"),
    ]);

    format!(
        r#"<div class="grid">
  <section class="card">
    <div class="card-head">
      <div>
        <div class="eyebrow">Commands</div>
        <h2>Command workspace</h2>
        <p class="muted">Buttons inject real CLI commands into the embedded shell.</p>
      </div>
      <div class="actions">
        <button class="btn primary" type="button" onclick="ocLoadTerminal()">Load terminal</button>
        <button class="btn" type="button" onclick="ocCloseTerminal()">Close terminal</button>
        <a class="btn" href="./openclaw-ca.crt" target="_blank" rel="noopener noreferrer">Download CA cert</a>
      </div>
    </div>
    <div class="eyebrow">Setup</div><div class="actions">{}</div>
    <div class="eyebrow">Diagnostics</div><div class="actions">{}</div>
    <div class="eyebrow">Storage and MCP</div><div class="actions">{}</div>
  </section>
  {}
</div>"#,
        setup,
        diagnostics,
        storage,
        terminal_card(
            "Embedded shell",
            "Use the shell for quick maintenance or to continue command output.",
            "Load terminal"
        ),
    )
}

fn logs_content() -> String {
    let logs = button_row(&[
        ("Follow logs", "openclaw logs --follow"),
        (
            "Gateway log file",
            "tail -f /tmp/openclaw/openclaw-$(date +%F).log",
        ),
        ("Doctor", "openclaw doctor"),
        ("Doctor --fix", "openclaw doctor --fix"),
        ("Status --deep", "openclaw status --deep"),
    ]);

    format!(
        r#"<div class="grid">
  <section class="card">
    <div class="card-head">
      <div>
        <div class="eyebrow">Logs</div>
        <h2>Logs and diagnosis</h2>
        <p class="muted">Keep long-running tails and repair output on a dedicated page.</p>
      </div>
      <div class="actions">
        <button class="btn" type="button" onclick="ocOpenTerminalWindow()">New terminal window</button>
      </div>
    </div>
    <div class="actions">{}</div>
  </section>
  <section class="card">
    <div class="grid two">
      <section id="healthPanel" class="panel" aria-live="polite"><p class="muted">Loading service summary…</p></section>
      <section id="diagPanel" class="panel" aria-live="polite"><p class="muted">Loading capability summary…</p></section>
    </div>
  </section>
  {}
</div>"#,
        logs,
        terminal_card(
            "Log terminal",
            "Send a log command above, then continue watching output here.",
            "Load log terminal"
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
    let nav_home = nav_link(page, NavPage::Home, "./", "Home");
    let nav_config = nav_link(page, NavPage::Config, "./config", "Config");
    let nav_commands = nav_link(page, NavPage::Commands, "./commands", "Commands");
    let nav_logs = nav_link(page, NavPage::Logs, "./logs", "Logs");
    let gateway_url = js_string(&config.gateway_url);

    Html(format!(
        r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>OpenClawHAOSAddon-Rust</title>
  <style>
    body {{ margin:0; font-family:"Segoe UI","Microsoft YaHei",sans-serif; color:#183250; background:linear-gradient(180deg,#edf3ff 0%,#f9fbff 100%); }}
    .wrap {{ max-width:1280px; margin:0 auto; padding:24px 18px 40px; }}
    .skip-link {{ position:absolute; left:16px; top:12px; transform:translateY(-160%); padding:10px 14px; border-radius:999px; background:#183250; color:#fff; text-decoration:none; font-weight:700; z-index:10; transition:transform .16s ease; }}
    .skip-link:focus-visible {{ transform:translateY(0); outline:2px solid #8fc8ff; outline-offset:3px; }}
    .nav {{ display:flex; gap:8px; flex-wrap:wrap; margin-bottom:18px; }}
    .nav-link {{ padding:10px 16px; border-radius:999px; background:#fff; border:1px solid #d7e4f4; color:#183250; text-decoration:none; font-weight:700; transition:background-color .16s ease, border-color .16s ease, color .16s ease, box-shadow .16s ease; }}
    .nav-link.active {{ background:#1f8ceb; border-color:#1f8ceb; color:#fff; }}
    .nav-link:hover {{ background:#f3f8ff; border-color:#b8cef0; }}
    .nav-link:focus-visible {{ outline:2px solid #1f8ceb; outline-offset:3px; box-shadow:0 0 0 4px rgba(31,140,235,.18); }}
    .hero, .card, .panel {{ border:1px solid #d7e4f4; border-radius:22px; background:rgba(255,255,255,.96); box-shadow:0 10px 28px rgba(23,52,86,.08); }}
    .hero {{ padding:24px; margin-bottom:18px; }}
    .hero h1 {{ margin:0 0 8px; font-size:34px; }}
    .muted {{ color:#667f99; line-height:1.7; }}
    .grid {{ display:grid; gap:18px; }}
    .grid.two {{ grid-template-columns:repeat(auto-fit,minmax(280px,1fr)); }}
    .card {{ padding:20px; }}
    .panel {{ padding:16px; }}
    .card-head {{ display:flex; gap:16px; justify-content:space-between; align-items:flex-start; margin-bottom:14px; flex-wrap:wrap; }}
    .eyebrow {{ font-size:12px; text-transform:uppercase; letter-spacing:.08em; color:#6c83a4; font-weight:800; margin-bottom:8px; }}
    .actions {{ display:flex; gap:10px; flex-wrap:wrap; }}
    .btn {{ min-height:42px; padding:10px 16px; border-radius:999px; border:1px solid #b8cef0; background:#eef5ff; color:#183250; font-weight:700; cursor:pointer; text-decoration:none; transition:background-color .16s ease, border-color .16s ease, color .16s ease, box-shadow .16s ease, transform .16s ease; }}
    .btn.primary {{ background:#1f8ceb; border-color:#1f8ceb; color:#fff; }}
    .btn:hover {{ background:#e3f0ff; border-color:#9ec0eb; transform:translateY(-1px); }}
    .btn.primary:hover {{ background:#157fd8; border-color:#157fd8; }}
    .btn:focus-visible {{ outline:2px solid #1f8ceb; outline-offset:3px; box-shadow:0 0 0 4px rgba(31,140,235,.18); }}
    .kv-row {{ display:flex; justify-content:space-between; gap:12px; padding:10px 0; border-bottom:1px solid #e7eef8; }}
    .kv-row:last-child {{ border-bottom:0; }}
    .terminal-shell {{ border-radius:18px; overflow:hidden; border:1px solid #273451; background:#10192f; }}
    .terminal-head {{ padding:12px 16px; color:#dbe7ff; border-bottom:1px solid #273451; font-weight:700; }}
    .terminal-stage {{ min-height:420px; }}
    .terminal-placeholder {{ min-height:420px; display:grid; place-items:center; color:#dbe7ff; }}
  </style>
</head>
<body>
  <a class="skip-link" href="#main-content">Skip to main content</a>
  <div class="wrap">
    <nav class="nav">{nav_home}{nav_config}{nav_commands}{nav_logs}</nav>
    <section class="hero">
      <div class="eyebrow">OpenClawHAOSAddon-Rust</div>
      <h1>{title}</h1>
      <p class="muted">{subtitle}</p>
      <p class="muted">Add-on version: {addon_version}</p>
    </section>
    <main id="main-content">
      {content}
    </main>
  </div>
  <script>
    const configuredGatewayUrl = {gateway_url};
    const httpsPort = {https_port};
    const terminalState = {{ loaded:false, loading:false, pendingCommand:null }};
    const initialTerminalStageHtml = document.getElementById("terminalStage") ? document.getElementById("terminalStage").innerHTML : "";
    let gatewayTokenValue = "";

    function appUrl(relativePath) {{ return new URL(relativePath, location.href).toString(); }}
    function nativeGatewayUrl() {{ return configuredGatewayUrl && configuredGatewayUrl.trim() !== "" ? configuredGatewayUrl : `https://${{location.hostname}}:${{httpsPort}}/`; }}
    function withTokenHash(url, token) {{ return !url || !token ? url : String(url).replace(/#.*$/, "") + "#token=" + encodeURIComponent(token); }}

    async function fetchGatewayToken() {{
      if (gatewayTokenValue) return gatewayTokenValue;
      const response = await fetch(appUrl("./token"), {{ credentials: "same-origin", cache: "no-cache" }});
      if (!response.ok) throw new Error(`token-${{response.status}}`);
      gatewayTokenValue = (await response.text()).trim();
      if (!gatewayTokenValue) throw new Error("empty-token");
      return gatewayTokenValue;
    }}

    async function loadPanel(url, targetId) {{
      const target = document.getElementById(targetId);
      if (!target) return;
      try {{
        const response = await fetch(url, {{ credentials: "same-origin" }});
        if (!response.ok) throw new Error(`HTTP ${{response.status}}`);
        target.innerHTML = await response.text();
      }} catch (error) {{
        target.innerHTML = `<p class="muted">Panel load failed: ${{error.message}}</p>`;
      }}
    }}

    function refreshPanels() {{
      if (document.visibilityState !== "visible") return;
      if (document.getElementById("healthPanel")) loadPanel(appUrl("./partials/health"), "healthPanel");
      if (document.getElementById("diagPanel")) loadPanel(appUrl("./partials/diag"), "diagPanel");
    }}

    function ensureTerminalLoaded() {{
      if (terminalState.loaded || terminalState.loading) return;
      const stage = document.getElementById("terminalStage");
      if (!stage) return;
      terminalState.loading = true;
      stage.innerHTML = `<iframe id="termFrame" src="${{appUrl("./terminal/")}}" title="terminal" style="width:100%;height:420px;border:0;background:#10192f"></iframe>`;
      const frame = document.getElementById("termFrame");
      frame.addEventListener("load", function () {{
        terminalState.loading = false;
        terminalState.loaded = true;
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

    window.ocOpenGateway = function () {{
      const targetUrl = nativeGatewayUrl();
      fetchGatewayToken().then((token) => window.open(withTokenHash(targetUrl, token), "_blank", "noopener,noreferrer")).catch(() => window.open(targetUrl, "_blank", "noopener,noreferrer"));
    }};
    window.ocOpenTerminalWindow = function () {{ window.open(appUrl("./terminal/"), "_blank", "noopener,noreferrer"); }};
    window.ocLoadTerminal = function () {{ ensureTerminalLoaded(); }};
    window.ocRunCommand = function (command) {{ injectTerminalCommand(command || ""); }};
    window.ocRunButton = function (button) {{ if (button) injectTerminalCommand(button.getAttribute("data-command") || ""); }};
    window.ocCloseTerminal = function () {{
      const stage = document.getElementById("terminalStage");
      if (!stage) return;
      stage.innerHTML = initialTerminalStageHtml;
      terminalState.loaded = false;
      terminalState.loading = false;
      terminalState.pendingCommand = null;
    }};

    document.addEventListener("visibilitychange", refreshPanels);
    window.setInterval(refreshPanels, 45000);
    window.setTimeout(refreshPanels, 120);
  </script>
</body>
</html>"##,
        title = escape_html(title),
        subtitle = escape_html(subtitle),
        addon_version = escape_html(&config.addon_version),
        nav_home = nav_home,
        nav_config = nav_config,
        nav_commands = nav_commands,
        nav_logs = nav_logs,
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
        "Overview",
        "Service health, versions, and resource pressure.",
        &home_content(config),
    )
}

async fn config_page(State(state): State<AppState>) -> impl IntoResponse {
    let config = &state.config;
    render_shell(
        config,
        NavPage::Config,
        "Configuration",
        "Settings and directories managed by the add-on.",
        &config_content(config),
    )
}

async fn commands_page(State(state): State<AppState>) -> impl IntoResponse {
    let config = &state.config;
    render_shell(
        config,
        NavPage::Commands,
        "Command workspace",
        "High-frequency maintenance commands with a nearby shell.",
        &commands_content(),
    )
}

async fn logs_page(State(state): State<AppState>) -> impl IntoResponse {
    let config = &state.config;
    render_shell(
        config,
        NavPage::Logs,
        "Logs and diagnosis",
        "A focused page for tails, doctor output, and troubleshooting.",
        &logs_content(),
    )
}

async fn health_partial(State(state): State<AppState>) -> impl IntoResponse {
    let config = &state.config;
    Html(format!(
        r#"<div class="eyebrow">Service summary</div><h2>Managed services</h2>{}{}{}{}"#,
        kv_row("Access mode", &config.access_mode),
        kv_row("Gateway mode", &config.gateway_mode),
        kv_row("OpenClaw version", &config.openclaw_version),
        kv_row("Runtime PID", &runtime_pid()),
    ))
}

async fn diag_partial(State(state): State<AppState>) -> impl IntoResponse {
    let config = &state.config;
    Html(format!(
        r#"<div class="eyebrow">Capability summary</div><h2>Startup-managed features</h2>{}{}{}{}"#,
        kv_row("MCP", display_value(&config.mcp_status)),
        kv_row("Web search", display_value(&config.web_status)),
        kv_row("Memory search", display_value(&config.memory_status)),
        kv_row("HTTPS port", &config.https_port),
    ))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("haos-ui: listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
