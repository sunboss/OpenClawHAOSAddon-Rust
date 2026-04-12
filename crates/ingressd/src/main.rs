use axum::{
    Json,
    Router,
    body::{Body, Bytes, to_bytes},
    extract::{
        ConnectInfo, Request, State,
        ws::{Message as AxumWsMessage, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, HeaderName, HeaderValue, Response, StatusCode, header},
    response::{Html, IntoResponse},
    routing::{any, get},
};
use axum_server::tls_rustls::RustlsConfig;
use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use rustls::crypto::aws_lc_rs;
use serde::Serialize;
use std::{env, fs, net::SocketAddr, path::PathBuf};
use tokio::{net::TcpStream, time::{Duration, timeout}};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        Message as TungsteniteMessage, client::IntoClientRequest,
        handshake::client::Request as WsClientRequest,
    },
};

#[derive(Clone)]
struct AppState {
    client: Client,
    ui_base: String,
    gateway_http_base: String,
    gateway_ws_base: String,
}

#[derive(Debug, Serialize)]
struct ActionResponse {
    ok: bool,
    action: String,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    error: Option<String>,
}

struct GatewayProbe {
    ok: bool,
    status: StatusCode,
    stdout: String,
    stderr: String,
    error: Option<String>,
}

#[tokio::main]
async fn main() {
    let _ = aws_lc_rs::default_provider().install_default();

    let ingress_port = env::var("INGRESS_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(48099);
    let ui_port = env::var("UI_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(48101);
    let gateway_internal_port = env::var("GATEWAY_INTERNAL_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(18790);
    let https_port = env::var("HTTPS_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(18789);
    let enable_https = env::var("ENABLE_HTTPS_PROXY")
        .map(|value| value == "true")
        .unwrap_or(true);

    let state = AppState {
        client: Client::builder()
            .http2_adaptive_window(true)
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("build reqwest client"),
        ui_base: format!("http://127.0.0.1:{ui_port}"),
        gateway_http_base: format!("http://127.0.0.1:{gateway_internal_port}"),
        gateway_ws_base: format!("ws://127.0.0.1:{gateway_internal_port}"),
    };

    let ingress_app = build_ingress_router(state.clone());
    let ingress_addr = SocketAddr::from(([0, 0, 0, 0], ingress_port));
    let ingress_listener = tokio::net::TcpListener::bind(ingress_addr)
        .await
        .expect("bind ingress listener");
    println!("ingressd: HA ingress listening on http://{ingress_addr}");

    let ingress_server = tokio::spawn(async move {
        axum::serve(
            ingress_listener,
            ingress_app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .expect("serve ingress app");
    });

    if enable_https {
        let gateway_app = build_gateway_router(state);
        let https_addr = SocketAddr::from(([0, 0, 0, 0], https_port));
        let tls_config =
            RustlsConfig::from_pem_file("/config/certs/gateway.crt", "/config/certs/gateway.key")
                .await
                .expect("load rustls config");
        println!("ingressd: Gateway HTTPS listening on https://{https_addr}");

        let gateway_server = tokio::spawn(async move {
            axum_server::bind_rustls(https_addr, tls_config)
                .serve(gateway_app.into_make_service_with_connect_info::<SocketAddr>())
                .await
                .expect("serve gateway app");
        });

        let _ = tokio::join!(ingress_server, gateway_server);
    } else {
        let _ = ingress_server.await;
    }
}

fn build_ingress_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(proxy_health))
        .route("/healthz", get(proxy_health))
        .route("/readyz", get(proxy_health))
        .route("/token", get(token_file))
        .route("/openclaw-ca.crt", get(cert_file))
        .route("/cert/ca.crt", get(cert_file))
        .fallback(any(proxy_ui))
        .with_state(state)
}

fn build_gateway_router(state: AppState) -> Router {
    Router::new()
        .route("/openclaw-ca.crt", get(cert_file))
        .route("/cert/ca.crt", get(cert_file))
        .route("/healthz", get(proxy_health))
        .route("/readyz", get(proxy_health))
        .fallback(any(proxy_gateway))
        .with_state(state)
}

async fn proxy_health(State(_state): State<AppState>, request: Request) -> impl IntoResponse {
    let path = request.uri().path().to_string();
    match path.as_str() {
        "/health" => local_health().await.into_response(),
        "/healthz" => local_healthz().await.into_response(),
        "/readyz" => local_readyz().await.into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn proxy_ui(State(state): State<AppState>, request: Request) -> impl IntoResponse {
    let path = request.uri().path().to_string();
    let response = proxy_http_request(&state.client, &state.ui_base, request, false, None).await;
    if response.status() != StatusCode::BAD_GATEWAY {
        return response;
    }

    match path.as_str() {
        "/" | "/index.html" => fallback_ui_response(),
        "/partials/health" => Html(
            r#"<h2>Service Status</h2><p class="hint">UI backend is still warming up. Refresh in a few seconds.</p>"#
                .to_string(),
        )
        .into_response(),
        "/partials/diag" => Html(
            r#"<h2>Quick Diagnostics</h2><p class="hint">UI backend is temporarily unavailable, but ingress is alive.</p>"#
                .to_string(),
        )
        .into_response(),
        _ => response,
    }
}

async fn proxy_gateway(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    ws: Result<WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>,
    request: Request,
) -> impl IntoResponse {
    if let Ok(ws) = ws {
        let path = request.uri().path().to_string();
        let query = request.uri().query().map(|q| q.to_string());
        let headers = request.headers().clone();
        return ws
            .on_upgrade(move |socket| {
                proxy_gateway_ws(state, socket, path, query, headers, peer_addr)
            })
            .into_response();
    }
    let response = proxy_http_request(
        &state.client,
        &state.gateway_http_base,
        request,
        true,
        Some(peer_addr),
    )
    .await;
    if response.status() == StatusCode::BAD_GATEWAY {
        return fallback_gateway_response();
    }
    response
}

async fn proxy_gateway_ws(
    state: AppState,
    socket: WebSocket,
    path: String,
    query: Option<String>,
    headers: HeaderMap,
    peer_addr: SocketAddr,
) {
    let mut target = format!("{}{}", state.gateway_ws_base, path);
    if let Some(query) = query {
        target.push('?');
        target.push_str(&query);
    }

    let mut upstream_request: WsClientRequest = match target.clone().into_client_request() {
        Ok(request) => request,
        Err(_) => return,
    };

    for header in [
        "host",
        "origin",
        "cookie",
        "authorization",
        "user-agent",
        "sec-websocket-protocol",
        "sec-websocket-extensions",
    ] {
        if let Some(value) = headers.get(header) {
            upstream_request.headers_mut().insert(
                HeaderName::from_bytes(header.as_bytes()).expect("header name"),
                value.clone(),
            );
        }
    }
    if let Ok(value) = HeaderValue::from_str(&peer_addr.ip().to_string()) {
        upstream_request
            .headers_mut()
            .insert(HeaderName::from_static("x-forwarded-for"), value.clone());
        upstream_request
            .headers_mut()
            .insert(HeaderName::from_static("x-real-ip"), value);
    }
    if let Some(host) = headers.get("host") {
        upstream_request
            .headers_mut()
            .insert(HeaderName::from_static("x-forwarded-host"), host.clone());
        if let Some(port) = forwarded_port_from_host(host) {
            upstream_request
                .headers_mut()
                .insert(HeaderName::from_static("x-forwarded-port"), port);
        }
        if let Some(forwarded) = forwarded_header_value(Some(host), peer_addr, "https") {
            upstream_request
                .headers_mut()
                .insert(HeaderName::from_static("forwarded"), forwarded);
        }
    }
    upstream_request.headers_mut().insert(
        HeaderName::from_static("x-forwarded-proto"),
        HeaderValue::from_static("https"),
    );

    let Ok((upstream, _)) = connect_async(upstream_request).await else {
        return;
    };

    let (mut client_tx, mut client_rx) = socket.split();
    let (mut upstream_tx, mut upstream_rx) = upstream.split();

    let client_to_upstream = tokio::spawn(async move {
        while let Some(Ok(message)) = client_rx.next().await {
            let translated = match message {
                AxumWsMessage::Text(text) => TungsteniteMessage::Text(text.to_string().into()),
                AxumWsMessage::Binary(data) => TungsteniteMessage::Binary(data),
                AxumWsMessage::Ping(data) => TungsteniteMessage::Ping(data),
                AxumWsMessage::Pong(data) => TungsteniteMessage::Pong(data),
                AxumWsMessage::Close(frame) => {
                    let _ = upstream_tx
                        .send(TungsteniteMessage::Close(frame.map(|f| {
                            tokio_tungstenite::tungstenite::protocol::CloseFrame {
                                code: f.code.into(),
                                reason: f.reason.to_string().into(),
                            }
                        })))
                        .await;
                    break;
                }
            };
            if upstream_tx.send(translated).await.is_err() {
                break;
            }
        }
    });

    let upstream_to_client = tokio::spawn(async move {
        while let Some(Ok(message)) = upstream_rx.next().await {
            let translated = match message {
                TungsteniteMessage::Text(text) => AxumWsMessage::Text(text.to_string().into()),
                TungsteniteMessage::Binary(data) => AxumWsMessage::Binary(data),
                TungsteniteMessage::Ping(data) => AxumWsMessage::Ping(data),
                TungsteniteMessage::Pong(data) => AxumWsMessage::Pong(data),
                TungsteniteMessage::Close(frame) => {
                    let _ = client_tx
                        .send(AxumWsMessage::Close(frame.map(|f| {
                            axum::extract::ws::CloseFrame {
                                code: f.code.into(),
                                reason: f.reason.to_string().into(),
                            }
                        })))
                        .await;
                    break;
                }
                TungsteniteMessage::Frame(_) => continue,
            };
            if client_tx.send(translated).await.is_err() {
                break;
            }
        }
    });

    let _ = tokio::join!(client_to_upstream, upstream_to_client);
}

async fn token_file() -> impl IntoResponse {
    let path = public_share_dir().join("gateway.token");
    file_response(path, "text/plain").await
}

async fn cert_file() -> impl IntoResponse {
    let path = public_share_dir().join("openclaw-ca.crt");
    let mut response = file_response(path, "application/x-x509-ca-cert")
        .await
        .into_response();
    response.headers_mut().insert(
        HeaderName::from_static("content-disposition"),
        HeaderValue::from_static("attachment; filename=\"openclaw-ca.crt\""),
    );
    response
}

fn public_share_dir() -> PathBuf {
    env::var("PUBLIC_SHARE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/run/openclaw-rs/public"))
}

async fn file_response(path: PathBuf, content_type: &str) -> impl IntoResponse {
    match fs::read(path) {
        Ok(bytes) => ([(axum::http::header::CONTENT_TYPE, content_type)], bytes).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn proxy_http_request(
    client: &Client,
    base: &str,
    request: Request,
    preserve_host: bool,
    peer_addr: Option<SocketAddr>,
) -> Response<Body> {
    let (parts, body) = request.into_parts();
    let mut target = format!("{base}{}", parts.uri.path());
    if let Some(query) = parts.uri.query() {
        target.push('?');
        target.push_str(query);
    }

    let body = match to_bytes(body, usize::MAX).await {
        Ok(bytes) => bytes,
        Err(err) => {
            return simple_response(
                StatusCode::BAD_REQUEST,
                format!("failed to read request body: {err}"),
            );
        }
    };

    let mut builder = client.request(parts.method.clone(), &target);
    builder = copy_request_headers(builder, &parts.headers, preserve_host);
    if preserve_host {
        builder = builder.header("x-forwarded-proto", "https");
        if let Some(host) = parts.headers.get("host") {
            builder = builder.header("x-forwarded-host", host);
            if let Some(port) = forwarded_port_from_host(host) {
                builder = builder.header("x-forwarded-port", port);
            }
            if let Some(peer_addr) = peer_addr
                && let Some(forwarded) = forwarded_header_value(Some(host), peer_addr, "https")
            {
                builder = builder.header("forwarded", forwarded);
            }
        }
        if let Some(peer_addr) = peer_addr {
            builder = builder.header("x-forwarded-for", peer_addr.ip().to_string());
            builder = builder.header("x-real-ip", peer_addr.ip().to_string());
        }
    }

    let response = match builder.body(body).send().await {
        Ok(response) => response,
        Err(err) => {
            return simple_response(StatusCode::BAD_GATEWAY, format!("proxy failed: {err}"));
        }
    };

    let status = response.status();
    let headers = response.headers().clone();
    let bytes = match response.bytes().await {
        Ok(bytes) => bytes,
        Err(err) => {
            return simple_response(StatusCode::BAD_GATEWAY, format!("proxy body failed: {err}"));
        }
    };
    build_response(status, &headers, bytes)
}

fn copy_request_headers(
    mut builder: reqwest::RequestBuilder,
    headers: &HeaderMap,
    preserve_host: bool,
) -> reqwest::RequestBuilder {
    for (name, value) in headers {
        if should_skip_header(name, preserve_host) {
            continue;
        }
        builder = builder.header(name, value);
    }
    builder
}

fn build_response(status: reqwest::StatusCode, headers: &HeaderMap, body: Bytes) -> Response<Body> {
    let mut response = Response::builder().status(status);
    for (name, value) in headers {
        if should_skip_response_header(name) {
            continue;
        }
        response = response.header(name, value);
    }
    response.body(Body::from(body)).unwrap_or_else(|_| {
        Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from("response build failed"))
            .expect("fallback response")
    })
}

fn simple_response(status: StatusCode, message: String) -> Response<Body> {
    Response::builder()
        .status(status)
        .body(Body::from(message))
        .expect("simple response")
}

fn fallback_gateway_response() -> Response<Body> {
    Html(
        r#"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta http-equiv="refresh" content="8">
  <title>OpenClaw Gateway</title>
  <style>
    body {
      margin: 0;
      font-family: "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif;
      background: linear-gradient(180deg, #0d1b38 0%, #111f3d 100%);
      color: #dbe8ff;
      display: flex;
      align-items: center;
      justify-content: center;
      min-height: 100vh;
    }
    .card {
      max-width: 480px;
      width: 90%;
      border: 1px solid rgba(255,255,255,.1);
      border-radius: 20px;
      background: rgba(255,255,255,.05);
      padding: 32px;
      text-align: center;
    }
    h1 { margin: 0 0 12px; font-size: 22px; color: #60cbff; }
    p { margin: 0 0 20px; color: #8aacd4; line-height: 1.7; font-size: 14px; }
    .btn {
      display: inline-block;
      padding: 10px 22px;
      border-radius: 999px;
      border: 1px solid rgba(255,255,255,.2);
      background: rgba(255,255,255,.08);
      color: #dbe8ff;
      text-decoration: none;
      font-size: 13px;
      font-weight: 700;
      cursor: pointer;
    }
  </style>
</head>
<body>
  <div class="card">
    <h1>OpenClaw Gateway</h1>
    <p>Gateway 正在启动，通常需要 30–60 秒。<br>页面将自动刷新。</p>
    <button class="btn" onclick="location.reload()">立即刷新</button>
  </div>
</body>
</html>"#
        .to_string(),
    )
    .into_response()
}

fn fallback_ui_response() -> Response<Body> {
    Html(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>OpenClawHAOSAddon-Rust</title>
  <style>
    body {
      margin: 0;
      font-family: "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif;
      background: linear-gradient(180deg, #eef4ff 0%, #f8fbff 100%);
      color: #17314d;
    }
    .wrap {
      max-width: 840px;
      margin: 0 auto;
      padding: 40px 20px;
    }
    .card {
      border: 1px solid #d7e4f4;
      border-radius: 22px;
      background: rgba(255,255,255,.96);
      padding: 24px;
      box-shadow: 0 10px 28px rgba(23, 52, 86, .08);
    }
    h1 {
      margin: 0 0 10px;
      font-size: 30px;
    }
    p {
      line-height: 1.7;
      color: #58718b;
    }
    .actions {
      display: flex;
      flex-wrap: wrap;
      gap: 12px;
      margin-top: 18px;
    }
    .btn {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      min-height: 44px;
      padding: 10px 16px;
      border-radius: 999px;
      border: 1px solid #b8cef0;
      background: #edf5ff;
      color: #17314d;
      text-decoration: none;
      font-weight: 700;
      cursor: pointer;
    }
  </style>
</head>
<body>
  <div class="wrap">
    <div class="card">
      <h1>OpenClawHAOSAddon-Rust</h1>
      <p>
        Ingress is responding, but the Rust UI backend is still starting or restarting.
        This fallback avoids a blank 502 screen while the UI catches up.
      </p>
      <div class="actions">
        <button class="btn" type="button" onclick="location.reload()">Reload</button>
        <a class="btn" href="./openclaw-ca.crt" target="_blank" rel="noopener noreferrer">Download CA Cert</a>
      </div>
    </div>
  </div>
</body>
</html>"#
            .to_string(),
    )
    .into_response()
}

fn forwarded_port_from_host(host: &HeaderValue) -> Option<HeaderValue> {
    let host = host.to_str().ok()?;
    let port = host.rsplit_once(':')?.1;
    HeaderValue::from_str(port).ok()
}

fn forwarded_header_value(
    host: Option<&HeaderValue>,
    peer_addr: SocketAddr,
    proto: &str,
) -> Option<HeaderValue> {
    let host = host
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let mut forwarded = format!("for={};proto={proto}", peer_addr.ip());
    if !host.is_empty() {
        forwarded.push_str(";host=");
        forwarded.push_str(host);
    }
    HeaderValue::from_str(&forwarded).ok()
}

fn should_skip_header(name: &HeaderName, preserve_host: bool) -> bool {
    let lower = name.as_str().to_ascii_lowercase();
    if preserve_host {
        matches!(
            lower.as_str(),
            "content-length" | "connection" | "upgrade" | "transfer-encoding"
        )
    } else {
        matches!(
            lower.as_str(),
            "host" | "content-length" | "connection" | "upgrade" | "transfer-encoding"
        )
    }
}

fn should_skip_response_header(name: &HeaderName) -> bool {
    matches!(
        name.as_str().to_ascii_lowercase().as_str(),
        "content-length" | "connection" | "transfer-encoding"
    )
}

async fn local_health() -> (StatusCode, Json<ActionResponse>) {
    let probe = local_gateway_probe().await;
    (
        probe.status,
        Json(ActionResponse {
            ok: probe.ok,
            action: "health".to_string(),
            exit_code: Some(if probe.ok { 0 } else { 1 }),
            stdout: probe.stdout,
            stderr: probe.stderr,
            error: probe.error,
        }),
    )
}

async fn local_healthz() -> (StatusCode, [(HeaderName, &'static str); 1], &'static str) {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        "ok\n",
    )
}

async fn local_readyz() -> (StatusCode, [(HeaderName, &'static str); 1], String) {
    probe_text_response(local_gateway_probe().await)
}

fn probe_text_response(probe: GatewayProbe) -> (StatusCode, [(HeaderName, &'static str); 1], String) {
    let body = if probe.ok {
        format!("ok: {}\n", probe.stdout)
    } else if !probe.stderr.is_empty() {
        format!("not ready: {}\n", probe.stderr)
    } else {
        format!(
            "not ready: {}\n",
            probe.error.unwrap_or_else(|| "unknown".to_string())
        )
    };
    (
        probe.status,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        body,
    )
}

async fn local_gateway_probe() -> GatewayProbe {
    let port = configured_gateway_port();
    let gateway_mode = env::var("GATEWAY_MODE").unwrap_or_else(|_| "local".to_string());

    let Some((process_name, pid)) = current_gateway_process() else {
        return GatewayProbe {
            ok: false,
            status: StatusCode::SERVICE_UNAVAILABLE,
            stdout: String::new(),
            stderr: "no managed gateway pid file present".to_string(),
            error: Some("missing_gateway_pid".to_string()),
        };
    };

    if !gateway_process_requires_local_port(&gateway_mode, process_name) {
        return GatewayProbe {
            ok: true,
            status: StatusCode::OK,
            stdout: format!("{process_name} pid {pid} running in {gateway_mode} mode"),
            stderr: String::new(),
            error: None,
        };
    }

    let target = format!("127.0.0.1:{port}");
    let port_ready = timeout(Duration::from_millis(800), TcpStream::connect(&target))
        .await
        .map(|result| result.is_ok())
        .unwrap_or(false);

    if port_ready {
        return GatewayProbe {
            ok: true,
            status: StatusCode::OK,
            stdout: format!("{process_name} pid {pid} listening on {target}"),
            stderr: String::new(),
            error: None,
        };
    }

    GatewayProbe {
        ok: false,
        status: StatusCode::SERVICE_UNAVAILABLE,
        stdout: format!("{process_name} pid {pid} present"),
        stderr: format!("gateway port {target} is not accepting connections yet"),
        error: Some("gateway_port_not_ready".to_string()),
    }
}

fn configured_gateway_port() -> u16 {
    env::var("GATEWAY_INTERNAL_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(18790)
}

fn current_gateway_process() -> Option<(&'static str, String)> {
    select_gateway_process(
        non_empty_trimmed_file("/run/openclaw-rs/openclaw-gateway.pid"),
        non_empty_trimmed_file("/run/openclaw-rs/openclaw-node.pid"),
    )
}

fn select_gateway_process(
    gateway_pid: Option<String>,
    node_pid: Option<String>,
) -> Option<(&'static str, String)> {
    gateway_pid
        .map(|pid| ("openclaw-gateway", pid))
        .or_else(|| node_pid.map(|pid| ("openclaw-node", pid)))
}

fn gateway_process_requires_local_port(gateway_mode: &str, process_name: &str) -> bool {
    !(gateway_mode == "remote" && process_name == "openclaw-node")
}

fn non_empty_trimmed_file(path: &str) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwarded_helpers_keep_host_and_port() {
        let host = HeaderValue::from_static("192.168.1.122:18789");
        let peer_addr: SocketAddr = "192.168.1.142:51234".parse().expect("socket addr");

        let port = forwarded_port_from_host(&host).expect("forwarded port");
        let forwarded =
            forwarded_header_value(Some(&host), peer_addr, "https").expect("forwarded header");

        assert_eq!(port.to_str().expect("port str"), "18789");
        assert_eq!(
            forwarded.to_str().expect("forwarded str"),
            "for=192.168.1.142;proto=https;host=192.168.1.122:18789"
        );
    }
}
