use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    extract::{
        Path, Request, State,
        ws::{Message as AxumWsMessage, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, HeaderName, HeaderValue, Response, StatusCode},
    response::{Html, IntoResponse, Redirect},
    routing::{any, get},
};
use axum_server::tls_rustls::RustlsConfig;
use futures_util::{SinkExt, StreamExt};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use reqwest::Client;
use rustls::crypto::aws_lc_rs;
use std::{
    env, fs,
    io::{Read, Write},
    net::SocketAddr,
    sync::{Arc, Mutex},
    thread,
};
use tokio::sync::mpsc;
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
    action_base: String,
    gateway_http_base: String,
    gateway_ws_base: String,
    enable_terminal: bool,
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
    let action_port = env::var("ACTION_SERVER_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(48100);
    let gateway_internal_port = env::var("GATEWAY_INTERNAL_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(18790);
    let https_port = env::var("HTTPS_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(18789);
    let enable_terminal = env::var("ENABLE_TERMINAL")
        .map(|value| value == "true")
        .unwrap_or(true);
    let enable_https = env::var("ENABLE_HTTPS_PROXY")
        .map(|value| value == "true")
        .unwrap_or(true);

    let state = AppState {
        client: Client::builder()
            .http2_adaptive_window(true)
            .build()
            .expect("build reqwest client"),
        ui_base: format!("http://127.0.0.1:{ui_port}"),
        action_base: format!("http://127.0.0.1:{action_port}"),
        gateway_http_base: format!("http://127.0.0.1:{gateway_internal_port}"),
        gateway_ws_base: format!("ws://127.0.0.1:{gateway_internal_port}"),
        enable_terminal,
    };

    let ingress_app = build_ingress_router(state.clone());
    let ingress_addr = SocketAddr::from(([127, 0, 0, 1], ingress_port));
    let ingress_listener = tokio::net::TcpListener::bind(ingress_addr)
        .await
        .expect("bind ingress listener");
    println!("ingressd: HA ingress listening on http://{ingress_addr}");

    let ingress_server = tokio::spawn(async move {
        axum::serve(ingress_listener, ingress_app)
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
                .serve(gateway_app.into_make_service())
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
        .route("/terminal", get(terminal_redirect))
        .route("/terminal/", get(terminal_page))
        .route("/terminal/ws", get(terminal_ws))
        .route("/health", get(proxy_health))
        .route("/action/{action}", any(proxy_action))
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
        .fallback(any(proxy_gateway))
        .with_state(state)
}

async fn terminal_redirect() -> impl IntoResponse {
    Redirect::temporary("/terminal/")
}

async fn terminal_page(State(state): State<AppState>) -> impl IntoResponse {
    if !state.enable_terminal {
        return Html(
            r#"<!doctype html><meta charset="utf-8"><body style="font-family:Segoe UI,Microsoft YaHei,sans-serif;padding:24px">终端未启用。</body>"#
                .to_string(),
        );
    }

    Html(
        r##"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>OpenClaw Terminal</title>
  <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/xterm@5.5.0/css/xterm.min.css">
  <style>
    html, body {{ margin: 0; height: 100%; background: #0f172a; }}
    #terminal {{ height: 100vh; width: 100vw; padding: 10px; box-sizing: border-box; }}
  </style>
</head>
<body>
  <div id="terminal"></div>
  <script src="https://cdn.jsdelivr.net/npm/xterm@5.5.0/lib/xterm.min.js"></script>
  <script>
    const term = new Terminal({{
      cursorBlink: true,
      fontFamily: "Consolas, 'SFMono-Regular', monospace",
      theme: {{
        background: "#0f172a",
        foreground: "#dbe8ff"
      }}
    }});
    term.open(document.getElementById("terminal"));

    const scheme = location.protocol === "https:" ? "wss" : "ws";
    const socket = new WebSocket(`${{scheme}}://${{location.host}}/terminal/ws`);
    socket.binaryType = "arraybuffer";

    socket.addEventListener("message", (event) => {{
      if (typeof event.data === "string") {{
        term.write(event.data);
        return;
      }}
      const decoded = new TextDecoder().decode(new Uint8Array(event.data));
      term.write(decoded);
    }});

    socket.addEventListener("close", () => {{
      term.write("\r\n[terminal closed]\r\n");
    }});

    term.onData((data) => {{
      if (socket.readyState === WebSocket.OPEN) {{
        socket.send(data);
      }}
    }});

    window.injectCommand = function (command) {{
      if (!command) return;
      if (socket.readyState === WebSocket.OPEN) {{
        socket.send(command + "\n");
      }}
    }};
  </script>
</body>
</html>"##
            .to_string(),
    )
}

async fn terminal_ws(State(state): State<AppState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    if !state.enable_terminal {
        return StatusCode::NOT_FOUND.into_response();
    }
    ws.on_upgrade(handle_terminal_socket).into_response()
}

async fn handle_terminal_socket(socket: WebSocket) {
    let pty_system = native_pty_system();
    let Ok(pair) = pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }) else {
        return;
    };

    let shell = env::var("SHELL").unwrap_or_else(|_| "bash".to_string());
    let cmd = CommandBuilder::new(shell);
    let Ok(mut child) = pair.slave.spawn_command(cmd) else {
        return;
    };
    drop(pair.slave);

    let Ok(mut reader) = pair.master.try_clone_reader() else {
        let _ = child.kill();
        return;
    };
    let Ok(writer) = pair.master.take_writer() else {
        let _ = child.kill();
        return;
    };
    let writer = Arc::new(Mutex::new(writer));

    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();

    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let send_task = tokio::spawn(async move {
        while let Some(chunk) = rx.recv().await {
            if sender
                .send(AxumWsMessage::Binary(chunk.into()))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    let write_task = {
        let writer = writer.clone();
        tokio::spawn(async move {
            while let Some(Ok(message)) = receiver.next().await {
                match message {
                    AxumWsMessage::Text(text) => {
                        if let Ok(mut handle) = writer.lock() {
                            let _ = handle.write_all(text.as_bytes());
                            let _ = handle.flush();
                        }
                    }
                    AxumWsMessage::Binary(data) => {
                        if let Ok(mut handle) = writer.lock() {
                            let _ = handle.write_all(&data);
                            let _ = handle.flush();
                        }
                    }
                    AxumWsMessage::Close(_) => break,
                    _ => {}
                }
            }
        })
    };

    let _ = tokio::join!(send_task, write_task);
    let _ = child.kill();
}

async fn proxy_health(State(state): State<AppState>, request: Request) -> impl IntoResponse {
    proxy_http_request(&state.client, &state.action_base, request, false).await
}

async fn proxy_action(
    State(state): State<AppState>,
    Path(action): Path<String>,
    request: Request,
) -> impl IntoResponse {
    let _ = action;
    proxy_http_request(&state.client, &state.action_base, request, false).await
}

async fn proxy_ui(State(state): State<AppState>, request: Request) -> impl IntoResponse {
    proxy_http_request(&state.client, &state.ui_base, request, false).await
}

async fn proxy_gateway(
    State(state): State<AppState>,
    ws: Result<WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>,
    request: Request,
) -> impl IntoResponse {
    if let Ok(ws) = ws {
        let path = request.uri().path().to_string();
        let query = request.uri().query().map(|q| q.to_string());
        let headers = request.headers().clone();
        return ws
            .on_upgrade(move |socket| proxy_gateway_ws(state, socket, path, query, headers))
            .into_response();
    }
    proxy_http_request(&state.client, &state.gateway_http_base, request, true).await
}

async fn proxy_gateway_ws(
    state: AppState,
    socket: WebSocket,
    path: String,
    query: Option<String>,
    headers: HeaderMap,
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
        "origin",
        "cookie",
        "authorization",
        "user-agent",
        "sec-websocket-protocol",
    ] {
        if let Some(value) = headers.get(header) {
            upstream_request.headers_mut().insert(
                HeaderName::from_bytes(header.as_bytes()).expect("header name"),
                value.clone(),
            );
        }
    }

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
    file_response("/etc/nginx/html/gateway.token", "text/plain").await
}

async fn cert_file() -> impl IntoResponse {
    let mut response = file_response(
        "/etc/nginx/html/openclaw-ca.crt",
        "application/x-x509-ca-cert",
    )
    .await
    .into_response();
    response.headers_mut().insert(
        HeaderName::from_static("content-disposition"),
        HeaderValue::from_static("attachment; filename=\"openclaw-ca.crt\""),
    );
    response
}

async fn file_response(path: &str, content_type: &str) -> impl IntoResponse {
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
