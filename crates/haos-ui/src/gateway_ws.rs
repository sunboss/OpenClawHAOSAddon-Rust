/// 通过 WebSocket webchat 协议与 openclaw gateway 通信。
///
/// 协议流程（来自逆向分析 openclaw 2026.4.8 dist/method-scopes-*.js）：
///   1. 建立 ws://127.0.0.1:18790 连接
///   2. 服务端推送 { type:"event", event:"connect.challenge", payload:{nonce:"..."} }
///   3. 客户端发送 connect 请求（携带 token + client 信息）
///   4. 服务端返回 hello-ok 响应
///   5. 此后可发送任意 { type:"req", id, method, params } 请求
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::{connect_async, tungstenite::Message};

const CONNECT_TIMEOUT_SECS: u64 = 12;
const REQUEST_TIMEOUT_SECS: u64 = 10;

#[derive(Debug)]
#[derive(Clone)]
pub struct PendingPair {
    pub request_id: String,
    pub device_name: String,
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
            // gateway 尚未就绪时会出现 "Connection refused"，属于正常启动阶段现象，不打错误日志
            if err.contains("Connection refused") {
                // gateway 未启动，静默跳过
            } else {
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
    // 使用 CLI 身份连接（不带 Origin 头），避免触发 Control UI 的设备身份校验。
    // gateway 安全逻辑：
    //   - id="openclaw-control-ui" → isControlUi=true → 必须有 device identity → 被拒
    //   - id="cli" + mode="cli" → isControlUi=false → roleCanSkipDeviceIdentity("operator", true) → allow
    //   - 带 Origin 头会使 hasBrowserOriginHeader=true，影响 CLI 本地等价判断，故不加 Origin
    let (mut ws, _) = connect_async("ws://127.0.0.1:18790")
        .await
        .map_err(|e| format!("ws connect failed: {e}"))?;

    // 等待 connect.challenge 事件，拿到 nonce
    let _nonce = tokio::time::timeout(
        std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS),
        wait_for_challenge(&mut ws),
    )
    .await
    .map_err(|_| "connect.challenge timeout".to_string())??;

    // 发送 connect 请求
    // 使用 id="cli" + mode="cli"：gateway 把我们识别为 CLI 客户端而非 Control UI，
    // 从而跳过 device identity 校验（roleCanSkipDeviceIdentity("operator", true) → allow）
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
                "platform": "linux",
                "instanceId": new_id()
            },
            "caps": [],
            "auth": { "token": token },
            "role": "operator",
            "scopes": ["operator.pairing"]
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

fn new_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    // 简单 UUID-like ID，不引入 uuid crate
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    format!("{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}", t, t >> 8, t & 0xfff, t ^ 0xabcd, t as u64 * 0x123456789)
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
