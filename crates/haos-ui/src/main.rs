use axum::{
    Router,
    extract::State,
    response::{Html, IntoResponse},
    routing::get,
};
use std::{env, fs, net::SocketAddr, sync::Arc};

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
    if value.trim().is_empty() {
        "disabled"
    } else {
        value
    }
}

fn js_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

#[tokio::main]
async fn main() {
    let app_state = AppState {
        config: Arc::new(PageConfig::from_env()),
    };

    let app = Router::new()
        .route("/", get(index))
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

async fn index(State(state): State<AppState>) -> impl IntoResponse {
    let config = &state.config;
    let gateway_url = js_string(&config.gateway_url);

    Html(format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>OpenClawHAOSAddon-Rust</title>
  <style>
    :root {{
      --bg: #eef4ff;
      --bg2: #f8fbff;
      --card: rgba(255, 255, 255, 0.96);
      --line: #d7e4f4;
      --text: #18304d;
      --muted: #667f99;
      --blue: #2563eb;
      --teal: #0f9f96;
      --soft: #edf5ff;
      --shell: #0f172a;
      --shellLine: #223252;
    }}
    * {{ box-sizing: border-box; }}
    body {{
      margin: 0;
      color: var(--text);
      font-family: "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif;
      background:
        radial-gradient(circle at top right, rgba(37,99,235,.10), transparent 24%),
        linear-gradient(180deg, var(--bg) 0%, var(--bg2) 100%);
    }}
    .wrap {{
      max-width: 1380px;
      margin: 0 auto;
      padding: 24px 18px 40px;
    }}
    .hero {{
      display: flex;
      justify-content: space-between;
      gap: 18px;
      align-items: flex-start;
      margin-bottom: 18px;
    }}
    .title {{
      margin: 0 0 8px;
      font-size: 30px;
      line-height: 1.08;
      font-weight: 800;
    }}
    .sub {{
      margin: 0;
      color: var(--muted);
      line-height: 1.7;
      max-width: 920px;
    }}
    .chip {{
      display: inline-flex;
      align-items: center;
      gap: 8px;
      padding: 10px 14px;
      border-radius: 999px;
      border: 1px solid #bdd2f0;
      background: var(--soft);
      color: #23425f;
      font-weight: 700;
      white-space: nowrap;
    }}
    .layout {{
      display: grid;
      grid-template-columns: minmax(0, 1.2fr) minmax(360px, .9fr);
      gap: 18px;
      align-items: start;
    }}
    .stack > .card + .card {{ margin-top: 18px; }}
    .card {{
      border: 1px solid var(--line);
      border-radius: 22px;
      background: var(--card);
      padding: 20px;
      box-shadow: 0 10px 28px rgba(23, 52, 86, 0.08);
      backdrop-filter: blur(12px);
    }}
    .card h2 {{
      margin: 0 0 8px;
      font-size: 21px;
    }}
    .hint {{
      margin: 0;
      color: var(--muted);
      line-height: 1.7;
    }}
    .note {{
      margin-top: 14px;
      padding: 12px 14px;
      border-radius: 16px;
      background: #f3f8ff;
      color: #45617b;
      line-height: 1.6;
    }}
    .actions {{
      display: flex;
      flex-wrap: wrap;
      gap: 12px;
      margin-top: 16px;
    }}
    .btn {{
      display: inline-flex;
      align-items: center;
      justify-content: center;
      min-height: 44px;
      padding: 10px 16px;
      border-radius: 999px;
      border: 1px solid #b8cef0;
      background: var(--soft);
      color: var(--text);
      text-decoration: none;
      font-weight: 700;
      cursor: pointer;
      transition: transform 120ms ease, box-shadow 120ms ease, border-color 120ms ease;
    }}
    .btn:hover {{
      transform: translateY(-1px);
      box-shadow: 0 6px 18px rgba(37,99,235,.12);
      border-color: #8fb2e4;
    }}
    .btn.primary {{
      color: #fff;
      border-color: transparent;
      background: linear-gradient(135deg, var(--blue), var(--teal));
    }}
    .btn.ghost {{ background: #fff; }}
    .section-label {{
      margin-top: 16px;
      color: var(--muted);
      font-size: 12px;
      font-weight: 800;
      letter-spacing: .08em;
      text-transform: uppercase;
    }}
    .kvs {{
      display: grid;
      gap: 12px;
      margin-top: 14px;
    }}
    .kv {{
      display: flex;
      justify-content: space-between;
      align-items: flex-start;
      gap: 14px;
      padding-bottom: 12px;
      border-bottom: 1px solid var(--line);
    }}
    .kv:last-child {{
      border-bottom: 0;
      padding-bottom: 0;
    }}
    .key {{
      color: var(--muted);
      white-space: nowrap;
    }}
    .value {{
      text-align: right;
      font-weight: 700;
      word-break: break-word;
    }}
    .badges {{
      display: flex;
      flex-wrap: wrap;
      gap: 8px;
      justify-content: flex-end;
    }}
    .badge {{
      display: inline-flex;
      align-items: center;
      padding: 8px 12px;
      border-radius: 999px;
      border: 1px solid #b8cef0;
      background: #f4f8ff;
      color: #2e4a67;
      font-weight: 700;
    }}
    details {{
      border: 1px solid var(--line);
      border-radius: 18px;
      background: rgba(255,255,255,.96);
      box-shadow: 0 10px 28px rgba(23, 52, 86, 0.08);
      overflow: hidden;
    }}
    details > summary {{
      list-style: none;
      cursor: pointer;
      padding: 14px 18px;
      font-weight: 800;
      color: var(--text);
      background: #fbfdff;
      display: flex;
      justify-content: space-between;
      align-items: center;
    }}
    details > summary::-webkit-details-marker {{ display: none; }}
    details > summary::after {{
      content: "+";
      color: var(--muted);
      font-size: 18px;
    }}
    details[open] > summary::after {{
      content: "-";
    }}
    .details-body {{
      padding: 16px 18px 18px;
      border-top: 1px solid var(--line);
      color: var(--muted);
      line-height: 1.8;
      background: #f9fbff;
    }}
    .details-body p {{
      margin: 0 0 10px;
    }}
    .details-body code {{
      padding: 2px 6px;
      border-radius: 8px;
      background: #eef4ff;
      color: #294565;
      font-family: Consolas, "SFMono-Regular", monospace;
    }}
    .details-body ul {{
      margin: 0;
      padding-left: 18px;
    }}
    .details-body li + li {{
      margin-top: 6px;
    }}
    .terminal-shell {{
      margin-top: 14px;
      border-radius: 20px;
      border: 1px solid #243a5f;
      background: var(--shell);
      overflow: hidden;
      min-height: 520px;
    }}
    .terminal-head {{
      display: flex;
      justify-content: space-between;
      gap: 12px;
      padding: 12px 16px;
      border-bottom: 1px solid var(--shellLine);
      color: #dbe8ff;
    }}
    .terminal-stage {{ min-height: 468px; }}
    .terminal-placeholder {{
      min-height: 468px;
      padding: 28px;
      display: grid;
      place-items: center;
      text-align: center;
      color: #c7d8f1;
    }}
    iframe {{
      display: block;
      width: 100%;
      min-height: 520px;
      border: 0;
      background: var(--shell);
    }}
    code {{
      padding: 2px 6px;
      border-radius: 8px;
      background: #eef4ff;
      font-family: Consolas, "SFMono-Regular", monospace;
      color: #294565;
    }}
    @media (max-width: 1120px) {{
      .layout {{ grid-template-columns: 1fr; }}
    }}
    @media (max-width: 720px) {{
      .wrap {{ padding: 18px 14px 28px; }}
      .hero {{ flex-direction: column; }}
      .title {{ font-size: 26px; }}
      .terminal-shell {{ min-height: 360px; }}
      .terminal-placeholder {{ min-height: 320px; }}
      iframe {{ min-height: 380px; }}
    }}
  </style>
</head>
<body>
  <div class="wrap">
    <section class="hero">
      <div>
        <h1 class="title">OpenClawHAOSAddon-Rust</h1>
        <p class="sub">
          Rust rewrite of the local HAOS add-on layer. Upstream <code>openclaw</code> and
          <code>mcporter</code> stay unchanged. This page is intentionally self-contained so it can
          still open even when external JS CDNs are unavailable.
        </p>
      </div>
      <div class="chip">Add-on {addon_version}</div>
    </section>

    <div class="layout">
      <div class="stack">
        <section class="card">
          <h2>Main Actions</h2>
          <p class="hint">
            These buttons target the most common control paths. Terminal loading is deferred until
            you actually need it.
          </p>
          <div class="actions">
            <button class="btn primary" type="button" onclick="ocOpenGateway()">Open Native Gateway</button>
            <button class="btn" type="button" onclick="ocLoadTerminal()">Open Terminal</button>
            <a class="btn" href="./openclaw-ca.crt" target="_blank" rel="noopener noreferrer">Download CA Cert</a>
          </div>
          <div class="section-label">Diagnostics</div>
          <div class="actions">
            <button class="btn ghost" type="button" onclick="ocRunCommand('openclaw status --deep')">Gateway Status</button>
            <button class="btn ghost" type="button" onclick="ocRunCommand('openclaw gateway restart')">Restart Gateway</button>
            <button class="btn ghost" type="button" onclick="ocRunCommand(`curl -fsSL https://registry.npmjs.org/openclaw/latest | jq -r '\"npm latest: \" + .version'`)">Check npm Version</button>
            <button class="btn ghost" type="button" onclick="ocRunCommand('openclaw devices list')">Devices List</button>
            <button class="btn ghost" type="button" onclick="ocRunCommand('openclaw doctor --fix')">Doctor Fix</button>
            <button class="btn ghost" type="button" onclick="ocRunCommand('openclaw logs --follow')">Follow Logs</button>
          </div>
          <div class="section-label">Setup And Recovery</div>
          <div class="actions">
            <button class="btn ghost" type="button" onclick="ocRunCommand('openclaw onboard')">Onboard</button>
            <button class="btn ghost" type="button" onclick="ocRunCommand('cat /etc/nginx/html/gateway.token')">Read Token</button>
            <button class="btn ghost" type="button" onclick="ocRunCommand('mcporter list HA')">MCP List</button>
            <button class="btn ghost" type="button" onclick="ocRunCommand('cat /config/.mcporter/mcporter.json')">MCP Config</button>
            <button class="btn ghost" type="button" onclick="ocRunCommand('ls -la /share/openclaw-backup/latest')">Backup Dir</button>
            <button class="btn ghost" type="button" onclick="ocRunCommand('rsync -a --delete /config/.openclaw/ /share/openclaw-backup/latest/.openclaw/ && rsync -a --delete /config/.mcporter/ /share/openclaw-backup/latest/.mcporter/')">Backup State</button>
          </div>
          <div class="note">
            If the native Gateway page reports a certificate error, trust the downloaded CA cert
            first and then reopen the native HTTPS page.
          </div>
        </section>

        <section class="card" id="healthPanel">
          <p class="hint">Loading service status...</p>
        </section>
      </div>

      <div class="stack">
        <section class="card" id="diagPanel">
          <p class="hint">Loading quick diagnostics...</p>
        </section>

        <section class="card">
          <h2>Embedded Terminal</h2>
          <div class="terminal-shell">
            <div class="terminal-head">
              <strong>Workspace Terminal</strong>
              <span class="hint" style="color:#aac0df;margin:0;">Commands from the left panel are injected here.</span>
            </div>
            <div class="terminal-stage" id="terminalStage">
              <div class="terminal-placeholder" id="terminalPlaceholder">
                <div>
                  <p>
                    The terminal is lazy-loaded to keep first paint fast. You can open it now or
                    just click any command button and it will auto-load.
                  </p>
                  <button class="btn primary" type="button" onclick="ocLoadTerminal()">Load Terminal</button>
                </div>
              </div>
            </div>
          </div>
        </section>
      </div>
    </div>

    <details>
      <summary>Token And Connection Help</summary>
      <div class="details-body">
        <p>
          The native Gateway page uses the local HTTPS endpoint on port <code>{https_port}</code>.
          If your browser warns about the certificate, install the downloaded CA cert first and reopen
          the page.
        </p>
        <ul>
          <li><code>Read Token</code> prints the current gateway token in the embedded terminal.</li>
          <li><code>Open Native Gateway</code> opens the HTTPS Gateway UI in a new tab.</li>
          <li><code>Open Terminal</code> scrolls to the embedded terminal and focuses the command input.</li>
        </ul>
      </div>
    </details>

    <details>
      <summary>MCP Settings</summary>
      <div class="details-body">
        <p>
          This add-on keeps Home Assistant MCP registration under <code>/config/.mcporter</code>.
          Use the buttons above to inspect the live registration state.
        </p>
        <ul>
          <li><code>MCP List</code> runs <code>mcporter list HA</code>.</li>
          <li><code>MCP Config</code> prints <code>/config/.mcporter/mcporter.json</code>.</li>
          <li>The diagnostics panel only shows current runtime state to keep the page fast.</li>
        </ul>
      </div>
    </details>

    <details>
      <summary>Backup And Restore</summary>
      <div class="details-body">
        <p>
          Runtime state is stored under <code>/config/.openclaw</code> and <code>/config/.mcporter</code>.
          The backup copy lives under <code>/share/openclaw-backup/latest</code>.
        </p>
        <ul>
          <li><code>Backup Dir</code> shows the backup target directory.</li>
          <li><code>Backup State</code> syncs the current runtime state into the backup directory.</li>
          <li>For reinstall recovery, copy the backup back into <code>/config</code> before restarting the add-on.</li>
        </ul>
      </div>
    </details>
  </div>

  <script>
    const configuredGatewayUrl = {gateway_url};
    const httpsPort = {https_port};
    const terminalState = {{
      loaded: false,
      loading: false,
      pendingCommand: null,
    }};

    function appBaseHref() {{
      const path = location.pathname.endsWith("/") ? location.pathname : location.pathname + "/";
      return new URL(path, location.origin);
    }}

    function appUrl(relativePath) {{
      return new URL(relativePath, appBaseHref()).toString();
    }}

    async function loadPanel(url, targetId) {{
      const target = document.getElementById(targetId);
      if (!target) return;
      try {{
        const response = await fetch(url, {{ credentials: "same-origin" }});
        if (!response.ok) throw new Error(`HTTP ${{response.status}}`);
        target.innerHTML = await response.text();
      }} catch (error) {{
        target.innerHTML = `<p class="hint">Failed to load panel: ${{error.message}}</p>`;
      }}
    }}

    function refreshPanels() {{
      if (document.visibilityState !== "visible") return;
      loadPanel(appUrl("partials/health"), "healthPanel");
      loadPanel(appUrl("partials/diag"), "diagPanel");
    }}

    function nativeGatewayUrl() {{
      if (configuredGatewayUrl && configuredGatewayUrl.trim() !== "") {{
        return configuredGatewayUrl;
      }}
      return `https://${{location.hostname}}:${{httpsPort}}/`;
    }}

    function focusTerminal() {{
      const shell = document.querySelector(".terminal-shell");
      if (shell) {{
        shell.scrollIntoView({{ behavior: "smooth", block: "start" }});
      }}
      const frame = document.getElementById("termFrame");
      if (frame && frame.contentWindow) {{
        frame.contentWindow.postMessage({{ type: "openclaw-focus-terminal" }}, "*");
      }}
    }}

    function ensureTerminalLoaded() {{
      if (terminalState.loaded || terminalState.loading) return;
      const stage = document.getElementById("terminalStage");
      if (!stage) return;

      terminalState.loading = true;
      stage.innerHTML = `<iframe id="termFrame" src="${{appUrl('terminal/')}}" title="terminal"></iframe>`;

      const frame = document.getElementById("termFrame");
      const finish = function () {{
        terminalState.loading = false;
        terminalState.loaded = true;
        focusTerminal();
        if (terminalState.pendingCommand) {{
          const next = terminalState.pendingCommand;
          terminalState.pendingCommand = null;
          window.setTimeout(() => injectTerminalCommand(next), 180);
        }}
      }};

      frame.addEventListener("load", finish, {{ once: true }});
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
      frame.contentWindow.postMessage({{
        type: "openclaw-run-command",
        command
      }}, "*");
      if (typeof frame.contentWindow.injectCommand === "function") {{
        frame.contentWindow.injectCommand(command);
        return;
      }}

      const doc = frame.contentWindow.document;
      const input = doc.querySelector(".xterm-helper-textarea, textarea.xterm-helper-textarea, textarea");
      if (!input) {{
        terminalState.pendingCommand = command;
        window.setTimeout(() => injectTerminalCommand(command), 180);
        return;
      }}

      frame.contentWindow.focus();
      input.focus();
      input.value = command + "\n";
      input.dispatchEvent(new InputEvent("input", {{
        bubbles: true,
        cancelable: true,
        data: command + "\n",
        inputType: "insertText"
      }}));
    }}

    window.ocOpenGateway = function () {{
      window.open(nativeGatewayUrl(), "_blank", "noopener,noreferrer");
    }};

    window.ocLoadTerminal = function () {{
      ensureTerminalLoaded();
      window.setTimeout(focusTerminal, 120);
    }};

    window.ocRunCommand = function (command) {{
      injectTerminalCommand(command || "");
    }};

    document.addEventListener("visibilitychange", refreshPanels);
    window.setInterval(refreshPanels, 45000);
    refreshPanels();
  </script>
</body>
</html>"#,
        addon_version = config.addon_version,
        gateway_url = gateway_url,
        https_port = config.https_port,
    ))
}

async fn health_partial(State(state): State<AppState>) -> impl IntoResponse {
    let config = &state.config;
    let gateway_pid = pid_value("openclaw-gateway");
    let node_pid = pid_value("openclaw-node");
    let display_gateway_pid = if gateway_pid != "-" {
        gateway_pid
    } else {
        node_pid
    };

    Html(format!(
        r#"<h2>Service Status</h2>
<div class="kvs">
  <div class="kv"><span class="key">Access Mode</span><span class="value">{access}</span></div>
  <div class="kv"><span class="key">Gateway Mode</span><span class="value">{gateway_mode}</span></div>
  <div class="kv"><span class="key">Add-on Version</span><span class="value">{addon_version}</span></div>
  <div class="kv"><span class="key">OpenClaw Version</span><span class="value">{openclaw_version}</span></div>
  <div class="kv">
    <span class="key">PID</span>
    <span class="badges">
      <span class="badge">Gateway {gw}</span>
      <span class="badge">Ingress {ingress}</span>
      <span class="badge">UI {ui}</span>
      <span class="badge">Action {action}</span>
    </span>
  </div>
</div>"#,
        access = config.access_mode,
        gateway_mode = config.gateway_mode,
        addon_version = config.addon_version,
        openclaw_version = config.openclaw_version,
        gw = display_gateway_pid,
        ingress = pid_value("ingressd"),
        ui = pid_value("haos-ui"),
        action = pid_value("actiond"),
    ))
}

async fn diag_partial(State(state): State<AppState>) -> impl IntoResponse {
    let config = &state.config;
    Html(format!(
        r#"<h2>Quick Diagnostics</h2>
<div class="kvs">
  <div class="kv"><span class="key">MCP</span><span class="value">{mcp}</span></div>
  <div class="kv"><span class="key">Web Search</span><span class="value">{web}</span></div>
  <div class="kv"><span class="key">Memory Search</span><span class="value">{memory}</span></div>
</div>
<p class="hint" style="margin-top:14px;">
  This panel only shows current runtime state so the page stays fast and predictable.
</p>"#,
        mcp = display_value(&config.mcp_status),
        web = display_value(&config.web_status),
        memory = display_value(&config.memory_status),
    ))
}
