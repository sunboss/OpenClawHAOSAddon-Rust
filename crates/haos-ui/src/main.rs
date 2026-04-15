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

fn load_json_file(path: PathBuf) -> Option<serde_json::Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
}

fn load_runtime_config() -> Option<serde_json::Value> {
    load_json_file(runtime_config_path())
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
            "message": if !result.stdout.is_empty() {
                result.stdout
            } else if !result.stderr.is_empty() {
                result.stderr
            } else {
                "已执行 openclaw devices approve --latest".to_string()
            }
        })),
        Ok(result) => Json(serde_json::json!({
            "ok": false,
            "message": if !result.stderr.is_empty() {
                result.stderr
            } else if !result.stdout.is_empty() {
                result.stdout
            } else {
                "确认授权失败，请先查看待批准设备。".to_string()
            }
        })),
        Err(err) => Json(serde_json::json!({ "ok": false, "message": err })),
    }
}

async fn list_devices() -> impl IntoResponse {
    match run_openclaw_command(vec!["devices", "list", "--json"]).await {
        Ok(result) if result.ok => {
            let output = if result.stdout.is_empty() {
                "没有返回设备数据".to_string()
            } else {
                match serde_json::from_str::<serde_json::Value>(&result.stdout) {
                    Ok(json) => serde_json::to_string_pretty(&json)
                        .unwrap_or_else(|_| result.stdout.clone()),
                    Err(_) => result.stdout.clone(),
                }
            };

            let parsed = serde_json::from_str::<serde_json::Value>(&result.stdout).ok();
            let pending = parsed
                .as_ref()
                .and_then(|value| value.get("pending"))
                .and_then(|value| value.as_array())
                .map(|items| items.len())
                .unwrap_or(0);
            let paired = parsed
                .as_ref()
                .and_then(|value| value.get("paired"))
                .and_then(|value| value.as_array())
                .map(|items| items.len())
                .unwrap_or(0);

            Json(serde_json::json!({
                "ok": true,
                "message": format!("已读取设备列表：pending {pending}，paired {paired}"),
                "output": output
            }))
        }
        Ok(result) => Json(serde_json::json!({
            "ok": false,
            "message": if !result.stderr.is_empty() {
                result.stderr.clone()
            } else {
                "读取设备列表失败".to_string()
            },
            "output": if result.stdout.is_empty() {
                result.stderr
            } else {
                result.stdout
            }
        })),
        Err(err) => Json(serde_json::json!({
            "ok": false,
            "message": err,
            "output": ""
        })),
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

    render_shell(
        &config,
        "OpenClaw Gateway",
        "这里只保留最常用的两个入口：原生网关和维护 Shell。状态、令牌与授权确认也都集中在这一页。",
        &home_content(&config, &snapshot, health_ok),
    )
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

    let app_state = AppState { cache };
    let app = Router::new()
        .route("/", get(index))
        .route("/action/devices-list", post(list_devices))
        .route(
            "/action/devices-approve-latest",
            post(approve_latest_device),
        )
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

fn primary_link_button(label: &str, id: &str, onclick: &str) -> String {
    format!(
        r##"<a class="btn btn-primary" id="{id}" href="#" target="_blank" rel="noopener noreferrer" onclick="{onclick}">{label}</a>"##
    )
}

fn shell_window_button(label: &str) -> String {
    format!(
        r#"<button class="btn btn-secondary" type="button" onclick="ocOpenShellWindow()">{label}</button>"#
    )
}

fn kv_row(label: &str, value: &str) -> String {
    format!(
        r#"<div class="kv-row"><span class="kv-label">{label}</span><span class="kv-value">{value}</span></div>"#
    )
}

fn health_strip(value: &str, sub: &str, tone: &str) -> String {
    format!(
        r#"<article class="health-strip {tone}"><div class="health-strip-label">Gateway</div><div class="health-strip-value">{value}</div><div class="health-strip-sub">{sub}</div></article>"#
    )
}

fn service_badge(label: &str, pid: &str) -> String {
    let (state_class, state_text, pid_text) = if pid != "-" {
        ("is-online", "在线", format!("PID {pid}"))
    } else {
        ("is-offline", "离线", "未检测到 PID".to_string())
    };

    format!(
        r#"<article class="service-badge {state_class}"><div class="service-badge-head"><span class="service-name"><span class="service-dot"></span>{label}</span><span class="service-state">{state_text}</span></div><div class="service-meta">{pid_text}</div></article>"#
    )
}

fn gateway_token_section(token: &str) -> String {
    if token.is_empty() {
        return String::new();
    }

    let suffix = token.get(token.len().saturating_sub(8)..).unwrap_or(token);
    let masked = format!("••••••••{suffix}");
    let escaped = token.replace('\\', "\\\\").replace('"', "\\\"");

    format!(
        r#"<section class="panel panel-soft"><div class="panel-head"><div><div class="panel-kicker">Gateway Token</div><h2>显示访问令牌</h2></div></div><p class="panel-copy">打开原生网关会自动带上 token。这里保留显示，只用于人工核对和手动调试。</p><div class="token-box"><div class="token-head"><span>访问令牌</span><span>不要分享给不受信任的设备</span></div><div class="token-row"><code class="token-value" id="ocTokenVal">{masked}</code><button class="btn btn-secondary" id="ocTokenToggleBtn" type="button" onclick="ocToggleToken()">显示</button><button class="btn btn-secondary" type="button" onclick="ocCopyToken(this)">复制</button></div></div><script>(function(){{var t="{escaped}";window.ocToggleToken=function(){{var v=document.getElementById("ocTokenVal"),b=document.getElementById("ocTokenToggleBtn");if(!v||!b)return;if(b.dataset.vis==="1"){{v.textContent="••••••••"+t.slice(-8);b.textContent="显示";b.dataset.vis="";}}else{{v.textContent=t;b.textContent="隐藏";b.dataset.vis="1";}}}};window.ocCopyToken=function(btn){{var orig=btn.textContent;function done(){{btn.textContent="已复制";setTimeout(function(){{btn.textContent=orig;}},1500);}}function fallback(){{try{{var ta=document.createElement("textarea");ta.value=t;ta.style.cssText="position:fixed;opacity:0;top:0;left:0;width:1px;height:1px";document.body.appendChild(ta);ta.focus();ta.select();var ok=document.execCommand("copy");document.body.removeChild(ta);if(ok){{done();}}else{{alert("Token: "+t);}}}}catch(e){{alert("Token: "+t);}}}}if(navigator.clipboard){{navigator.clipboard.writeText(t).then(done,fallback);}}else{{fallback();}}}};}})()</script></section>"#,
        masked = masked,
        escaped = escaped
    )
}

fn home_content(config: &PageConfig, snapshot: &SystemSnapshot, health_ok: Option<bool>) -> String {
    let gateway_pid = pid_value("openclaw-gateway");
    let (health_text, health_sub, health_tone) = match health_ok {
        Some(true) => ("运行正常", "Gateway 已通过健康检查。", "tone-good"),
        Some(false) => ("状态异常", "Gateway 当前未通过健康检查。", "tone-danger"),
        None if gateway_pid != "-" => (
            "等待确认",
            "已检测到 Gateway 进程，等待健康结果。",
            "tone-warn",
        ),
        None => ("当前离线", "未检测到 Gateway 进程。", "tone-danger"),
    };

    format!(
        r#"<div class="stack"><section class="hero-shell"><div class="brand-lockup"><div class="brand-mark"><span class="brand-letter">O</span><span class="brand-accent"></span><span class="brand-dot"></span></div><div class="brand-copy"><div class="brand-name">OpenClaw Agent</div><div class="brand-sub">Hermes-style thin shell for Home Assistant</div><div class="brand-line"></div></div></div><div class="hero-body"><div><div class="hero-kicker">Gateway Shell</div><h1>单页控制壳</h1><p class="hero-copy">这里不再做第二套控制台。打开网关会固定走 <code>https://</code> 并自动带上 token；维护 Shell 会直接进入完整的 Web Shell。</p></div><div class="hero-actions">{open_gateway}{open_shell}</div></div></section><section class="panel"><div class="panel-head"><div><div class="panel-kicker">Realtime Status</div><h2>OpenClaw Gateway 状态</h2></div>{health}</div><div class="status-grid">{gateway_badge}<div class="status-meta">{version_row}{uptime_row}{entry_row}</div></div></section>{token_section}<section class="panel panel-soft"><div class="panel-head"><div><div class="panel-kicker">Device Approval</div><h2>授权提醒与确认</h2></div></div><p class="panel-copy">这里直接执行官方 <code>openclaw devices</code> 命令。新设备登录后，先看列表，再确认最新授权。</p><div class="hero-actions"><button class="btn btn-secondary" type="button" onclick="ocListDevices('deviceListStatus','deviceListOutput')">列出待批准设备</button><button class="btn btn-primary" type="button" onclick="ocApproveLatestDevice('deviceApproveStatus')">确认最新授权</button></div><span class="form-status" id="deviceListStatus">页面会直接执行官方 <code>openclaw devices list --json</code></span><pre class="command-output" id="deviceListOutput">点击“列出待批准设备”后，这里会显示 pending 与 paired 设备快照。</pre><span class="form-status" id="deviceApproveStatus">按钮会在本机执行官方 <code>openclaw devices approve --latest</code></span></section></div>"#,
        open_gateway = primary_link_button(
            "打开网关",
            "ocGatewayLink",
            "return ocOpenGatewayLink(event, this)"
        ),
        open_shell = shell_window_button("维护 Shell"),
        health = health_strip(health_text, health_sub, health_tone),
        gateway_badge = service_badge("OpenClaw Gateway", &gateway_pid),
        version_row = kv_row("OpenClaw 版本", &config.openclaw_version),
        uptime_row = kv_row("运行时长", &snapshot.openclaw_uptime),
        entry_row = kv_row("入口", "https://主机:18789/#token=..."),
        token_section = gateway_token_section(&config.gateway_token)
    )
}

fn render_shell(config: &PageConfig, title: &str, subtitle: &str, content: &str) -> Html<String> {
    let gateway_url = js_string(&config.gateway_url);
    let gateway_token = js_string(&config.gateway_token);

    Html(format!(
        r##"<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
:root {{
  --bg: #0f1628;
  --bg-deep: #0b1221;
  --panel: rgba(17, 25, 43, 0.92);
  --panel-soft: rgba(20, 30, 50, 0.96);
  --line: rgba(108, 133, 173, 0.24);
  --line-strong: rgba(34, 199, 234, 0.34);
  --text: #f3f7fb;
  --muted: #9eb0c7;
  --muted-strong: #c8d6e7;
  --teal: #1f6f77;
  --cyan: #22c7ea;
  --yellow: #f6c928;
  --good: #9ce6ca;
  --danger: #f07b84;
  --warn: #e3be5a;
  --shadow: 0 26px 72px rgba(0, 0, 0, 0.34);
}}
* {{ box-sizing: border-box; }}
html, body {{ min-height: 100%; }}
body {{
  margin: 0;
  background:
    radial-gradient(circle at 18% 0%, rgba(31, 111, 119, 0.28), transparent 28%),
    radial-gradient(circle at 82% 12%, rgba(34, 199, 234, 0.12), transparent 20%),
    linear-gradient(180deg, var(--bg-deep) 0%, var(--bg) 48%, #121a2d 100%);
  color: var(--text);
  font: 14px/1.65 "MiSans", "HarmonyOS Sans SC", "Noto Sans SC", "Segoe UI", "PingFang SC", sans-serif;
}}
body::before {{
  content: "";
  position: fixed;
  inset: 0;
  pointer-events: none;
  opacity: 0.14;
  background-image:
    linear-gradient(rgba(255,255,255,0.03) 1px, transparent 1px),
    linear-gradient(90deg, rgba(255,255,255,0.03) 1px, transparent 1px);
  background-size: 28px 28px;
}}
.shell {{
  max-width: 920px;
  margin: 0 auto;
  padding: 22px 16px 36px;
}}
.topbar {{
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 16px;
}}
.topbar-copy {{
  display: grid;
  gap: 4px;
}}
.topbar-title {{
  font-size: 16px;
  font-weight: 800;
  letter-spacing: -0.02em;
}}
.topbar-sub {{
  color: var(--muted);
  font-size: 12px;
}}
.version-chip {{
  display: inline-flex;
  align-items: center;
  min-height: 32px;
  padding: 0 12px;
  border-radius: 999px;
  border: 1px solid var(--line);
  background: rgba(255,255,255,0.03);
  color: var(--muted-strong);
  font-size: 12px;
  font-weight: 700;
}}
.hero {{
  padding: 22px 22px 0;
  border-radius: 26px;
  border: 1px solid var(--line);
  background: linear-gradient(180deg, rgba(17,25,43,0.95), rgba(12,19,34,0.94));
  box-shadow: var(--shadow);
}}
.hero-head {{
  margin-bottom: 18px;
}}
.hero-kicker {{
  color: var(--cyan);
  font-size: 11px;
  font-weight: 800;
  letter-spacing: 0.14em;
  text-transform: uppercase;
  margin-bottom: 8px;
}}
h1 {{
  margin: 0 0 8px;
  font-size: 28px;
  line-height: 1.08;
  letter-spacing: -0.04em;
}}
.subtitle {{
  margin: 0;
  max-width: 720px;
  color: var(--muted);
}}
.stack {{
  display: grid;
  gap: 16px;
}}
.hero-shell,
.panel {{
  border-radius: 24px;
  border: 1px solid var(--line);
  background: var(--panel);
  box-shadow: var(--shadow);
}}
.hero-shell {{
  padding: 24px;
}}
.brand-lockup {{
  display: flex;
  align-items: center;
  gap: 18px;
  margin-bottom: 22px;
}}
.brand-mark {{
  position: relative;
  width: 92px;
  height: 92px;
  display: grid;
  place-items: center;
  border-radius: 999px;
  border: 8px solid rgba(242, 246, 250, 0.96);
  background: var(--teal);
}}
.brand-letter {{
  font-size: 52px;
  line-height: 1;
  font-weight: 900;
  color: white;
  letter-spacing: -0.04em;
}}
.brand-accent {{
  position: absolute;
  right: -8px;
  top: 18px;
  width: 58px;
  height: 8px;
  border-radius: 999px;
  background: var(--cyan);
}}
.brand-dot {{
  position: absolute;
  right: 15px;
  bottom: 16px;
  width: 14px;
  height: 14px;
  border-radius: 999px;
  background: var(--yellow);
}}
.brand-copy {{
  display: grid;
  gap: 6px;
}}
.brand-name {{
  font-size: 20px;
  font-weight: 800;
  letter-spacing: -0.03em;
}}
.brand-sub {{
  color: var(--muted);
  font-size: 13px;
}}
.brand-line {{
  width: 180px;
  height: 3px;
  border-radius: 999px;
  background: linear-gradient(90deg, var(--cyan), rgba(34,199,234,0));
}}
.hero-body {{
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: 18px;
}}
.hero-copy,
.panel-copy {{
  color: var(--muted);
  margin: 0;
}}
.hero-actions {{
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
}}
.btn {{
  appearance: none;
  border: 0;
  cursor: pointer;
  text-decoration: none;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-height: 44px;
  padding: 0 18px;
  border-radius: 999px;
  font-weight: 700;
  font-size: 14px;
  transition: transform .18s ease, opacity .18s ease, background .18s ease, border-color .18s ease;
}}
.btn:hover {{
  transform: translateY(-1px);
}}
.btn-primary {{
  background: linear-gradient(180deg, #24d2ef, #1eb1cd);
  color: #08101c;
}}
.btn-secondary {{
  background: rgba(255,255,255,0.02);
  color: var(--text);
  border: 1px solid var(--line-strong);
}}
.panel {{
  padding: 20px;
}}
.panel-soft {{
  background: var(--panel-soft);
}}
.panel-head {{
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 14px;
}}
.panel-kicker {{
  color: var(--cyan);
  font-size: 11px;
  font-weight: 800;
  letter-spacing: 0.14em;
  text-transform: uppercase;
  margin-bottom: 6px;
}}
.panel h2 {{
  margin: 0;
  font-size: 20px;
  letter-spacing: -0.03em;
}}
.health-strip {{
  min-width: 220px;
  padding: 14px 16px;
  border-radius: 18px;
  border: 1px solid var(--line);
  background: rgba(255,255,255,0.02);
}}
.health-strip-label {{
  color: var(--muted);
  font-size: 12px;
  margin-bottom: 4px;
}}
.health-strip-value {{
  font-size: 22px;
  font-weight: 800;
  letter-spacing: -0.04em;
}}
.health-strip-sub {{
  color: var(--muted);
  font-size: 12px;
  margin-top: 4px;
}}
.tone-good {{
  border-color: rgba(156,230,202,0.24);
}}
.tone-good .health-strip-value {{
  color: var(--good);
}}
.tone-warn {{
  border-color: rgba(227,190,90,0.24);
}}
.tone-warn .health-strip-value {{
  color: var(--warn);
}}
.tone-danger {{
  border-color: rgba(240,123,132,0.24);
}}
.tone-danger .health-strip-value {{
  color: var(--danger);
}}
.status-grid {{
  display: grid;
  gap: 16px;
}}
.service-badge {{
  padding: 16px;
  border-radius: 18px;
  border: 1px solid var(--line);
  background: rgba(8, 14, 27, 0.46);
}}
.service-badge-head {{
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}}
.service-name {{
  display: inline-flex;
  align-items: center;
  gap: 10px;
  font-size: 15px;
  font-weight: 700;
}}
.service-dot {{
  width: 10px;
  height: 10px;
  border-radius: 999px;
  background: currentColor;
}}
.service-meta,
.service-state {{
  color: var(--muted);
  font-size: 12px;
}}
.service-badge.is-online {{
  border-color: rgba(156,230,202,0.24);
  color: var(--good);
}}
.service-badge.is-offline {{
  border-color: rgba(240,123,132,0.24);
  color: var(--danger);
}}
.status-meta {{
  display: grid;
  gap: 10px;
}}
.kv-row {{
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 12px 14px;
  border-radius: 16px;
  background: rgba(255,255,255,0.02);
  border: 1px solid rgba(255,255,255,0.04);
}}
.kv-label {{
  color: var(--muted);
}}
.kv-value {{
  color: var(--muted-strong);
  font-weight: 700;
  text-align: right;
}}
.token-box {{
  display: grid;
  gap: 12px;
  padding: 14px;
  border-radius: 18px;
  border: 1px solid var(--line);
  background: rgba(255,255,255,0.02);
}}
.token-head,
.token-row {{
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}}
.token-head {{
  color: var(--muted);
  font-size: 12px;
}}
.token-value {{
  flex: 1 1 auto;
  display: inline-flex;
  min-height: 44px;
  align-items: center;
  padding: 0 14px;
  border-radius: 14px;
  border: 1px solid rgba(34, 199, 234, 0.18);
  background: rgba(8, 15, 29, 0.52);
  color: #d9edf4;
  overflow: auto;
}}
.form-status {{
  display: block;
  color: var(--muted);
  margin-top: 10px;
}}
.command-output {{
  margin: 12px 0 0;
  padding: 14px 16px;
  border-radius: 18px;
  border: 1px solid rgba(34, 199, 234, 0.18);
  background: rgba(8, 15, 29, 0.52);
  color: #d9edf4;
  font: 13px/1.6 ui-monospace, "Cascadia Code", Consolas, monospace;
  white-space: pre-wrap;
  overflow: auto;
}}
code {{
  font-family: ui-monospace, "Cascadia Code", Consolas, monospace;
}}
@media (max-width: 720px) {{
  .shell {{ padding: 16px 12px 28px; }}
  .topbar,
  .panel-head,
  .token-head,
  .kv-row,
  .brand-lockup,
  .hero-body {{
    flex-direction: column;
    align-items: flex-start;
  }}
  .version-chip {{ align-self: flex-start; }}
  .hero-actions,
  .token-row {{
    width: 100%;
  }}
  .hero-actions .btn,
  .token-row .btn {{
    flex: 1 1 100%;
  }}
  .kv-value {{
    text-align: left;
  }}
  .brand-line {{
    width: 120px;
  }}
}}
</style>
</head>
<body>
<div class="shell">
  <header class="topbar">
    <div class="topbar-copy">
      <div class="topbar-title">OpenClaw Add-on</div>
      <div class="topbar-sub">Hermes 色板的深色单页入口</div>
    </div>
    <div class="version-chip">Add-on {addon_version}</div>
  </header>
  <section class="hero">
    <div class="hero-head">
      <div class="hero-kicker">Gateway Shell</div>
      <h1>{title}</h1>
      <p class="subtitle">{subtitle}</p>
    </div>
    <main class="stack">{content}</main>
  </section>
</div>
<script>
const OC_GATEWAY_URL = {gateway_url};
const OC_GATEWAY_TOKEN = {gateway_token};
const OC_GATEWAY_PORT = "{gateway_port}";

function ocBaseGatewayUrl() {{
  if (OC_GATEWAY_URL && OC_GATEWAY_URL.trim()) return OC_GATEWAY_URL.trim();
  return "https://" + window.location.hostname + ":" + OC_GATEWAY_PORT + "/";
}}

function ocGatewayHref() {{
  const base = ocBaseGatewayUrl().replace(/#.*$/, "");
  if (!OC_GATEWAY_TOKEN || !String(OC_GATEWAY_TOKEN).trim()) return base;
  return base + "#token=" + encodeURIComponent(String(OC_GATEWAY_TOKEN).trim());
}}

function openAddonWindow(url, name) {{
  const win = window.open(url, name, "popup=yes,width=1440,height=920,noopener,noreferrer");
  if (!win) {{
    window.location.href = url;
  }}
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

function ocOpenShellWindow() {{
  const shellUrl = new URL("./shell/", window.location.href).toString();
  return openAddonWindow(shellUrl, "openclaw-shell");
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

syncGatewayLink();
</script>
</body>
</html>"##,
        title = title,
        subtitle = subtitle,
        addon_version = html_escape(&config.addon_version),
        content = content,
        gateway_url = gateway_url,
        gateway_token = gateway_token,
        gateway_port = DEFAULT_GATEWAY_PORT
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
        let Html(html) = render_shell(&config, "标题", "副标题", "<div>content</div>");

        assert!(html.contains("OpenClaw Add-on"));
        assert!(html.contains("Hermes 色板的深色单页入口"));
        assert!(html.contains("ocOpenGatewayLink"));
        assert!(html.contains("ocOpenShellWindow"));
        assert!(html.contains("#token="));
    }

    #[test]
    fn home_page_keeps_only_required_actions() {
        let config = sample_page_config();
        let snapshot = SystemSnapshot {
            openclaw_uptime: "35 分钟".to_string(),
        };

        let html = home_content(&config, &snapshot, Some(true));
        assert!(html.contains("打开网关"));
        assert!(html.contains("维护 Shell"));
        assert!(html.contains("OpenClaw Gateway 状态"));
        assert!(html.contains("显示访问令牌"));
        assert!(html.contains("列出待批准设备"));
        assert!(html.contains("确认最新授权"));
    }
}
