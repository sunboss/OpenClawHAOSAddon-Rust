/// 通过 WebSocket webchat 协议与 openclaw gateway 通信。
///
/// 协议流程（逆向自 openclaw 2026.4.9 dist/）：
///   1. 建立 ws://127.0.0.1:18790 连接（id="cli" + mode="cli"，不带 Origin 头）
///   2. 服务端推送 { type:"event", event:"connect.challenge", payload:{nonce:"..."} }
///   3. 客户端用本地 Ed25519 私钥对 payload 签名，发送 connect 请求
///   4. gateway 判断为 cli_container_local → silent auto-approve（无需手动配对）
///   5. 服务端返回 hello-ok，scopes 包含 operator.pairing
///   6. 此后可发送 device.pair.list / device.pair.approve 等请求
///
/// 关于 device identity：
///   - 首次运行时在 /config/.openclaw/haos-ui-identity.json 生成 Ed25519 密钥对
///   - deviceId = SHA256(publicKeyRaw) hex 字符串
///   - 签名 payload 格式（v3）：
///     "v3|deviceId|cli|cli|operator|scopes|signedAtMs|token|nonce|platform|deviceFamily"
///   - 第一次连接时 gateway 会 silent auto-approve（因为 cli_container_local locality）
///   - 此后复用同一 identity 文件，已配对设备直接通过
use futures_util::{SinkExt, StreamExt};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use tokio_tungstenite::{connect_async, tungstenite::Message};

const CONNECT_TIMEOUT_SECS: u64 = 30;
const REQUEST_TIMEOUT_SECS: u64 = 25;

const IDENTITY_PATH: &str = "/config/.openclaw/haos-ui-identity.json";

#[derive(Debug)]
#[derive(Clone)]
pub struct PendingPair {
    pub request_id: String,
    pub device_name: String,
}

/// 本地设备身份（持久化到磁盘）
struct DeviceIdentity {
    device_id: String,
    public_key_b64url: String,
    key_pair: Ed25519KeyPair,
}

/// 加载或创建设备身份文件
fn load_or_create_identity() -> Result<DeviceIdentity, String> {
    if Path::new(IDENTITY_PATH).exists() {
        let raw = fs::read_to_string(IDENTITY_PATH)
            .map_err(|e| format!("read identity file: {e}"))?;
        let v: Value = serde_json::from_str(&raw)
            .map_err(|e| format!("parse identity file: {e}"))?;
        let pkcs8_b64 = v["pkcs8"].as_str().ok_or("missing pkcs8 field")?;
        let pkcs8 = base64url_decode(pkcs8_b64)?;
        let key_pair = Ed25519KeyPair::from_pkcs8(&pkcs8)
            .map_err(|e| format!("load key pair: {e}"))?;
        let pub_raw = key_pair.public_key().as_ref();
        let device_id = hex_sha256(pub_raw);
        let public_key_b64url = base64url_encode(pub_raw);
        return Ok(DeviceIdentity { device_id, public_key_b64url, key_pair });
    }

    // 生成新密钥对
    let rng = ring::rand::SystemRandom::new();
    let pkcs8_doc = Ed25519KeyPair::generate_pkcs8(&rng)
        .map_err(|e| format!("generate key pair: {e}"))?;
    let pkcs8_bytes = pkcs8_doc.as_ref();
    let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8_bytes)
        .map_err(|e| format!("load new key pair: {e}"))?;
    let pub_raw = key_pair.public_key().as_ref();
    let device_id = hex_sha256(pub_raw);
    let public_key_b64url = base64url_encode(pub_raw);
    let pkcs8_b64 = base64url_encode(pkcs8_bytes);

    // 确保目录存在
    if let Some(parent) = Path::new(IDENTITY_PATH).parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create identity dir: {e}"))?;
    }
    let stored = json!({
        "version": 1,
        "deviceId": &device_id,
        "pkcs8": &pkcs8_b64,
        "createdAtMs": now_ms()
    });
    fs::write(IDENTITY_PATH, format!("{}\n", serde_json::to_string_pretty(&stored).unwrap()))
        .map_err(|e| format!("write identity file: {e}"))?;
    println!("haos-ui: 生成新设备身份 deviceId={}", &device_id[..16]);

    Ok(DeviceIdentity { device_id, public_key_b64url, key_pair })
}

/// 查询待配对设备列表。失败时返回空列表（不打 panic）。
pub async fn list_pending_pairs(gateway_token: &str) -> Vec<PendingPair> {
    match call_gateway(gateway_token, "device.pair.list", json!({})).await {
        Ok(payload) => {
            let pairs = parse_pending_pairs(&payload);
            if !pairs.is_empty() {
                println!("haos-ui: device.pair.list: {} 个待配对请求", pairs.len());
            }
            pairs
        }
        Err(err) => {
            // 启动阶段两类正常现象，不打错误日志：
            //   - "Connection refused"：gateway 进程尚未启动
            //   - "timed out"：gateway 已启动但 acpx 运行时尚未就绪（通常需要 60-120s）
            let is_startup_noise = err.contains("Connection refused") || err.contains("timed out");
            if !is_startup_noise {
                eprintln!("haos-ui: device.pair.list failed: {err}");
            }
            vec![]
        }
    }
}

/// 批准指定 requestId 的配对请求。返回 (ok, 消息)。
pub async fn approve_pair(gateway_token: &str, request_id: &str) -> (bool, String) {
    println!("haos-ui: device.pair.approve request_id={request_id}");
    match call_gateway(
        gateway_token,
        "device.pair.approve",
        json!({ "requestId": request_id }),
    )
    .await
    {
        Ok(_) => {
            println!("haos-ui: device.pair.approve ok request_id={request_id}");
            (true, "已批准".to_string())
        }
        Err(err) => {
            eprintln!("haos-ui: device.pair.approve failed request_id={request_id}: {err}");
            (false, format!("批准失败：{err}"))
        }
    }
}

/// 通用 gateway WebSocket 请求。
async fn call_gateway(token: &str, method: &str, params: Value) -> Result<Value, String> {
    tokio::time::timeout(
        std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS + REQUEST_TIMEOUT_SECS),
        call_gateway_inner(token, method, params),
    )
    .await
    .map_err(|_| format!("gateway call timed out ({method})"))?
}

async fn call_gateway_inner(token: &str, method: &str, params: Value) -> Result<Value, String> {
    let identity = load_or_create_identity()?;

    let (mut ws, _) = connect_async("ws://127.0.0.1:18790")
        .await
        .map_err(|e| format!("ws connect failed: {e}"))?;

    // 等待 connect.challenge，拿到 nonce
    let nonce = tokio::time::timeout(
        std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS),
        wait_for_challenge(&mut ws),
    )
    .await
    .map_err(|_| "connect.challenge timeout".to_string())??;

    // 构建签名 payload（v3 格式）
    let scopes_str = "operator.pairing";
    let signed_at_ms = now_ms();
    let platform = "linux";
    let device_family = "";
    let payload_str = format!(
        "v3|{}|cli|cli|operator|{}|{}|{}|{}|{}|{}",
        identity.device_id,
        scopes_str,
        signed_at_ms,
        token,
        nonce,
        platform,
        device_family
    );
    let signature = {
        let sig = identity.key_pair.sign(payload_str.as_bytes());
        base64url_encode(sig.as_ref())
    };

    // 发送 connect 请求（携带 device identity + 签名）
    let connect_id = new_id();
    let connect_frame = json!({
        "type": "req",
        "id": connect_id,
        "method": "connect",
        "params": {
            "minProtocol": 3,
            "maxProtocol": 3,
            "client": {
                "id": "cli",
                "version": "2026.4.9",
                "mode": "cli",
                "platform": platform,
                "instanceId": new_id()
            },
            "caps": [],
            "auth": { "token": token },
            "role": "operator",
            "scopes": ["operator.pairing"],
            "device": {
                "id": identity.device_id,
                "publicKey": identity.public_key_b64url,
                "signature": signature,
                "signedAt": signed_at_ms,
                "nonce": nonce
            }
        }
    });
    send_json(&mut ws, &connect_frame).await?;

    // 等待 hello-ok
    tokio::time::timeout(
        std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS),
        wait_for_response(&mut ws, &connect_id),
    )
    .await
    .map_err(|_| "hello-ok timeout".to_string())??;

    // 发送实际请求
    let req_id = new_id();
    let req_frame = json!({
        "type": "req",
        "id": req_id,
        "method": method,
        "params": params
    });
    send_json(&mut ws, &req_frame).await?;

    // 等待响应
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS),
        wait_for_response(&mut ws, &req_id),
    )
    .await
    .map_err(|_| format!("request timeout ({method})"))?;

    let _ = ws.close(None).await;
    result
}

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn wait_for_challenge(ws: &mut WsStream) -> Result<String, String> {
    while let Some(msg) = ws.next().await {
        let text = match msg.map_err(|e| format!("ws read error: {e}"))? {
            Message::Text(t) => t,
            Message::Close(_) => return Err("ws closed before challenge".to_string()),
            _ => continue,
        };
        let Ok(frame) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if frame.get("type").and_then(|v| v.as_str()) == Some("event")
            && frame.get("event").and_then(|v| v.as_str()) == Some("connect.challenge")
        {
            let nonce = frame
                .pointer("/payload/nonce")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if nonce.is_empty() {
                return Err("connect.challenge missing nonce".to_string());
            }
            return Ok(nonce);
        }
    }
    Err("ws stream ended before connect.challenge".to_string())
}

async fn wait_for_response(ws: &mut WsStream, id: &str) -> Result<Value, String> {
    while let Some(msg) = ws.next().await {
        let text = match msg.map_err(|e| format!("ws read error: {e}"))? {
            Message::Text(t) => t,
            Message::Close(_) => return Err("ws closed before response".to_string()),
            _ => continue,
        };
        let Ok(frame) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if frame.get("type").and_then(|v| v.as_str()) != Some("res") {
            continue;
        }
        if frame.get("id").and_then(|v| v.as_str()) != Some(id) {
            continue;
        }
        return if frame.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
            Ok(frame.get("payload").cloned().unwrap_or(Value::Null))
        } else {
            let msg = frame
                .pointer("/error/message")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error")
                .to_string();
            Err(msg)
        };
    }
    Err("ws stream ended before response".to_string())
}

async fn send_json(ws: &mut WsStream, value: &Value) -> Result<(), String> {
    let text = serde_json::to_string(value).map_err(|e| format!("serialize error: {e}"))?;
    ws.send(Message::Text(text.into()))
        .await
        .map_err(|e| format!("ws send error: {e}"))
}

// ─── 工具函数 ──────────────────────────────────────────────────────────────────

fn base64url_encode(input: &[u8]) -> String {
    let b64 = {
        let mut s = String::new();
        // 使用标准库手写 base64（不引入额外 crate）
        const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut i = 0;
        while i + 2 < input.len() {
            let n = ((input[i] as u32) << 16) | ((input[i+1] as u32) << 8) | (input[i+2] as u32);
            s.push(CHARS[((n >> 18) & 63) as usize] as char);
            s.push(CHARS[((n >> 12) & 63) as usize] as char);
            s.push(CHARS[((n >> 6) & 63) as usize] as char);
            s.push(CHARS[(n & 63) as usize] as char);
            i += 3;
        }
        if i + 1 == input.len() {
            let n = (input[i] as u32) << 16;
            s.push(CHARS[((n >> 18) & 63) as usize] as char);
            s.push(CHARS[((n >> 12) & 63) as usize] as char);
        } else if i + 2 == input.len() {
            let n = ((input[i] as u32) << 16) | ((input[i+1] as u32) << 8);
            s.push(CHARS[((n >> 18) & 63) as usize] as char);
            s.push(CHARS[((n >> 12) & 63) as usize] as char);
            s.push(CHARS[((n >> 6) & 63) as usize] as char);
        }
        s
    };
    b64.replace('+', "-").replace('/', "_").trim_end_matches('=').to_string()
}

fn base64url_decode(input: &str) -> Result<Vec<u8>, String> {
    let normalized = input.replace('-', "+").replace('_', "/");
    let padded = match normalized.len() % 4 {
        2 => format!("{}==", normalized),
        3 => format!("{}=", normalized),
        _ => normalized,
    };
    // 手写 base64 解码
    const TABLE: [i8; 256] = {
        let mut t = [-1i8; 256];
        let chars = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut i = 0usize;
        while i < 64 {
            t[chars[i] as usize] = i as i8;
            i += 1;
        }
        t['=' as usize] = 0;
        t
    };
    let bytes = padded.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    let mut i = 0;
    while i + 3 < bytes.len() {
        let a = TABLE[bytes[i] as usize];
        let b = TABLE[bytes[i+1] as usize];
        let c = TABLE[bytes[i+2] as usize];
        let d = TABLE[bytes[i+3] as usize];
        if a < 0 || b < 0 || c < 0 || d < 0 {
            return Err("invalid base64url input".to_string());
        }
        let n = ((a as u32) << 18) | ((b as u32) << 12) | ((c as u32) << 6) | (d as u32);
        out.push((n >> 16) as u8);
        if bytes[i+2] != b'=' { out.push((n >> 8) as u8); }
        if bytes[i+3] != b'=' { out.push(n as u8); }
        i += 4;
    }
    Ok(out)
}

fn hex_sha256(data: &[u8]) -> String {
    use ring::digest;
    let hash = digest::digest(&digest::SHA256, data);
    hash.as_ref().iter().fold(String::new(), |mut acc, b| {
        acc.push_str(&format!("{b:02x}"));
        acc
    })
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn new_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let ms = t.as_millis() as u64;
    let ns = t.subsec_nanos();
    format!("{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        ns, (ns >> 8) & 0xffff, ns & 0xfff, (ns ^ 0xabcd) & 0xffff, ms * 0x123456789)
}

fn parse_pending_pairs(payload: &Value) -> Vec<PendingPair> {
    let Some(pending) = payload.get("pending").and_then(|v| v.as_array()) else {
        return vec![];
    };
    pending
        .iter()
        .filter_map(|item| {
            let request_id = item.get("requestId")?.as_str()?.to_string();
            let device_name = item
                .get("deviceName")
                .or_else(|| item.get("name"))
                .or_else(|| item.get("label"))
                .and_then(|v| v.as_str())
                .unwrap_or("未知设备")
                .to_string();
            Some(PendingPair { request_id, device_name })
        })
        .collect()
}
