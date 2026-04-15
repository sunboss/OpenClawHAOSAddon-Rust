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
    agent_model: String,
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
            agent_model: runtime_config
                .as_ref()
                .and_then(|value| {
                    string_path(value, "gateway.agent.model")
                        .or_else(|| string_path(value, "agent.model"))
                        .or_else(|| string_path(value, "model"))
                })
                .unwrap_or_else(|| "未配置".to_string()),
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
    let (health_text, health_sub, tone, health_label) = match health_ok {
        Some(true) => (
            "已就绪",
            "OpenClaw Gateway 已通过健康检查，可直接进入控制台。",
            "good",
            "实时状态",
        ),
        Some(false) => (
            "异常",
            "Gateway 当前未通过健康检查，建议先检查日志与设备授权。",
            "danger",
            "实时状态",
        ),
        None if gateway_pid != "-" => (
            "等待确认",
            "已检测到 Gateway 进程，正在等待健康结果回传。",
            "warn",
            "实时状态",
        ),
        None => (
            "离线",
            "当前未检测到 Gateway 进程，入口按钮将继续保留。",
            "danger",
            "实时状态",
        ),
    };
    let (model_primary, model_secondary) =
        if config.agent_model.is_empty() || config.agent_model == "未配置" {
            (
                "未配置".to_string(),
                "请在 OpenClaw 配置中设置模型".to_string(),
            )
        } else if let Some((provider, model)) = config.agent_model.rsplit_once('/') {
            (model.to_string(), provider.to_string())
        } else {
            (config.agent_model.clone(), "当前模型标识".to_string())
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
  --bg:#08101a;
  --bg2:#0d1626;
  --panel:#0c1525;
  --panel-strong:#101b2e;
  --panel-soft:rgba(12,21,37,.78);
  --line:rgba(118,153,180,.18);
  --line-strong:rgba(66,207,227,.26);
  --text:#edf5fb;
  --muted:#9aacc2;
  --muted-soft:#73859b;
  --cyan:#46d3ec;
  --teal:#1f6f77;
  --mint:#90e9cf;
  --yellow:#f0c65a;
  --good:#8af0c7;
  --warn:#e0be61;
  --danger:#f1838c;
}}
* {{ box-sizing:border-box; }}
body {{
  margin:0; color:var(--text);
  font:14px/1.65 "MiSans","HarmonyOS Sans SC","Noto Sans SC","Segoe UI","PingFang SC",sans-serif;
  background:
    radial-gradient(circle at 13% 0%, rgba(31,111,119,.34), transparent 30%),
    radial-gradient(circle at 82% 12%, rgba(70,211,236,.14), transparent 22%),
    radial-gradient(circle at 70% 68%, rgba(31,111,119,.13), transparent 26%),
    linear-gradient(180deg, var(--bg2) 0%, var(--bg) 42%, #070d18 100%);
  min-height:100vh;
}}
body::before {{
  content:""; position:fixed; inset:0; pointer-events:none; opacity:.14;
  background-image:
    linear-gradient(rgba(255,255,255,.03) 1px, transparent 1px),
    linear-gradient(90deg, rgba(255,255,255,.03) 1px, transparent 1px);
  background-size:32px 32px;
}}
.shell {{ width:min(1180px, calc(100% - 32px)); margin:0 auto; padding:28px 0 34px; }}
.eyebrow {{
  color:var(--cyan);
  font-size:12px;
  font-weight:800;
  letter-spacing:.18em;
  text-transform:uppercase;
}}
.copy,.meta,.hint,.micro-copy,.hero-note {{ color:var(--muted); }}
h1,h2,h3,p {{ margin:0; }}
.hero {{
  position:relative;
  overflow:hidden;
  padding:34px 34px 30px;
  border:1px solid var(--line);
  border-radius:34px;
  background:
    linear-gradient(135deg, rgba(25,39,62,.96) 0%, rgba(11,20,35,.98) 52%, rgba(9,15,28,.98) 100%);
  box-shadow:0 34px 90px rgba(0,0,0,.36);
}}
.hero::before {{
  content:"";
  position:absolute;
  inset:-10% 42% 24% -14%;
  border-radius:999px;
  background:radial-gradient(circle, rgba(31,111,119,.34), rgba(31,111,119,0));
  filter:blur(18px);
}}
.hero::after {{
  content:"";
  position:absolute;
  inset:0;
  pointer-events:none;
  background:
    linear-gradient(90deg, rgba(70,211,236,.08), transparent 24%, transparent 76%, rgba(70,211,236,.06)),
    linear-gradient(180deg, rgba(255,255,255,.02), transparent 18%, transparent 82%, rgba(255,255,255,.03));
}}
.hero-grid {{
  position:relative;
  z-index:1;
  display:grid;
  grid-template-columns:minmax(0, 1.28fr) minmax(300px, .72fr);
  gap:22px;
  align-items:end;
}}
.hero-main {{ max-width:680px; }}
.brand-lockup {{
  display:flex;
  align-items:center;
  gap:18px;
  margin-bottom:26px;
}}
.mark {{
  position:relative;
  width:84px;
  height:84px;
  display:grid;
  place-items:center;
  border-radius:26px;
  border:1px solid rgba(255,255,255,.08);
  background:linear-gradient(145deg, rgba(70,211,236,.92), rgba(240,198,90,.38));
  box-shadow:inset 0 1px 0 rgba(255,255,255,.14), 0 18px 48px rgba(0,0,0,.18);
}}
.mark::before {{
  content:"";
  position:absolute;
  inset:9px;
  border-radius:20px;
  background:linear-gradient(180deg, rgba(9,18,30,.16), rgba(9,18,30,.36));
}}
.mark .o {{
  position:relative;
  z-index:1;
  font-size:42px;
  font-weight:900;
  color:#05101c;
}}
.mark .trace {{
  position:absolute;
  z-index:1;
  width:54px;
  height:3px;
  bottom:13px;
  border-radius:999px;
  background:rgba(7,16,28,.62);
}}
.brand-meta {{ display:grid; gap:8px; }}
.brand-rule {{
  width:min(240px, 42vw);
  height:2px;
  border-radius:999px;
  background:linear-gradient(90deg, var(--cyan), rgba(70,211,236,0));
}}
h1 {{
  max-width:10ch;
  font-size:clamp(42px, 5.4vw, 64px);
  line-height:.98;
  letter-spacing:-.05em;
  text-wrap:balance;
}}
.lede {{
  margin-top:18px;
  max-width:40ch;
  font-size:18px;
  line-height:1.72;
  color:#d7e3ef;
}}
.hero-flags {{
  display:flex;
  flex-wrap:wrap;
  gap:10px;
  margin-top:22px;
}}
.flag {{
  display:inline-flex;
  align-items:center;
  min-height:34px;
  padding:0 14px;
  border-radius:999px;
  border:1px solid rgba(70,211,236,.2);
  background:rgba(255,255,255,.02);
  color:#d7edf4;
  font-size:13px;
}}
.flag::before {{
  content:"";
  width:8px;
  height:8px;
  margin-right:10px;
  border-radius:999px;
  background:var(--cyan);
  box-shadow:0 0 0 6px rgba(70,211,236,.08);
}}
.hero-side {{
  position:relative;
  z-index:1;
  padding:22px 22px 20px;
  border:1px solid rgba(70,211,236,.14);
  border-radius:26px;
  background:linear-gradient(180deg, rgba(17,28,46,.94), rgba(11,20,35,.94));
  box-shadow:inset 0 1px 0 rgba(255,255,255,.04);
}}
.hero-side-grid {{
  display:grid;
  grid-template-columns:repeat(2,minmax(0,1fr));
  gap:14px 18px;
  margin-top:18px;
}}
.hero-side-grid span {{
  display:block;
  margin-bottom:6px;
  color:var(--muted-soft);
  font-size:12px;
  letter-spacing:.08em;
  text-transform:uppercase;
}}
.hero-side-grid strong {{
  display:block;
  font-size:22px;
  line-height:1.1;
  letter-spacing:-.03em;
}}
.hero-side .hero-note {{
  margin-top:16px;
  font-size:14px;
}}
.metrics {{
  display:grid;
  grid-template-columns:repeat(3,minmax(0,1fr));
  gap:18px;
  margin-top:20px;
}}
.metric-card,
.action-card,
.ops-strip,
.notice-strip {{
  border:1px solid var(--line);
  border-radius:28px;
  background:var(--panel-soft);
  box-shadow:0 24px 70px rgba(0,0,0,.28);
}}
.metric-card {{
  min-height:220px;
  padding:24px 24px 22px;
  display:flex;
  flex-direction:column;
  justify-content:space-between;
}}
.metric-card.model {{
  background:
    linear-gradient(180deg, rgba(20,38,50,.94), rgba(11,18,31,.96)),
    radial-gradient(circle at 15% 10%, rgba(70,211,236,.11), transparent 36%);
  border-color:var(--line-strong);
}}
.metric-card.status {{
  background:linear-gradient(180deg, rgba(12,19,34,.96), rgba(8,14,26,.98));
}}
.metric-card.access {{
  background:
    linear-gradient(180deg, rgba(12,21,38,.96), rgba(8,14,26,.98)),
    radial-gradient(circle at 76% 18%, rgba(240,198,90,.08), transparent 24%);
}}
.metric-label {{
  color:var(--muted);
  font-size:13px;
  letter-spacing:.12em;
  text-transform:uppercase;
}}
.metric-value {{
  margin-top:12px;
  font-size:clamp(30px, 3vw, 42px);
  font-weight:850;
  line-height:1.02;
  letter-spacing:-.05em;
}}
.metric-sub {{
  margin-top:12px;
  font-size:15px;
  color:var(--muted);
}}
.status-good .metric-value {{ color:var(--good); }}
.status-warn .metric-value {{ color:var(--warn); }}
.status-danger .metric-value {{ color:var(--danger); }}
.support-strip {{
  display:grid;
  grid-template-columns:repeat(3,minmax(0,1fr));
  gap:14px;
  margin-top:14px;
}}
.support-card {{
  padding:16px 18px;
  border:1px solid var(--line);
  border-radius:20px;
  background:rgba(10,17,29,.72);
}}
.support-card strong {{
  display:block;
  margin-top:6px;
  font-size:22px;
  line-height:1.05;
  letter-spacing:-.03em;
}}
.action-deck {{
  display:grid;
  grid-template-columns:minmax(0,1.08fr) minmax(0,.92fr);
  gap:20px;
  margin-top:22px;
}}
.action-card {{
  position:relative;
  overflow:hidden;
  padding:28px 28px 24px;
}}
.action-card::before {{
  content:"";
  position:absolute;
  inset:auto auto -10% -8%;
  width:42%;
  aspect-ratio:1/1;
  border-radius:999px;
  background:radial-gradient(circle, rgba(70,211,236,.12), transparent 70%);
}}
.action-card .glyph {{
  width:84px;
  height:84px;
  display:grid;
  place-items:center;
  border-radius:24px;
  border:1px solid rgba(70,211,236,.24);
  background:linear-gradient(180deg, rgba(30,82,91,.46), rgba(15,33,47,.3));
  color:var(--cyan);
  font-size:38px;
  box-shadow:inset 0 1px 0 rgba(255,255,255,.06);
}}
.action-card h2 {{
  margin-top:22px;
  font-size:clamp(30px, 3.4vw, 44px);
  line-height:1.02;
  letter-spacing:-.05em;
}}
.action-card p {{
  margin-top:14px;
  max-width:34ch;
  font-size:18px;
  color:#cfdae7;
}}
.action-buttons {{
  display:flex;
  flex-wrap:wrap;
  gap:12px;
  margin-top:26px;
}}
.btn {{
  border:0;
  text-decoration:none;
  cursor:pointer;
  display:inline-flex;
  align-items:center;
  justify-content:center;
  min-height:54px;
  padding:0 22px;
  border-radius:999px;
  font-weight:800;
  font-size:16px;
  transition:transform .2s ease, background .2s ease, border-color .2s ease;
}}
.btn:hover {{ transform:translateY(-1px); }}
.btn-primary {{
  background:linear-gradient(180deg, #49daf0, #1fb6d2);
  color:#07101a;
  box-shadow:0 16px 30px rgba(34,199,234,.16);
}}
.btn-secondary {{
  background:rgba(255,255,255,.02);
  color:var(--text);
  border:1px solid rgba(70,211,236,.3);
}}
.ops-strip {{
  display:grid;
  grid-template-columns:minmax(280px,.92fr) minmax(0,1.08fr);
  gap:18px;
  margin-top:22px;
  padding:22px;
}}
.ops-block {{
  padding:18px 18px 16px;
  border:1px solid rgba(70,211,236,.12);
  border-radius:22px;
  background:rgba(9,16,29,.58);
}}
.ops-title {{
  font-size:12px;
  font-weight:800;
  letter-spacing:.14em;
  text-transform:uppercase;
  color:var(--cyan);
}}
.token {{
  margin-top:14px;
  padding:14px 16px;
  border-radius:16px;
  border:1px solid rgba(70,211,236,.18);
  background:rgba(7,13,24,.72);
  color:#dff2f7;
  font:14px/1.5 ui-monospace,Consolas,monospace;
  overflow:auto;
}}
.inline-actions {{
  display:flex;
  flex-wrap:wrap;
  gap:10px;
  margin-top:14px;
}}
.inline-actions .btn {{
  min-height:44px;
  padding:0 16px;
  font-size:14px;
}}
.status-hint {{
  margin-top:14px;
  color:var(--muted);
  font-size:14px;
}}
pre {{
  margin:16px 0 0;
  padding:16px 18px;
  border-radius:20px;
  border:1px solid rgba(70,211,236,.16);
  background:rgba(7,13,24,.78);
  color:#d7edf4;
  font:13px/1.68 ui-monospace,Consolas,monospace;
  white-space:pre-wrap;
  overflow:auto;
}}
.notice-strip {{
  display:flex;
  justify-content:space-between;
  gap:14px;
  align-items:center;
  margin-top:18px;
  padding:18px 22px;
  background:linear-gradient(180deg, rgba(9,16,28,.82), rgba(7,13,24,.92));
}}
.notice-strip strong {{
  display:block;
  font-size:14px;
  letter-spacing:.08em;
  text-transform:uppercase;
}}
.notice-strip span {{
  color:var(--muted);
  font-size:15px;
}}
@media (max-width: 720px) {{
  .shell {{ width:min(100% - 20px, 1180px); padding:14px 0 24px; }}
  .hero {{ padding:24px 18px 20px; border-radius:28px; }}
  .hero-grid,
  .metrics,
  .support-strip,
  .action-deck,
  .ops-strip {{
    grid-template-columns:1fr;
  }}
  .brand-lockup {{ align-items:flex-start; }}
  .mark {{ width:72px; height:72px; border-radius:22px; }}
  .mark .o {{ font-size:36px; }}
  .hero-side,
  .metric-card,
  .action-card,
  .ops-block {{
    padding-left:18px;
    padding-right:18px;
  }}
  .hero-side-grid {{ grid-template-columns:1fr 1fr; }}
  .action-card h2 {{ font-size:34px; }}
  .action-card p,
  .lede {{ font-size:16px; }}
  .action-buttons .btn,
  .inline-actions .btn {{
    flex:1 1 100%;
  }}
  .notice-strip {{ flex-direction:column; align-items:flex-start; }}
}}
</style>
</head>
<body>
<div class="shell">
  <section class="hero">
    <div class="hero-grid">
      <div class="hero-main">
        <div class="brand-lockup">
          <div class="mark"><span class="o">O</span><span class="trace"></span></div>
          <div class="brand-meta">
            <div class="eyebrow">Home Assistant Ingress</div>
            <div class="brand-rule"></div>
          </div>
        </div>
        <h1>OpenClaw 主控台</h1>
        <p class="lede">这不是聊天窗口，而是一张持续值守的 Agent 入口面板。默认优先直连原生 HTTPS Gateway，同时保留一条 HAOS Ingress 测试链路，方便我们并排验证。</p>
        <div class="hero-flags">
          <span class="flag">原生 HTTPS Gateway</span>
          <span class="flag">HAOS Ingress Test</span>
          <span class="flag">维护 Shell</span>
        </div>
      </div>
      <aside class="hero-side">
        <div class="eyebrow">运行快照</div>
        <div class="hero-side-grid">
          <div>
            <span>Add-on</span>
            <strong>{addon_version}</strong>
          </div>
          <div>
            <span>Runtime</span>
            <strong>{openclaw_version}</strong>
          </div>
          <div>
            <span>Gateway PID</span>
            <strong>{gateway_pid}</strong>
          </div>
          <div>
            <span>Uptime</span>
            <strong>{openclaw_uptime}</strong>
          </div>
        </div>
        <p class="hero-note">外部默认入口是 <code>https://主机:{gateway_port}</code>。HAOS 测试入口会沿着当前 Ingress 路径走 <code>./gateway/</code>，只用于确认侧边栏链路。</p>
      </aside>
    </div>
  </section>

  <section class="metrics">
    <article class="metric-card model">
      <div>
        <div class="metric-label">当前模型</div>
        <div class="metric-value">{model_primary}</div>
        <div class="metric-sub">{model_secondary}</div>
      </div>
      <div class="micro-copy">模型信息直接从运行配置读取，不再手写展示值。</div>
    </article>
    <article class="metric-card status status-{tone}">
      <div>
        <div class="metric-label">{health_label}</div>
        <div class="metric-value">{health_text}</div>
        <div class="metric-sub">{health_sub}</div>
      </div>
      <div class="micro-copy">OpenClaw Gateway {gateway_state}，当前进程 PID {gateway_pid}。</div>
    </article>
    <article class="metric-card access">
      <div>
        <div class="metric-label">访问方式</div>
        <div class="metric-value">双路径</div>
        <div class="metric-sub">原生直连与 HAOS Ingress 测试入口同时保留。</div>
      </div>
      <div class="micro-copy">主入口使用外部 HTTPS，测试入口使用 <code>./gateway/</code>。</div>
    </article>
  </section>

  <section class="support-strip">
    <article class="support-card">
      <div class="metric-label">OpenClaw Runtime</div>
      <strong>{openclaw_version}</strong>
    </article>
    <article class="support-card">
      <div class="metric-label">Gateway 访问</div>
      <strong>https://主机:{gateway_port}</strong>
    </article>
    <article class="support-card">
      <div class="metric-label">当前运行时长</div>
      <strong>{openclaw_uptime}</strong>
    </article>
  </section>

  <section class="action-deck">
    <article class="action-card">
      <div class="glyph">⌁</div>
      <div class="eyebrow" style="margin-top:18px">官方 Web 控制面板</div>
      <h2>打开 Gateway</h2>
      <p>把主入口做得更直接。主按钮直连原生 HTTPS Gateway，副按钮保留 HAOS Ingress 测试链路，方便我们快速对比哪条路径最稳。</p>
      <div class="action-buttons">
        <a class="btn btn-primary" id="ocGatewayLink" href="#" target="_blank" rel="noopener noreferrer" onclick="return ocOpenGatewayLink(event,this)">打开网关</a>
        <button class="btn btn-secondary" type="button" onclick="ocOpenIngressGatewayWindow()">HAOS 网关（测试）</button>
      </div>
    </article>
    <article class="action-card">
      <div class="glyph">&gt;_</div>
      <div class="eyebrow" style="margin-top:18px">原生命令行</div>
      <h2>维护 Shell</h2>
      <p>这里直接进入完整的 Web Shell。查看日志、运行 <code>openclaw</code> 命令、核对设备授权，都应该从这里一键进入，不再绕路。</p>
      <div class="action-buttons">
        <button class="btn btn-primary" type="button" onclick="ocOpenShellWindow()">进入命令行</button>
      </div>
    </article>
  </section>

  <section class="ops-strip">
    <div class="ops-block">
      <div class="ops-title">显示令牌</div>
      <div class="token" id="ocTokenVal">{token_masked}</div>
      <div class="inline-actions">
        <button class="btn btn-secondary" id="ocTokenToggleBtn" type="button" onclick="ocToggleToken()">显示</button>
        <button class="btn btn-secondary" type="button" onclick="ocCopyToken(this)">复制</button>
      </div>
      <div class="status-hint">原生 Gateway 与 HAOS 测试入口都会复用这枚令牌。</div>
    </div>
    <div class="ops-block">
      <div class="ops-title">授权提醒与确认</div>
      <div class="hint" style="margin-top:12px">新设备登录后，先看待批准列表，再确认最新请求。这里直接调用官方 <code>openclaw devices</code> 命令，不再经过旧终端注入路径。</div>
      <div class="inline-actions">
        <button class="btn btn-secondary" type="button" onclick="ocListDevices('deviceListStatus','deviceListOutput')">列出待批准设备</button>
        <button class="btn btn-primary" type="button" onclick="ocApproveLatestDevice('deviceApproveStatus')">确认最新授权</button>
      </div>
      <div class="status-hint" id="deviceListStatus">页面会直接执行官方 <code>openclaw devices list --json</code></div>
      <div class="status-hint" id="deviceApproveStatus">按钮会在本机执行官方 <code>openclaw devices approve --latest</code></div>
      <pre id="deviceListOutput">点击“列出待批准设备”后，这里会显示 pending 与 paired 设备快照。</pre>
    </div>
  </section>

  <section class="notice-strip">
    <div>
      <strong>入口说明</strong>
      <span>如果原生 HTTPS 入口能直接打开，就优先使用它；HAOS 测试入口只用于验证侧边栏路径。</span>
    </div>
    <div>
      <strong>运行边界</strong>
      <span>这个页面只做入口、状态、令牌和授权，不再承担完整控制台职责。</span>
    </div>
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
        health_label = health_label,
        health_text = health_text,
        health_sub = health_sub,
        model_primary = html_escape(&model_primary),
        model_secondary = html_escape(&model_secondary),
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
            addon_version: "2026.04.15.10".to_string(),
            gateway_url: String::new(),
            openclaw_version: "2026.4.14".to_string(),
            gateway_token: "tok_test_12345678".to_string(),
            agent_model: "openai-codex/gpt-5.4".to_string(),
        }
    }

    #[test]
    fn render_shell_keeps_single_page_controls() {
        let config = sample_page_config();
        let snapshot = SystemSnapshot {
            openclaw_uptime: "35 分钟".to_string(),
        };
        let Html(html) = render_shell(&config, &snapshot, Some(true));
        assert!(html.contains("OpenClaw 主控台"));
        assert!(html.contains("HAOS 网关（测试）"));
        assert!(html.contains("ocOpenIngressGatewayWindow"));
        assert!(html.contains("#token="));
        assert!(html.contains("当前模型"));
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
