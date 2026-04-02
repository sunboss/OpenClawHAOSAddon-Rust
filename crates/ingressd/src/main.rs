use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    extract::{
        Path, Request, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, HeaderName, HeaderValue, Response, StatusCode},
    response::{Html, IntoResponse, Redirect},
    routing::{any, get},
};
use futures_util::{SinkExt, StreamExt};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use reqwest::Client;
use std::{
    env, fs,
    io::{Read, Write},
    net::SocketAddr,
    sync::{Arc, Mutex},
    thread,
};
use tokio::sync::mpsc;

#[derive(Clone)]
struct AppState {
    client: Client,
    ui_base: String,
    action_base: String,
    enable_terminal: bool,
}

#[tokio::main]
async fn main() {
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
    let enable_terminal = env::var("ENABLE_TERMINAL")
        .map(|value| value == "true")
        .unwrap_or(true);

    let state = AppState {
        client: Client::builder()
            .http2_adaptive_window(true)
            .build()
            .expect("build reqwest client"),
        ui_base: format!("http://127.0.0.1:{ui_port}"),
        action_base: format!("http://127.0.0.1:{action_port}"),
        enable_terminal,
    };

    let app = Router::new()
        .route("/terminal", get(terminal_redirect))
        .route("/terminal/", get(terminal_page))
        .route("/terminal/ws", get(terminal_ws))
        .route("/health", get(proxy_health))
        .route("/action/{action}", any(proxy_action))
        .route("/token", get(token_file))
        .route("/openclaw-ca.crt", get(cert_file))
        .route("/cert/ca.crt", get(cert_file))
        .fallback(any(proxy_ui))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], ingress_port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind ingress listener");
    println!("ingressd: listening on http://{addr}");
    axum::serve(listener, app).await.expect("serve ingressd");
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
    html, body { margin: 0; height: 100%; background: #0f172a; }
    #terminal { height: 100vh; width: 100vw; padding: 10px; box-sizing: border-box; }
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
            if sender.send(Message::Binary(chunk.into())).await.is_err() {
                break;
            }
        }
    });

    let write_task = {
        let writer = writer.clone();
        tokio::spawn(async move {
            while let Some(Ok(message)) = receiver.next().await {
                match message {
                    Message::Text(text) => {
                        if let Ok(mut handle) = writer.lock() {
                            let _ = handle.write_all(text.as_bytes());
                            let _ = handle.flush();
                        }
                    }
                    Message::Binary(data) => {
                        if let Ok(mut handle) = writer.lock() {
                            let _ = handle.write_all(&data);
                            let _ = handle.flush();
                        }
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
        })
    };

    let _ = tokio::join!(send_task, write_task);
    let _ = child.kill();
}

async fn proxy_health(State(state): State<AppState>, request: Request) -> impl IntoResponse {
    proxy_request(&state.client, &state.action_base, request).await
}

async fn proxy_action(
    State(state): State<AppState>,
    Path(action): Path<String>,
    mut request: Request,
) -> impl IntoResponse {
    let suffix = format!("/action/{action}");
    request.extensions_mut().insert(suffix);
    proxy_request(&state.client, &state.action_base, request).await
}

async fn proxy_ui(State(state): State<AppState>, request: Request) -> impl IntoResponse {
    proxy_request(&state.client, &state.ui_base, request).await
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

async fn proxy_request(client: &Client, base: &str, request: Request) -> Response<Body> {
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
    builder = copy_request_headers(builder, &parts.headers);
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
) -> reqwest::RequestBuilder {
    for (name, value) in headers {
        if should_skip_header(name) {
            continue;
        }
        builder = builder.header(name, value);
    }
    builder
}

fn build_response(status: reqwest::StatusCode, headers: &HeaderMap, body: Bytes) -> Response<Body> {
    let mut response = Response::builder().status(status);
    for (name, value) in headers {
        if should_skip_header(name) {
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

fn should_skip_header(name: &HeaderName) -> bool {
    matches!(
        name.as_str().to_ascii_lowercase().as_str(),
        "host" | "content-length" | "connection" | "upgrade" | "transfer-encoding"
    )
}
