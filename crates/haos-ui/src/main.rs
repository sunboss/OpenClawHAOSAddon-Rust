use axum::{
    Router,
    extract::State,
    response::{Html, IntoResponse},
    routing::get,
};
use std::{env, fs, net::SocketAddr, path::Path, sync::Arc};

#[derive(Clone)]
struct AppState {
    config: Arc<PageConfig>,
}

#[derive(Clone, Debug)]
struct PageConfig {
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

fn terminal_available() -> bool {
    Path::new("/run/openclaw-rs/ttyd.pid").exists()
}

fn display_value(value: &str) -> &str {
    if value.trim().is_empty() {
        "disabled"
    } else {
        value
    }
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
    let gateway_url = if config.gateway_url.trim().is_empty() {
        "#".to_string()
    } else {
        config.gateway_url.clone()
    };

    Html(format!(
        r##"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>OpenClawHAOSAddon-Rust</title>
  <script src="https://unpkg.com/htmx.org@2.0.6"></script>
  <style>
    :root {{
      --bg: #eef4ff;
      --bg-2: #f7fbff;
      --card: rgba(255, 255, 255, 0.96);
      --line: #d8e4f4;
      --text: #17314d;
      --muted: #6a829a;
      --accent: #2563eb;
      --accent-2: #0f9f96;
      --accent-soft: #edf5ff;
      --ok: #0a8f63;
      --warn: #c56d11;
      --shell: #0f172a;
      --shell-line: #223252;
    }}
    * {{ box-sizing: border-box; }}
    html {{ scroll-behavior: smooth; }}
    body {{
      margin: 0;
      color: var(--text);
      font-family: "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif;
      background:
        radial-gradient(circle at top right, rgba(37, 99, 235, 0.10), transparent 26%),
        linear-gradient(180deg, var(--bg) 0%, var(--bg-2) 100%);
    }}
    .wrap {{
      max-width: 1380px;
      margin: 0 auto;
      padding: 24px 18px 40px;
    }}
    .hero {{
      display: flex;
      justify-content: space-between;
      align-items: flex-start;
      gap: 18px;
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
      max-width: 900px;
      color: var(--muted);
      line-height: 1.75;
    }}
    .chip {{
      display: inline-flex;
      align-items: center;
      gap: 8px;
      padding: 10px 14px;
      border-radius: 999px;
      border: 1px solid #bdd2f0;
      background: var(--accent-soft);
      color: #23425f;
      font-weight: 700;
      white-space: nowrap;
    }}
    .layout {{
      display: grid;
      grid-template-columns: minmax(0, 1.25fr) minmax(360px, 0.92fr);
      gap: 18px;
      align-items: start;
    }}
    .stack > .card + .card {{
      margin-top: 18px;
    }}
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
      line-height: 1.65;
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
      background: var(--accent-soft);
      color: var(--text);
      text-decoration: none;
      font-weight: 700;
      cursor: pointer;
      transition: transform 120ms ease, box-shadow 120ms ease, border-color 120ms ease;
    }}
    .btn:hover {{
      transform: translateY(-1px);
      box-shadow: 0 6px 18px rgba(37, 99, 235, 0.12);
      border-color: #8fb2e4;
    }}
    .btn.primary {{
      color: #fff;
      border-color: transparent;
      background: linear-gradient(135deg, var(--accent), var(--accent-2));
    }}
    .btn.ghost {{
      background: #fff;
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
      justify-content: flex-end;
      gap: 8px;
    }}
    .badge {{
      display: inline-flex;
      align-items: center;
      padding: 7px 12px;
      border-radius: 999px;
      background: #eef4ff;
      color: #2a486c;
      font-weight: 700;
    }}
    .terminal-shell {{
      border-radius: 18px;
      overflow: hidden;
      border: 1px solid var(--shell-line);
      background: var(--shell);
      color: #dce9ff;
      min-height: 480px;
    }}
    .terminal-head {{
      display: flex;
      justify-content: space-between;
      align-items: center;
      gap: 10px;
      padding: 12px 14px;
      background: rgba(255, 255, 255, 0.03);
      border-bottom: 1px solid rgba(255, 255, 255, 0.08);
    }}
    .terminal-stage {{
      min-height: 420px;
    }}
    .terminal-placeholder {{
      display: flex;
      min-height: 420px;
      padding: 28px;
      align-items: center;
      justify-content: center;
      flex-direction: column;
      text-align: center;
      gap: 16px;
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
      .layout {{
        grid-template-columns: 1fr;
      }}
    }}
    @media (max-width: 720px) {{
      .wrap {{
        padding: 18px 14px 28px;
      }}
      .hero {{
        flex-direction: column;
      }}
      .title {{
        font-size: 26px;
      }}
      .terminal-shell {{
        min-height: 360px;
      }}
      .terminal-placeholder {{
        min-height: 320px;
      }}
      iframe {{
        min-height: 380px;
      }}
    }}
  </style>
</head>
<body>
  <div class="wrap">
    <section class="hero">
      <div>
        <h1 class="title">OpenClawHAOSAddon-Rust</h1>
        <p class="sub">
          这是我们这层的 Rust 重写版控制页。上游 <code>openclaw</code> 和 <code>mcporter</code>
          保持不动，页面、动作服务、配置助手和本地监管器逐步改成 Rust。
          这一版重点收掉旧模板里的乱码、重复脚本和首屏阻塞加载。
        </p>
      </div>
      <div class="chip">版本 {version}</div>
    </section>

    <div class="layout">
      <div class="stack">
        <section class="card">
          <h2>主操作</h2>
          <p class="hint">
            这里保留最常用入口。所有快捷命令都会优先写入右侧终端，
            不再额外弹中间提示层。
          </p>
          <div class="actions">
            <a class="btn primary" href="{gateway_url}" target="_blank" rel="noopener noreferrer">打开原生 Gateway</a>
            <button class="btn" type="button" data-load-terminal="true">打开终端</button>
            <a class="btn" href="https://127.0.0.1:{https_port}/openclaw-ca.crt" target="_blank" rel="noopener noreferrer">下载 CA 证书</a>
            <button class="btn ghost" type="button" data-cmd="openclaw gateway status --deep">查看网关状态</button>
            <button class="btn ghost" type="button" data-cmd="openclaw devices list">devices list</button>
            <button class="btn ghost" type="button" data-cmd="openclaw doctor --fix">doctor --fix</button>
            <button class="btn ghost" type="button" data-cmd="openclaw logs --follow">logs --follow</button>
          </div>
          <div class="note">
            终端改成按需加载，页面首屏不会立刻创建 iframe。健康状态和诊断面板只会在页面可见时刷新，
            避免旧版那种后台标签页持续轮询导致的卡顿。
          </div>
        </section>

        <section
          class="card"
          id="healthPanel"
          hx-get="/partials/health"
          hx-trigger="load, refresh-health from:body"
          hx-swap="innerHTML"
        >
          <p class="hint">正在加载服务状态…</p>
        </section>
      </div>

      <div class="stack">
        <section
          class="card"
          id="diagPanel"
          hx-get="/partials/diag"
          hx-trigger="load, refresh-diag from:body"
          hx-swap="innerHTML"
        >
          <p class="hint">正在加载快速诊断…</p>
        </section>

        <section class="card">
          <h2>内嵌终端</h2>
          <div class="terminal-shell">
            <div class="terminal-head">
              <strong>工作区终端</strong>
              <span class="hint" style="color:#aac0df;margin:0;">点击左侧命令会自动拉起终端并执行</span>
            </div>
            <div class="terminal-stage" id="terminalStage">
              <div class="terminal-placeholder" id="terminalPlaceholder">
                <p>
                  终端默认延迟加载，避免首屏卡顿。你可以手动打开，
                  或者直接点击左侧快捷命令让页面自动载入终端。
                </p>
                <button class="btn primary" type="button" data-load-terminal="true">立即加载终端</button>
              </div>
            </div>
          </div>
        </section>
      </div>
    </div>
  </div>

  <script>
    const terminalState = {{
      loaded: false,
      loading: false,
      pendingCommand: null,
    }};

    function ensureTerminalLoaded() {{
      if (terminalState.loaded || terminalState.loading) return;
      const stage = document.getElementById("terminalStage");
      if (!stage) return;

      terminalState.loading = true;
      stage.innerHTML = '<iframe id="termFrame" src="./terminal/" title="终端"></iframe>';

      const frame = document.getElementById("termFrame");
      const finish = function () {{
        terminalState.loading = false;
        terminalState.loaded = true;
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

    function refreshPanels() {{
      if (document.visibilityState !== "visible" || !window.htmx) return;
      htmx.trigger("#healthPanel", "refresh-health");
      htmx.trigger("#diagPanel", "refresh-diag");
    }}

    document.addEventListener("click", function (event) {{
      const trigger = event.target.closest("[data-cmd], [data-load-terminal]");
      if (!trigger) return;

      if (trigger.hasAttribute("data-load-terminal")) {{
        ensureTerminalLoaded();
        return;
      }}

      injectTerminalCommand(trigger.getAttribute("data-cmd") || "");
    }});

    document.addEventListener("visibilitychange", refreshPanels);
    window.setInterval(refreshPanels, 45000);
  </script>
</body>
</html>"##,
        version = config.openclaw_version,
        gateway_url = gateway_url,
        https_port = config.https_port
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
    let nginx_pid = pid_value("nginx");
    let ttyd_pid = if terminal_available() {
        pid_value("ttyd")
    } else {
        "-".to_string()
    };
    let action_pid = pid_value("actiond");

    Html(format!(
        r##"<h2>服务状态</h2>
<div class="kvs">
  <div class="kv"><span class="key">访问模式</span><span class="value">{access}</span></div>
  <div class="kv"><span class="key">网关模式</span><span class="value">{gateway_mode}</span></div>
  <div class="kv"><span class="key">版本</span><span class="value">{version}</span></div>
  <div class="kv"><span class="key">PID</span><span class="badges"><span class="badge">Gateway {gw}</span><span class="badge">nginx {nginx}</span><span class="badge">ttyd {ttyd}</span><span class="badge">Action {action}</span></span></div>
</div>"##,
        access = config.access_mode,
        gateway_mode = config.gateway_mode,
        version = config.openclaw_version,
        gw = display_gateway_pid,
        nginx = nginx_pid,
        ttyd = ttyd_pid,
        action = action_pid
    ))
}

async fn diag_partial(State(state): State<AppState>) -> impl IntoResponse {
    let config = &state.config;
    Html(format!(
        r##"<h2>快速诊断</h2>
<div class="kvs">
  <div class="kv"><span class="key">MCP</span><span class="value">{mcp}</span></div>
  <div class="kv"><span class="key">Web Search</span><span class="value">{web}</span></div>
  <div class="kv"><span class="key">Memory Search</span><span class="value">{memory}</span></div>
</div>
<p class="hint" style="margin-top:14px;">
  这里只显示当前运行时真正暴露出来的能力状态，避免旧模板里那种重复脚本、乱码文本和信息堆叠导致的卡顿。
</p>"##,
        mcp = display_value(&config.mcp_status),
        web = display_value(&config.web_status),
        memory = display_value(&config.memory_status)
    ))
}
