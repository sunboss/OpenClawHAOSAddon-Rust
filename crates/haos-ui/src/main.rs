use axum::{
    Json, Router,
    extract::State,
    response::{Html, IntoResponse},
    routing::{get, post},
};
use std::{env, fs, path::PathBuf, process::Command, sync::Arc, time::Duration};
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
        let config = load_runtime_config();
        let gateway_token = config
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

fn html_attr_escape(value: &str) -> String {
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
                "授权失败，请先检查待批准设备列表".to_string()
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
        "OpenClaw Gateway 单页入口",
        "参考 Hermes Add-on 的薄壳结构，只保留打开网关、维护 Shell、状态、令牌和授权确认。",
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
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind ui listener");
    println!("haos-ui: listening on http://{addr}");
    axum::serve(listener, app).await.expect("serve ui");
}

fn primary_link_button(label: &str, id: &str, onclick: &str) -> String {
    format!(
        r##"<a class="btn primary" id="{id}" href="#" target="_blank" rel="noopener noreferrer" onclick="{onclick}">{label}</a>"##
    )
}

fn shell_window_button(label: &str) -> String {
    format!(
        r#"<button class="btn secondary" type="button" onclick="ocOpenShellWindow()">{label}</button>"#
    )
}

fn kv_row(label: &str, value: &str) -> String {
    format!(
        r#"<div class="kv-row"><span class="kv-label">{label}</span><span class="kv-value">{value}</span></div>"#
    )
}

fn summary_strip(title: &str, value: &str, sub: &str, tone: &str) -> String {
    format!(
        r#"<article class="summary-strip-card {tone}"><div class="summary-strip-title">{title}</div><div class="summary-strip-value">{value}</div><div class="summary-strip-sub">{sub}</div></article>"#
    )
}

fn service_badge(label: &str, pid: &str) -> String {
    let (state_class, state_text, pid_text) = if pid != "-" {
        ("is-online", "在线", format!("PID {pid}"))
    } else {
        ("is-offline", "待启动", "未检测到 PID".to_string())
    };

    format!(
        r#"<article class="service-badge {state_class}"><div class="service-badge-top"><span class="service-name"><span class="svc-dot"></span>{label}</span><span class="service-state">{state_text}</span></div><div class="service-meta">{pid_text}</div></article>"#
    )
}

fn gateway_token_section(token: &str) -> String {
    if token.is_empty() {
        return String::new();
    }

    let suffix = token.get(token.len().saturating_sub(8)..).unwrap_or(token);
    let masked = format!("••••••••{suffix}");
    let tok_escaped = token.replace('\\', "\\\\").replace('"', "\\\"");

    format!(
        r#"<section class="card"><div class="card-head compact"><div><div class="eyebrow">访问令牌</div><h2>显示 Gateway Token</h2></div></div><div class="note-box"><strong>只在需要时显示</strong><p>这个令牌用于直接进入原生 OpenClaw Gateway。平时建议保持隐藏，只在调试或手动连接时查看。</p></div><div class="token-section"><div class="token-header"><span>Gateway Token</span><span>请勿分享给不受信任的设备</span></div><div class="token-row"><code class="token-val" id="ocTokenVal">{masked}</code><button class="btn" id="ocTokenToggleBtn" type="button" onclick="ocToggleToken()">显示</button><button class="btn" type="button" onclick="ocCopyToken(this)">复制</button></div></div><script>(function(){{var t="{tok_escaped}";window.ocToggleToken=function(){{var v=document.getElementById("ocTokenVal"),b=document.getElementById("ocTokenToggleBtn");if(!v||!b)return;if(b.dataset.vis==="1"){{v.textContent="••••••••"+t.slice(-8);b.textContent="显示";b.dataset.vis="";}}else{{v.textContent=t;b.textContent="隐藏";b.dataset.vis="1";}}}};window.ocCopyToken=function(btn){{var orig=btn.textContent;function done(){{btn.textContent="已复制 ✓";setTimeout(function(){{btn.textContent=orig;}},1500);}}function fallback(){{try{{var ta=document.createElement("textarea");ta.value=t;ta.style.cssText="position:fixed;opacity:0;top:0;left:0;width:1px;height:1px";document.body.appendChild(ta);ta.focus();ta.select();var ok=document.execCommand("copy");document.body.removeChild(ta);if(ok){{done();}}else{{alert("Token: "+t);}}}}catch(e){{alert("Token: "+t);}}}}if(navigator.clipboard){{navigator.clipboard.writeText(t).then(done,fallback);}}else{{fallback();}}}};}})()</script></section>"#,
        masked = masked,
        tok_escaped = tok_escaped
    )
}

fn home_content(config: &PageConfig, snapshot: &SystemSnapshot, health_ok: Option<bool>) -> String {
    let gateway_pid = pid_value("openclaw-gateway");
    let (health_text, health_sub, health_tone) = match health_ok {
        Some(true) => ("在线", "Gateway 已通过健康检查", "tone-good"),
        Some(false) => ("异常", "Gateway 当前未通过健康检查", "tone-danger"),
        None if gateway_pid != "-" => {
            ("待确认", "已检测到 Gateway 进程，等待健康结果", "tone-warn")
        }
        None => ("离线", "未检测到 Gateway 进程", "tone-danger"),
    };

    format!(
        r#"<div class="page-grid"><section class="card hero-card"><div class="card-head"><div><div class="eyebrow">Hermes 风格入口</div><h2>OpenClaw Gateway 单页控制台</h2><p class="hero-copy">通过 Home Assistant 侧边栏进入这里，只保留最常用的两件事：进入原生 Gateway，或者直接打开维护 Shell。</p></div><div class="header-actions">{open_gateway}{open_shell}</div></div><div class="note-box"><strong>使用方式</strong><p>“打开网关”会像 Hermes 一样弹出新窗口并跳入原生控制台；“维护 Shell”则直接进入完整 Web Shell。</p></div></section><section class="card"><div class="card-head compact"><div><div class="eyebrow">实时状态</div><h2>OpenClaw Gateway 状态</h2></div>{health}</div><div class="single-status-block">{gateway_badge}<div class="kv-list">{version_row}{uptime_row}{entry_row}</div></div></section>{token_section}<section class="card"><div class="card-head compact"><div><div class="eyebrow">授权提醒</div><h2>确认新设备授权</h2></div></div><div class="note-box"><strong>先看列表，再做确认</strong><p>这里直接执行官方 <code>openclaw devices</code> 命令，不经过旧的多页和 TUI 注入链路。新设备登录后，先读取待批准列表，再确认最新请求会更稳。</p></div><div class="action-row" style="margin-top:16px;"><button class="btn" type="button" onclick="ocListDevices('deviceListStatus','deviceListOutput')">列出待批准设备</button><button class="btn primary" type="button" onclick="ocApproveLatestDevice('deviceApproveStatus')">确认最新授权</button></div><span class="form-status" id="deviceListStatus">页面会直接执行官方 <code>openclaw devices list --json</code></span><pre class="command-output" id="deviceListOutput">点击“列出待批准设备”后，会在这里显示 pending 与 paired 设备快照。</pre><span class="form-status" id="deviceApproveStatus">按钮会在本机执行官方 <code>openclaw devices approve --latest</code></span></section></div>"#,
        open_gateway = primary_link_button(
            "打开网关",
            "ocGatewayLink",
            "return ocOpenGatewayLink(event, this)"
        ),
        open_shell = shell_window_button("维护 Shell"),
        health = summary_strip("Gateway", health_text, health_sub, health_tone),
        gateway_badge = service_badge("OpenClaw Gateway", &gateway_pid),
        version_row = kv_row("OpenClaw 版本", &config.openclaw_version),
        uptime_row = kv_row("运行时长", &snapshot.openclaw_uptime),
        entry_row = kv_row("入口", "HTTPS :18789 / Home Assistant 侧边栏打开"),
        token_section = gateway_token_section(&config.gateway_token)
    )
}

fn render_shell(config: &PageConfig, title: &str, subtitle: &str, content: &str) -> Html<String> {
    let gateway_url = js_string(&config.gateway_url);
    Html(format!(
        r##"<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>OpenClaw · {title}</title>
<style>
:root{{--bg:#f5f7fb;--panel:#ffffff;--line:#d8e2ef;--text:#1f2a37;--muted:#6b7788;--blue:#2563eb;--blue-deep:#1d4ed8;--shadow:0 10px 30px rgba(15,23,42,.06);--radius:18px}}
*{{box-sizing:border-box}}
body{{margin:0;min-height:100vh;background:var(--bg);color:var(--text);font:14px/1.65 "Segoe UI","PingFang SC","Microsoft YaHei",sans-serif}}
.shell{{max-width:900px;margin:0 auto;padding:20px 16px 32px}}
.topbar{{display:flex;align-items:flex-start;justify-content:space-between;gap:16px;margin-bottom:16px}}
.brand-title{{font-size:17px;font-weight:800;letter-spacing:-.02em}}
.brand-sub{{margin-top:2px;color:var(--muted);font-size:12px}}
.version-chip{{display:inline-flex;align-items:center;min-height:30px;padding:0 12px;border-radius:999px;border:1px solid var(--line);background:#fff;color:#536277;font-size:12px;font-weight:700}}
.hero{{padding:24px 22px;border-radius:22px;border:1px solid var(--line);background:var(--panel);box-shadow:var(--shadow);margin-bottom:16px}}
.eyebrow{{margin:0 0 8px;color:#5a6a7f;font-size:11px;font-weight:800;letter-spacing:.12em;text-transform:uppercase}}
h1{{margin:0 0 8px;font-size:28px;line-height:1.12;letter-spacing:-.03em}}
.subtitle,.hero-copy,.note-box p,.form-status{{margin:0;color:var(--muted)}}
.content,.page-grid{{display:grid;gap:16px}}
.card{{padding:22px 20px;border-radius:var(--radius);border:1px solid var(--line);background:var(--panel);box-shadow:var(--shadow)}}
.card-head{{display:flex;justify-content:space-between;align-items:flex-start;gap:16px;margin-bottom:16px}}
.card-head.compact{{margin-bottom:14px}}
.card h2{{margin:0;font-size:21px;line-height:1.15;letter-spacing:-.03em}}
.header-actions,.action-row{{display:flex;flex-wrap:wrap;gap:10px}}
.btn{{min-height:40px;display:inline-flex;align-items:center;justify-content:center;padding:0 16px;border-radius:999px;border:1px solid var(--line);background:#fff;color:var(--text);text-decoration:none;font-weight:700;cursor:pointer;transition:border-color .12s ease,box-shadow .12s ease}}
.btn:hover{{border-color:#bcc9da;box-shadow:0 8px 20px rgba(15,23,42,.08)}}
.btn.primary{{border-color:transparent;background:linear-gradient(135deg,var(--blue),var(--blue-deep));color:#fff;box-shadow:0 12px 28px rgba(37,99,235,.28)}}
.btn.secondary{{background:#f8fbff;color:#17324f}}
.summary-strip-card{{position:relative;padding:14px 16px 14px 18px;border:1px solid var(--line);border-radius:16px;background:#fafcff}}
.summary-strip-card::before{{content:"";position:absolute;left:0;top:12px;bottom:12px;width:3px;border-radius:999px;background:#94a3b8}}
.summary-strip-card.tone-good::before{{background:#22c55e}}
.summary-strip-card.tone-warn::before{{background:#f59e0b}}
.summary-strip-card.tone-danger::before{{background:#ef4444}}
.summary-strip-title{{color:#60748d;font-size:11px;font-weight:900;letter-spacing:.08em;text-transform:uppercase;margin-bottom:6px}}
.summary-strip-value{{font-size:20px;font-weight:900;letter-spacing:-.03em;line-height:1.1;margin-bottom:4px}}
.summary-strip-sub{{color:#6d829b;font-size:12px}}
.single-status-block{{display:grid;gap:14px}}
.service-badge{{border:1px solid var(--line);border-radius:16px;background:#fbfdff;padding:14px 16px}}
.service-badge-top{{display:flex;align-items:center;justify-content:space-between;gap:10px;margin-bottom:8px}}
.service-name{{display:flex;align-items:center;gap:8px;font-size:15px;font-weight:900}}
.svc-dot{{width:9px;height:9px;border-radius:999px;background:#22c55e;box-shadow:0 0 0 4px rgba(34,197,94,.12)}}
.service-badge.is-offline .svc-dot{{background:#f59e0b;box-shadow:0 0 0 4px rgba(245,158,11,.12)}}
.service-state{{color:#4c6a87;font-size:12px;font-weight:900}}
.service-meta{{color:#6f849d;font-size:13px}}
.kv-list{{display:grid;gap:10px}}
.kv-row{{display:flex;justify-content:space-between;gap:16px;padding:10px 0;border-bottom:1px solid rgba(219,228,240,.75)}}
.kv-row:last-child{{border-bottom:none;padding-bottom:0}}
.kv-label{{color:#6f849d;font-weight:700}}
.kv-value{{text-align:right;font-weight:800;word-break:break-word}}
.note-box{{padding:14px 16px;border-radius:16px;border:1px solid #dce6f5;background:#f7faff}}
.note-box strong{{display:block;margin-bottom:4px;font-size:13px}}
.token-section{{display:grid;gap:12px}}
.token-header{{display:flex;justify-content:space-between;gap:12px;color:#60748d;font-size:12px;font-weight:800}}
.token-row{{display:flex;flex-wrap:wrap;gap:10px;align-items:center}}
.token-val{{flex:1 1 280px;min-height:44px;display:flex;align-items:center;padding:0 14px;border-radius:16px;background:#edf3fb;color:#17324f;font:700 13px/1.4 ui-monospace,"Cascadia Code",Consolas,monospace;overflow:auto}}
.command-output{{margin:12px 0 0;min-height:180px;padding:14px 16px;border-radius:18px;background:#f8fbff;color:#17324f;border:1px solid #d8e2ef;font:13px/1.6 ui-monospace,"Cascadia Code",Consolas,monospace;white-space:pre-wrap;overflow:auto}}
code{{font-family:ui-monospace,"Cascadia Code",Consolas,monospace}}
@media (max-width:720px){{.shell{{padding:16px 12px 24px}}.topbar,.card-head,.token-header,.kv-row{{flex-direction:column;align-items:flex-start}}.version-chip{{align-self:flex-start}}.token-row,.header-actions,.action-row{{width:100%}}.header-actions .btn,.action-row .btn{{flex:1 1 100%}}.kv-value{{text-align:left}}}}
</style>
</head>
<body>
<div class="shell">
<header class="topbar">
  <div>
    <div class="brand-title">OpenClaw Add-on</div>
    <div class="brand-sub">Hermes 风格的单页薄壳入口</div>
  </div>
  <div class="version-chip">Add-on {addon_version}</div>
</header>
<section class="hero">
  <div class="eyebrow">Gateway Console</div>
  <h1>{title}</h1>
  <p class="subtitle">{subtitle}</p>
</section>
<main class="content">{content}</main>
</div>
<script>
const OC_GATEWAY_URL={gateway_url};
const OC_GATEWAY_PORT="{gateway_port}";
function ocGatewayHref(){{if(OC_GATEWAY_URL&&OC_GATEWAY_URL.trim())return OC_GATEWAY_URL;return "https://"+window.location.hostname+":"+OC_GATEWAY_PORT;}}
function openAddonWindow(url,name){{const win=window.open(url,name,"popup=yes,width=1440,height=920");if(!win)window.location.href=url;return false;}}
function syncGatewayLink(){{const link=document.getElementById("ocGatewayLink");if(link)link.href=ocGatewayHref();}}
function ocOpenGatewayLink(event,anchor){{if(event)event.preventDefault();const targetUrl=anchor&&anchor.href?anchor.href:ocGatewayHref();return openAddonWindow(targetUrl,"openclaw-gateway");}}
function ocOpenShellWindow(){{return openAddonWindow("./shell/","openclaw-shell");}}
async function ocPostJson(url,payload){{const resp=await fetch(url,{{method:"POST",headers:{{"Content-Type":"application/json"}},body:JSON.stringify(payload||{{}})}});const data=await resp.json().catch(()=>({{ok:false,message:"返回格式无效"}}));if(!resp.ok&&!data.ok)throw new Error(data.message||"请求失败");return data;}}
function ocSetFormStatus(id,message,ok){{const el=document.getElementById(id);if(!el)return;el.textContent=message;el.style.color=ok===false?"#b91c1c":(ok===true?"#065f46":"#66758a");}}
window.ocApproveLatestDevice=async function(statusId){{ocSetFormStatus(statusId,"正在执行授权…");try{{const data=await ocPostJson("./action/devices-approve-latest",{{}});ocSetFormStatus(statusId,data.message||"已完成",!!data.ok);}}catch(error){{ocSetFormStatus(statusId,"执行失败："+(error.message||error),false);}}}};
window.ocListDevices=async function(statusId,outputId){{ocSetFormStatus(statusId,"正在读取设备列表…");const output=document.getElementById(outputId);if(output)output.textContent="正在读取…";try{{const data=await ocPostJson("./action/devices-list",{{}});ocSetFormStatus(statusId,data.message||"已完成",!!data.ok);if(output)output.textContent=data.output||"没有返回设备数据";}}catch(error){{ocSetFormStatus(statusId,"读取失败："+(error.message||error),false);if(output)output.textContent="读取失败："+(error.message||error);}}}};
syncGatewayLink();
</script>
</body>
</html>"##,
        title = title,
        subtitle = subtitle,
        addon_version = html_attr_escape(&config.addon_version),
        content = content,
        gateway_url = gateway_url,
        gateway_port = DEFAULT_GATEWAY_PORT
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_page_config() -> PageConfig {
        PageConfig {
            addon_version: "2026.04.15.6".to_string(),
            gateway_url: String::new(),
            openclaw_version: "2026.4.14".to_string(),
            gateway_token: "tok_test_12345678".to_string(),
        }
    }

    #[test]
    fn render_shell_keeps_hermes_style_single_page_frame() {
        let config = sample_page_config();
        let Html(html) = render_shell(&config, "标题", "副标题", "<div>content</div>");

        assert!(html.contains("OpenClaw Add-on"));
        assert!(html.contains("ocOpenGatewayLink"));
        assert!(html.contains("ocOpenShellWindow"));
        assert!(html.contains("./action/devices-list"));
        assert!(!html.contains("配置中心"));
        assert!(!html.contains("命令行"));
        assert!(!html.contains("日志"));
    }

    #[test]
    fn home_page_keeps_only_single_page_controls() {
        let config = sample_page_config();
        let snapshot = SystemSnapshot {
            openclaw_uptime: "35 分".to_string(),
        };

        let html = home_content(&config, &snapshot, Some(true));
        assert!(html.contains("打开网关"));
        assert!(html.contains("维护 Shell"));
        assert!(html.contains("OpenClaw Gateway 状态"));
        assert!(html.contains("显示 Gateway Token"));
        assert!(html.contains("列出待批准设备"));
        assert!(html.contains("确认最新授权"));
        assert!(!html.contains("OpenClaw CLI"));
        assert!(!html.contains("资源遥测"));
        assert!(!html.contains("配置中心"));
    }
}
