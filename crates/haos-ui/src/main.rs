use axum::{
    Json, Router,
    extract::State,
    response::{Html, IntoResponse},
    routing::{get, post},
};
use std::{env, fs, net::SocketAddr, path::PathBuf, process::Command, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::RwLock,
    time::timeout,
};

const DEFAULT_CONFIG_PATH: &str = "/config/.openclaw/openclaw.json";
const DEFAULT_GATEWAY_PORT: &str = "18789";

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
    gateway_url: String,
    openclaw_version: String,
    gateway_token: String,
}

#[derive(Clone, Debug)]
struct SystemSnapshot {
    openclaw_uptime: String,
}

impl PageConfig {
    fn from_env() -> Self {
        let runtime_config = load_runtime_config();
        let gateway_token = runtime_config
            .as_ref()
            .and_then(|value| string_path(value, "gateway.auth.token"))
            .unwrap_or_default();

        Self {
            addon_version: env_value("ADDON_VERSION", "unknown"),
            gateway_url: env_value("GW_PUBLIC_URL", ""),
            openclaw_version: env_value("OPENCLAW_VERSION", "unknown"),
            gateway_token,
        }
    }
}

fn env_value(key: &str, fallback: &str) -> String {
    env::var(key).unwrap_or_else(|_| fallback.to_string())
}

fn runtime_config_path() -> PathBuf {
    env::var("OPENCLAW_CONFIG_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_CONFIG_PATH))
}

fn load_runtime_config() -> Option<serde_json::Value> {
    fs::read_to_string(runtime_config_path())
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
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

async fn fetch_openclaw_health() -> Option<bool> {
    let mut stream = timeout(
        Duration::from_millis(1500),
        TcpStream::connect("127.0.0.1:48099"),
    )
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
    timeout(
        Duration::from_millis(1500),
        stream.read_to_string(&mut response),
    )
    .await
    .ok()?
    .ok()?;

    Some(response.starts_with("HTTP/1.1 200") || response.starts_with("HTTP/1.0 200"))
}

fn format_duration(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    if days > 0 {
        format!("{days} 天 {hours} 小时 {minutes} 分钟")
    } else if hours > 0 {
        format!("{hours} 小时 {minutes} 分钟")
    } else {
        format!("{minutes} 分钟")
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

fn pid_value(name: &str) -> String {
    fs::read_to_string(format!("/run/openclaw-rs/{name}.pid"))
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "-".to_string())
}

async fn collect_system_snapshot() -> SystemSnapshot {
    tokio::task::spawn_blocking(|| {
        let openclaw_uptime =
            process_uptime(&pid_value("openclaw-gateway")).unwrap_or_else(|| "不可用".to_string());
        SystemSnapshot { openclaw_uptime }
    })
    .await
    .unwrap_or_else(|_| SystemSnapshot {
        openclaw_uptime: "不可用".to_string(),
    })
}

fn js_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

struct OpenClawCommandResult {
    ok: bool,
    stdout: String,
    stderr: String,
}

async fn run_openclaw_command(args: Vec<&'static str>) -> Result<OpenClawCommandResult, String> {
    tokio::task::spawn_blocking(move || {
        let output = Command::new("openclaw")
            .args(&args)
            .output()
            .map_err(|err| format!("无法执行 openclaw：{err}"))?;
        Ok(OpenClawCommandResult {
            ok: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
    })
    .await
    .map_err(|err| format!("后台任务失败：{err}"))?
}

async fn approve_latest_device() -> impl IntoResponse {
    match run_openclaw_command(vec!["devices", "approve", "--latest"]).await {
        Ok(result) if result.ok => Json(serde_json::json!({
            "ok": true,
            "message": if !result.stdout.is_empty() { result.stdout } else { "已确认最新授权请求".to_string() }
        })),
        Ok(result) => Json(serde_json::json!({
            "ok": false,
            "message": if !result.stderr.is_empty() { result.stderr } else { "确认授权失败".to_string() }
        })),
        Err(err) => Json(serde_json::json!({ "ok": false, "message": err })),
    }
}

async fn list_devices() -> impl IntoResponse {
    match run_openclaw_command(vec!["devices", "list", "--json"]).await {
        Ok(result) if result.ok => {
            let output = match serde_json::from_str::<serde_json::Value>(&result.stdout) {
                Ok(json) => {
                    serde_json::to_string_pretty(&json).unwrap_or_else(|_| result.stdout.clone())
                }
                Err(_) if result.stdout.is_empty() => "没有返回设备数据".to_string(),
                Err(_) => result.stdout.clone(),
            };
            Json(serde_json::json!({ "ok": true, "message": "已读取设备列表", "output": output }))
        }
        Ok(result) => Json(serde_json::json!({
            "ok": false,
            "message": if !result.stderr.is_empty() { result.stderr.clone() } else { "读取设备列表失败".to_string() },
            "output": if result.stdout.is_empty() { result.stderr } else { result.stdout }
        })),
        Err(err) => Json(serde_json::json!({ "ok": false, "message": err, "output": "" })),
    }
}

async fn index(State(state): State<AppState>) -> impl IntoResponse {
    let config = PageConfig::from_env();
    let guard = state.cache.read().await;
    let (snapshot, health_ok) = if let Some(cached) = guard.as_ref() {
        let result = (cached.snapshot.clone(), cached.health_ok);
        drop(guard);
        result
    } else {
        drop(guard);
        tokio::join!(collect_system_snapshot(), fetch_openclaw_health())
    };
    render_shell(&config, &snapshot, health_ok)
}

#[tokio::main]
async fn main() {
    let cache: Arc<RwLock<Option<CachedSnapshot>>> = Arc::new(RwLock::new(None));
    let cache_bg = cache.clone();
    tokio::spawn(async move {
        loop {
            let (snapshot, health_ok) =
                tokio::join!(collect_system_snapshot(), fetch_openclaw_health());
            *cache_bg.write().await = Some(CachedSnapshot {
                snapshot,
                health_ok,
            });
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });

    let app = Router::new()
        .route("/", get(index))
        .route("/action/devices-list", post(list_devices))
        .route(
            "/action/devices-approve-latest",
            post(approve_latest_device),
        )
        .with_state(AppState { cache });

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

fn render_shell(
    config: &PageConfig,
    snapshot: &SystemSnapshot,
    health_ok: Option<bool>,
) -> Html<String> {
    let gateway_url = js_string(&config.gateway_url);
    let gateway_token = js_string(&config.gateway_token);
    let gateway_pid = pid_value("openclaw-gateway");
    let (health_text, health_sub, tone) = match health_ok {
        Some(true) => ("已就绪", "Gateway 已通过健康检查，可直接进入。", "good"),
        Some(false) => ("异常", "Gateway 当前未通过健康检查。", "danger"),
        None if gateway_pid != "-" => ("等待确认", "已检测到 Gateway 进程，等待健康结果。", "warn"),
        None => ("离线", "未检测到 Gateway 进程。", "danger"),
    };
    let token_masked = if config.gateway_token.is_empty() {
        "未配置".to_string()
    } else {
        let suffix = config
            .gateway_token
            .get(config.gateway_token.len().saturating_sub(8)..)
            .unwrap_or(&config.gateway_token);
        format!("••••••••{suffix}")
    };

    Html(format!(
        r##"<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>OpenClaw 控制台</title>
<style>
:root {{
  --bg:#0f1628; --bg2:#0b1221; --panel:rgba(18,26,44,.92); --line:rgba(108,133,173,.22);
  --text:#f3f7fb; --muted:#9eb0c7; --cyan:#22c7ea; --teal:#1f6f77; --yellow:#f6c928;
  --good:#9ce6ca; --warn:#e3be5a; --danger:#f07b84;
}}
* {{ box-sizing:border-box; }}
body {{
  margin:0; color:var(--text);
  font:14px/1.65 "MiSans","HarmonyOS Sans SC","Noto Sans SC","Segoe UI","PingFang SC",sans-serif;
  background:
    radial-gradient(circle at 18% 0%, rgba(31,111,119,.28), transparent 28%),
    radial-gradient(circle at 82% 12%, rgba(34,199,234,.12), transparent 20%),
    linear-gradient(180deg, var(--bg2) 0%, var(--bg) 48%, #121a2d 100%);
}}
body::before {{
  content:""; position:fixed; inset:0; pointer-events:none; opacity:.14;
  background-image:
    linear-gradient(rgba(255,255,255,.03) 1px, transparent 1px),
    linear-gradient(90deg, rgba(255,255,255,.03) 1px, transparent 1px);
  background-size:28px 28px;
}}
.shell {{ max-width:920px; margin:0 auto; padding:22px 16px 36px; }}
.hero,.panel {{ border:1px solid var(--line); border-radius:24px; background:var(--panel); box-shadow:0 26px 72px rgba(0,0,0,.34); }}
.hero {{ padding:24px; }}
.topbar {{ display:flex; justify-content:space-between; gap:16px; margin-bottom:16px; }}
.topbar-title {{ font-size:16px; font-weight:800; }}
.topbar-sub,.copy,.meta,.hint {{ color:var(--muted); }}
.chip {{ display:inline-flex; align-items:center; min-height:32px; padding:0 12px; border:1px solid var(--line); border-radius:999px; }}
.brand {{ display:flex; gap:18px; align-items:center; margin-bottom:22px; }}
.mark {{ position:relative; width:92px; height:92px; display:grid; place-items:center; border-radius:999px; border:8px solid rgba(242,246,250,.96); background:var(--teal); }}
.mark .o {{ font-size:52px; font-weight:900; color:#fff; }}
.mark .line {{ position:absolute; right:-8px; top:18px; width:58px; height:8px; border-radius:999px; background:var(--cyan); }}
.mark .dot {{ position:absolute; right:15px; bottom:16px; width:14px; height:14px; border-radius:999px; background:var(--yellow); }}
.brand-sub {{ color:var(--muted); font-size:13px; text-transform:uppercase; letter-spacing:.12em; }}
.brand-rule {{ width:180px; height:3px; border-radius:999px; background:linear-gradient(90deg, var(--cyan), rgba(34,199,234,0)); }}
.hero-row {{ display:flex; justify-content:space-between; gap:18px; align-items:flex-end; }}
.kicker {{ color:var(--cyan); font-size:11px; font-weight:800; letter-spacing:.14em; text-transform:uppercase; margin-bottom:8px; }}
h1,h2 {{ margin:0; letter-spacing:-.03em; }}
h1 {{ font-size:28px; line-height:1.08; margin-bottom:8px; }}
.actions {{ display:flex; flex-wrap:wrap; gap:12px; }}
.btn {{ border:0; text-decoration:none; cursor:pointer; display:inline-flex; align-items:center; justify-content:center; min-height:44px; padding:0 18px; border-radius:999px; font-weight:700; font-size:14px; }}
.btn-primary {{ background:linear-gradient(180deg,#24d2ef,#1eb1cd); color:#08101c; }}
.btn-secondary {{ background:rgba(255,255,255,.02); color:var(--text); border:1px solid rgba(34,199,234,.34); }}
.grid3,.grid2 {{ display:grid; gap:16px; }}
.grid3 {{ grid-template-columns:repeat(3,minmax(0,1fr)); margin-top:16px; }}
.grid2 {{ grid-template-columns:repeat(2,minmax(0,1fr)); margin-top:16px; }}
.card {{ border:1px solid var(--line); border-radius:20px; background:rgba(8,14,27,.38); padding:20px; }}
.card .eyebrow {{ color:var(--muted); font-size:12px; margin-bottom:12px; letter-spacing:.14em; text-transform:uppercase; }}
.metric {{ font-size:22px; font-weight:800; }}
.good .metric {{ color:var(--good); }} .warn .metric {{ color:var(--warn); }} .danger .metric {{ color:var(--danger); }}
.status-pill {{ display:inline-flex; align-items:center; gap:8px; color:var(--good); font-weight:700; }}
.status-pill::before {{ content:""; width:10px; height:10px; border-radius:999px; background:currentColor; }}
.token {{ font-family:ui-monospace,Consolas,monospace; padding:12px 14px; border-radius:14px; border:1px solid rgba(34,199,234,.18); background:rgba(8,15,29,.52); overflow:auto; }}
.panel {{ padding:20px; margin-top:16px; }}
.panel-head {{ display:flex; justify-content:space-between; gap:16px; margin-bottom:14px; }}
pre {{ margin:12px 0 0; padding:14px 16px; border-radius:18px; border:1px solid rgba(34,199,234,.18); background:rgba(8,15,29,.52); color:#d9edf4; font:13px/1.6 ui-monospace,Consolas,monospace; white-space:pre-wrap; overflow:auto; }}
@media (max-width: 720px) {{
  .shell {{ padding:16px 12px 28px; }}
  .topbar,.brand,.hero-row,.panel-head {{ flex-direction:column; align-items:flex-start; }}
  .grid3,.grid2 {{ grid-template-columns:1fr; }}
  .actions {{ width:100%; }}
  .actions .btn {{ flex:1 1 100%; }}
}}
</style>
</head>
<body>
<div class="shell">
  <div class="topbar">
    <div>
      <div class="topbar-title">OpenClaw Add-on</div>
      <div class="topbar-sub">Hermes 色板的深色单页入口</div>
    </div>
    <div class="chip">Add-on {addon_version}</div>
  </div>
  <section class="hero">
    <div class="brand">
      <div class="mark"><span class="o">O</span><span class="line"></span><span class="dot"></span></div>
      <div>
        <div class="brand-sub">Home Assistant Ingress</div>
        <h1>OpenClaw 控制台</h1>
        <div class="brand-rule"></div>
      </div>
    </div>
    <div class="hero-row">
      <div>
        <div class="kicker">Gateway Shell</div>
        <p class="copy">先保留两种方式一起测。第一个按钮走原生 HTTPS 网关，第二个按钮走 HAOS Ingress 测试入口，第三个按钮进入维护 Shell。</p>
      </div>
      <div class="actions">
        <a class="btn btn-primary" id="ocGatewayLink" href="#" target="_blank" rel="noopener noreferrer" onclick="return ocOpenGatewayLink(event,this)">打开网关</a>
        <button class="btn btn-secondary" type="button" onclick="ocOpenIngressGatewayWindow()">HAOS 网关（测试）</button>
        <button class="btn btn-secondary" type="button" onclick="ocOpenShellWindow()">维护 Shell</button>
      </div>
    </div>
  </section>

  <div class="grid3">
    <section class="card {tone}">
      <div class="eyebrow">网关状态</div>
      <div class="metric">{health_text}</div>
      <div class="copy">{health_sub}</div>
    </section>
    <section class="card">
      <div class="eyebrow">OpenClaw Gateway</div>
      <div class="status-pill">{gateway_state}</div>
      <div class="meta">PID {gateway_pid}</div>
    </section>
    <section class="card">
      <div class="eyebrow">访问令牌</div>
      <div class="token" id="ocTokenVal">{token_masked}</div>
      <div class="actions" style="margin-top:12px">
        <button class="btn btn-secondary" id="ocTokenToggleBtn" type="button" onclick="ocToggleToken()">显示</button>
        <button class="btn btn-secondary" type="button" onclick="ocCopyToken(this)">复制</button>
      </div>
    </section>
  </div>

  <div class="grid2">
    <section class="card">
      <div class="eyebrow">原生入口</div>
      <div class="metric" style="font-size:16px">https://主机:{gateway_port}/#token=...</div>
      <div class="meta">直接进入外部 HTTPS Gateway</div>
    </section>
    <section class="card">
      <div class="eyebrow">HAOS 入口</div>
      <div class="metric" style="font-size:16px">./gateway/#token=...</div>
      <div class="meta">沿着 Home Assistant Ingress 走的测试入口</div>
    </section>
  </div>

  <section class="panel">
    <div class="panel-head">
      <div>
        <div class="kicker">Realtime Status</div>
        <h2>运行信息</h2>
      </div>
    </div>
    <div class="grid2">
      <div class="card"><div class="eyebrow">OpenClaw 版本</div><div class="metric" style="font-size:18px">{openclaw_version}</div></div>
      <div class="card"><div class="eyebrow">运行时长</div><div class="metric" style="font-size:18px">{openclaw_uptime}</div></div>
    </div>
  </section>

  <section class="panel">
    <div class="panel-head">
      <div>
        <div class="kicker">Device Approval</div>
        <h2>授权提醒与确认</h2>
      </div>
    </div>
    <p class="hint">这里直接执行官方 <code>openclaw devices</code> 命令。新设备登录后，先看列表，再确认最新授权。</p>
    <div class="actions" style="margin-top:16px">
      <button class="btn btn-secondary" type="button" onclick="ocListDevices('deviceListStatus','deviceListOutput')">列出待批准设备</button>
      <button class="btn btn-primary" type="button" onclick="ocApproveLatestDevice('deviceApproveStatus')">确认最新授权</button>
    </div>
    <div class="hint" id="deviceListStatus" style="margin-top:12px">页面会直接执行官方 <code>openclaw devices list --json</code></div>
    <pre id="deviceListOutput">点击“列出待批准设备”后，这里会显示 pending 与 paired 设备快照。</pre>
    <div class="hint" id="deviceApproveStatus" style="margin-top:12px">按钮会在本机执行官方 <code>openclaw devices approve --latest</code></div>
  </section>
</div>
<script>
const OC_GATEWAY_URL = {gateway_url};
const OC_GATEWAY_TOKEN = {gateway_token};
const OC_GATEWAY_PORT = "{gateway_port}";

function appendTokenHash(url) {{
  if (!OC_GATEWAY_TOKEN || !String(OC_GATEWAY_TOKEN).trim()) return url;
  return String(url).replace(/#.*$/, "") + "#token=" + encodeURIComponent(String(OC_GATEWAY_TOKEN).trim());
}}

function ocBaseGatewayUrl() {{
  if (OC_GATEWAY_URL && OC_GATEWAY_URL.trim()) return OC_GATEWAY_URL.trim();
  return "https://" + window.location.hostname + ":" + OC_GATEWAY_PORT + "/";
}}

function ocGatewayHref() {{ return appendTokenHash(ocBaseGatewayUrl()); }}
function ocIngressGatewayHref() {{ return appendTokenHash(new URL("./gateway/", window.location.href).toString()); }}

function openAddonWindow(url, name) {{
  const win = window.open(url, name, "popup=yes,width=1440,height=920,noopener,noreferrer");
  if (!win) window.location.href = url;
  return false;
}}

function syncGatewayLink() {{
  const link = document.getElementById("ocGatewayLink");
  if (link) link.href = ocGatewayHref();
}}

function ocOpenGatewayLink(event, anchor) {{
  if (event) event.preventDefault();
  const targetUrl = anchor && anchor.href ? anchor.href : ocGatewayHref();
  return openAddonWindow(targetUrl, "openclaw-gateway");
}}

function ocOpenIngressGatewayWindow() {{
  return openAddonWindow(ocIngressGatewayHref(), "openclaw-gateway-haos");
}}

function ocOpenShellWindow() {{
  return openAddonWindow(new URL("./shell/", window.location.href).toString(), "openclaw-shell");
}}

async function ocPostJson(url, payload) {{
  const resp = await fetch(url, {{
    method: "POST",
    headers: {{ "Content-Type": "application/json" }},
    body: JSON.stringify(payload || {{}})
  }});
  const data = await resp.json().catch(() => ({{ ok: false, message: "返回格式无效" }}));
  if (!resp.ok && !data.ok) throw new Error(data.message || "请求失败");
  return data;
}}

function ocSetFormStatus(id, message, ok) {{
  const el = document.getElementById(id);
  if (!el) return;
  el.textContent = message;
  el.style.color = ok === false ? "#f07b84" : (ok === true ? "#9ce6ca" : "#9eb0c7");
}}

window.ocApproveLatestDevice = async function(statusId) {{
  ocSetFormStatus(statusId, "正在执行授权…");
  try {{
    const data = await ocPostJson("./action/devices-approve-latest", {{}});
    ocSetFormStatus(statusId, data.message || "已完成", !!data.ok);
  }} catch (error) {{
    ocSetFormStatus(statusId, "执行失败：" + (error.message || error), false);
  }}
}};

window.ocListDevices = async function(statusId, outputId) {{
  ocSetFormStatus(statusId, "正在读取设备列表…");
  const output = document.getElementById(outputId);
  if (output) output.textContent = "正在读取…";
  try {{
    const data = await ocPostJson("./action/devices-list", {{}});
    ocSetFormStatus(statusId, data.message || "已完成", !!data.ok);
    if (output) output.textContent = data.output || "没有返回设备数据";
  }} catch (error) {{
    ocSetFormStatus(statusId, "读取失败：" + (error.message || error), false);
    if (output) output.textContent = "读取失败：" + (error.message || error);
  }}
}};

(function() {{
  const t = OC_GATEWAY_TOKEN || "";
  window.ocToggleToken = function() {{
    const v = document.getElementById("ocTokenVal");
    const b = document.getElementById("ocTokenToggleBtn");
    if (!v || !b) return;
    if (b.dataset.vis === "1") {{
      v.textContent = t ? "••••••••" + t.slice(-8) : "未配置";
      b.textContent = "显示";
      b.dataset.vis = "";
    }} else {{
      v.textContent = t || "未配置";
      b.textContent = "隐藏";
      b.dataset.vis = "1";
    }}
  }};
  window.ocCopyToken = function(btn) {{
    if (!t) return;
    const orig = btn.textContent;
    function done() {{
      btn.textContent = "已复制";
      setTimeout(function() {{ btn.textContent = orig; }}, 1500);
    }}
    function fallback() {{
      try {{
        var ta = document.createElement("textarea");
        ta.value = t;
        ta.style.cssText = "position:fixed;opacity:0;top:0;left:0;width:1px;height:1px";
        document.body.appendChild(ta);
        ta.focus();
        ta.select();
        if (document.execCommand("copy")) done();
        document.body.removeChild(ta);
      }} catch (_) {{
        alert("Token: " + t);
      }}
    }}
    if (navigator.clipboard) navigator.clipboard.writeText(t).then(done, fallback);
    else fallback();
  }};
  syncGatewayLink();
}})();
</script>
</body>
</html>"##,
        addon_version = html_escape(&config.addon_version),
        gateway_port = DEFAULT_GATEWAY_PORT,
        gateway_url = gateway_url,
        gateway_token = gateway_token,
        tone = tone,
        health_text = health_text,
        health_sub = health_sub,
        gateway_state = if gateway_pid != "-" {
            "在线"
        } else {
            "离线"
        },
        gateway_pid = html_escape(&gateway_pid),
        token_masked = html_escape(&token_masked),
        openclaw_version = html_escape(&config.openclaw_version),
        openclaw_uptime = html_escape(&snapshot.openclaw_uptime)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_page_config() -> PageConfig {
        PageConfig {
            addon_version: "2026.04.15.8".to_string(),
            gateway_url: String::new(),
            openclaw_version: "2026.4.14".to_string(),
            gateway_token: "tok_test_12345678".to_string(),
        }
    }

    #[test]
    fn render_shell_keeps_single_page_controls() {
        let config = sample_page_config();
        let snapshot = SystemSnapshot {
            openclaw_uptime: "35 分钟".to_string(),
        };
        let Html(html) = render_shell(&config, &snapshot, Some(true));
        assert!(html.contains("OpenClaw 控制台"));
        assert!(html.contains("HAOS 网关（测试）"));
        assert!(html.contains("ocOpenIngressGatewayWindow"));
        assert!(html.contains("#token="));
    }

    #[test]
    fn device_actions_are_present() {
        let config = sample_page_config();
        let snapshot = SystemSnapshot {
            openclaw_uptime: "35 分钟".to_string(),
        };
        let Html(html) = render_shell(&config, &snapshot, Some(true));
        assert!(html.contains("列出待批准设备"));
        assert!(html.contains("确认最新授权"));
        assert!(html.contains("维护 Shell"));
    }
}
