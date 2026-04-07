use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    extract::{
        ConnectInfo, Path, Query, Request, State,
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
use serde::Deserialize;
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

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum TerminalClientMessage {
    Input { data: String },
    Resize { cols: u16, rows: u16 },
}

#[derive(Default, Deserialize)]
struct TerminalPageQuery {
    command: Option<String>,
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
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("build reqwest client"),
        ui_base: format!("http://127.0.0.1:{ui_port}"),
        action_base: format!("http://127.0.0.1:{action_port}"),
        gateway_http_base: format!("http://127.0.0.1:{gateway_internal_port}"),
        gateway_ws_base: format!("ws://127.0.0.1:{gateway_internal_port}"),
        enable_terminal,
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
        .route("/terminal", get(terminal_redirect))
        .route("/terminal/", get(terminal_page))
        .route("/terminal/ws", any(terminal_ws))
        .route("/terminal/assets/xterm.js", get(terminal_xterm_js))
        .route("/terminal/assets/xterm.css", get(terminal_xterm_css))
        .route(
            "/terminal/assets/addon-fit.js",
            get(terminal_xterm_addon_fit_js),
        )
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

fn openclaw_brand_svg(class_name: &str) -> String {
    format!(
        r##"<svg class="{class_name}" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg" role="img" aria-label="OpenClaw logo" preserveAspectRatio="xMidYMid meet">
  <rect x="8" y="8" width="80" height="80" rx="24" fill="#10284c"/>
  <rect x="14" y="14" width="68" height="68" rx="20" fill="#14355f"/>
  <path d="M30 34 19 27 18 41 29 42Z" fill="#60cbff"/>
  <path d="M66 34 77 27 78 41 67 42Z" fill="#60cbff"/>
  <path d="M31 49c0-10 6-18 17-21" stroke="#8be0ff" stroke-width="6" stroke-linecap="round"/>
  <path d="M65 49c0-10-6-18-17-21" stroke="#8be0ff" stroke-width="6" stroke-linecap="round"/>
  <path d="M34 61c7 8 21 8 28 0" stroke="#8be0ff" stroke-width="6" stroke-linecap="round"/>
  <circle cx="48" cy="49" r="10" fill="#eef7ff"/>
  <circle cx="44" cy="46" r="2.6" fill="#14355f"/>
  <circle cx="52" cy="46" r="2.6" fill="#14355f"/>
</svg>"##
    )
}

async fn terminal_page(
    Query(query): Query<TerminalPageQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if !state.enable_terminal {
        return Html(
            r#"<!doctype html><meta charset="utf-8"><body style="font-family:Segoe UI,Microsoft YaHei,sans-serif;padding:24px">Terminal is disabled.</body>"#
                .to_string(),
        );
    }

    let boot_command = serde_json::to_string(&query.command.unwrap_or_default())
        .unwrap_or_else(|_| "\"\"".to_string());

    Html(
        r##"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>OpenClaw Terminal</title>
  <link rel="stylesheet" href="./assets/xterm.css">
  <style>
    :root {
      --bg: #0f172a;
      --bg2: #111c33;
      --line: #223252;
      --text: #dbe8ff;
      --muted: #8ea5c8;
      --accent: #2563eb;
    }
    html, body {
      margin: 0;
      height: 100%;
      background: var(--bg);
      color: var(--text);
      font-family: Consolas, "SFMono-Regular", "Microsoft YaHei", monospace;
    }
    .shell {
      display: grid;
      grid-template-rows: auto 1fr auto;
      height: 100vh;
    }
    .head {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 16px;
      padding: 10px 14px;
      border-bottom: 1px solid var(--line);
      background: rgba(255,255,255,.02);
      font-family: "Segoe UI", "Microsoft YaHei", sans-serif;
    }
    .brand-row {
      display: flex;
      align-items: center;
      gap: 12px;
      min-width: 0;
    }
    .brand-badge {
      width: 46px;
      height: 46px;
      flex: 0 0 auto;
      display: grid;
      place-items: center;
      border-radius: 14px;
      background: linear-gradient(180deg, #163a65 0%, #10284c 100%);
      box-shadow: 0 8px 18px rgba(7, 20, 40, .28);
    }
    .brand-mark {
      display: block;
      width: 34px;
      height: 34px;
    }
    .brand-copy {
      min-width: 0;
      display: grid;
      gap: 2px;
    }
    .brand-title {
      display: block;
      color: #f2f7ff;
      font-size: 18px;
      font-weight: 800;
      line-height: 1.15;
      letter-spacing: -.02em;
    }
    .brand-sub {
      color: #9fb5d7;
      font-size: 13px;
      line-height: 1.45;
    }
    .terminal-wrap {
      position: relative;
      min-height: 0;
      padding: 14px;
      background: linear-gradient(180deg, var(--bg) 0%, var(--bg2) 100%);
    }
    .terminal-shell {
      width: 100%;
      height: 100%;
      min-height: 0;
      border-radius: 14px;
      overflow: hidden;
      border: 1px solid rgba(62, 84, 126, .9);
      box-shadow: inset 0 0 0 1px rgba(12, 20, 38, .8);
    }
    .terminal-shell.is-focused {
      box-shadow: inset 0 0 0 1px rgba(37,99,235,.65), 0 0 0 1px rgba(37,99,235,.18);
    }
    .foot {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 12px;
      padding: 10px;
      border-top: 1px solid var(--line);
      background: rgba(255,255,255,.02);
    }
    .muted {
      color: var(--muted);
      font-size: 13px;
      line-height: 1.5;
    }
    .status {
      color: #9bc1ff;
      font-size: 12px;
      font-family: "Segoe UI", "Microsoft YaHei", sans-serif;
      white-space: nowrap;
    }
    .actions {
      display: inline-flex;
      align-items: center;
      gap: 10px;
      flex-wrap: wrap;
    }
    .btn {
      min-height: 34px;
      padding: 0 12px;
      border: 1px solid #39537d;
      border-radius: 999px;
      background: rgba(17,28,51,.85);
      color: #dbe8ff;
      font: inherit;
      cursor: pointer;
    }
    .btn:hover {
      background: rgba(33, 52, 90, .95);
    }
    .xterm, .xterm-viewport, .xterm-screen {
      height: 100%;
    }
    .xterm .xterm-viewport {
      background-color: transparent !important;
    }
  </style>
</head>
<body>
  <div class="shell">
    <div class="head">
      <div class="brand-row">
        <div class="brand-badge">__BRAND_MARK__</div>
        <div class="brand-copy">
          <strong class="brand-title">OpenClaw Terminal</strong>
          <span class="brand-sub">Commands sent from the home page run directly in this terminal window.</span>
        </div>
      </div>
    </div>
    <div class="terminal-wrap">
      <div id="terminalShell" class="terminal-shell"></div>
    </div>
    <div class="foot">
      <span class="muted">Selection, copy, paste, IME input, ANSI/TUI rendering, and terminal resize sync are supported.</span>
      <span class="actions">
        <button id="copyBtn" class="btn" type="button">Copy Selection</button>
        <button id="pasteBtn" class="btn" type="button">Paste</button>
        <span id="status" class="status">connecting</span>
      </span>
    </div>
  </div>
  <script src="./assets/xterm.js"></script>
  <script src="./assets/addon-fit.js"></script>
  <script>
    const terminalShell = document.getElementById("terminalShell");
    const statusEl = document.getElementById("status");
    const copyBtn = document.getElementById("copyBtn");
    const pasteBtn = document.getElementById("pasteBtn");
    const scheme = location.protocol === "https:" ? "wss" : "ws";
    const wsUrl = new URL("./ws", location.href);
    wsUrl.protocol = scheme + ":";
    const bootCommand = __BOOT_COMMAND__;
    const pending = [];
    const socket = new WebSocket(wsUrl.toString());
    socket.binaryType = "arraybuffer";
    const decoder = new TextDecoder();
    let statusResetTimer = null;
    let resizeTimer = null;
    let bootCommandSent = false;
    const term = new Terminal({
      allowTransparency: true,
      convertEol: true,
      cursorBlink: true,
      fontFamily: 'Cascadia Mono, Consolas, "SFMono-Regular", Menlo, Monaco, "PingFang SC", monospace',
      fontSize: 14,
      lineHeight: 1.2,
      scrollback: 5000,
      theme: {
        background: "#111c33",
        foreground: "#dbe8ff",
        cursor: "#9bc1ff",
        selectionBackground: "rgba(96, 165, 250, 0.30)"
      }
    });
    const fitAddon = new FitAddon.FitAddon();
    term.loadAddon(fitAddon);
    term.open(terminalShell);

    function flushPending() {
      while (pending.length && socket.readyState === WebSocket.OPEN) {
        socket.send(pending.shift());
      }
    }

    function setStatus(text) {
      statusEl.textContent = text;
    }

    function resetStatusSoon() {
      if (statusResetTimer) window.clearTimeout(statusResetTimer);
      statusResetTimer = window.setTimeout(() => {
        setStatus(socket.readyState === WebSocket.OPEN ? "connected" : "disconnected");
      }, 1200);
    }

    function sendPayload(payload) {
      if (!payload) return;
      if (socket.readyState === WebSocket.OPEN) {
        socket.send(payload);
        return;
      }
      pending.push(payload);
      if (socket.readyState === WebSocket.CONNECTING) return;
      term.writeln("[terminal not ready, payload queued]");
    }

    function sendTerminalInput(data) {
      if (!data) return;
      sendPayload(JSON.stringify({ type: "input", data }));
    }

    function sendResize() {
      if (!term.cols || !term.rows) return;
      sendPayload(JSON.stringify({ type: "resize", cols: term.cols, rows: term.rows }));
    }

    function sendCommand(command) {
      if (!command) return;
      sendTerminalInput(command + "\n");
    }

    function fitTerminal() {
      try {
        fitAddon.fit();
      } catch (_) {
        return;
      }
      sendResize();
    }

    function scheduleFit() {
      if (resizeTimer) window.clearTimeout(resizeTimer);
      resizeTimer = window.setTimeout(fitTerminal, 40);
    }

    function focusTerminal() {
      term.focus();
      terminalShell.classList.add("is-focused");
    }

    function blurTerminal() {
      terminalShell.classList.remove("is-focused");
    }

    socket.addEventListener("open", () => {
      setStatus("connected");
      term.writeln("[terminal connected]");
      flushPending();
      sendResize();
      if (!bootCommandSent && typeof bootCommand === "string" && bootCommand.trim()) {
        sendCommand(bootCommand);
        bootCommandSent = true;
      }
    });

    socket.addEventListener("message", (event) => {
      const decoded = typeof event.data === "string"
        ? event.data
        : decoder.decode(new Uint8Array(event.data), { stream: true });
      term.write(decoded);
    });

    socket.addEventListener("close", () => {
      setStatus("disconnected");
      term.writeln("");
      term.writeln("[terminal disconnected]");
    });

    socket.addEventListener("error", () => {
      setStatus("error");
      term.writeln("");
      term.writeln("[terminal websocket error]");
    });

    window.injectCommand = function (command) {
      sendCommand(command);
    };

    window.addEventListener("message", (event) => {
      const data = event.data;
      if (!data || typeof data !== "object") return;
      if (data.type === "openclaw-focus-terminal") {
        focusTerminal();
        return;
      }
      if (data.type !== "openclaw-run-command") return;
      if (typeof data.command !== "string" || !data.command.trim()) return;
      sendCommand(data.command);
    });

    term.onData((data) => {
      sendTerminalInput(data);
    });

    term.onSelectionChange(() => {
      copyBtn.disabled = term.getSelection().length === 0;
    });

    async function copySelection() {
      const selected = term.getSelection();
      if (!selected) return;
      try {
        await navigator.clipboard.writeText(selected);
        setStatus("copied");
        resetStatusSoon();
      } catch (_) {
        setStatus("copy-failed");
      }
    }

    async function pasteClipboardText(text) {
      if (!text) return;
      sendTerminalInput(text);
      setStatus("pasted");
      resetStatusSoon();
    }

    async function pasteFromClipboard() {
      try {
        const text = await navigator.clipboard.readText();
        await pasteClipboardText(text);
      } catch (_) {
        setStatus("paste-failed");
      }
    }

    copyBtn.addEventListener("click", async () => {
      await copySelection();
    });

    pasteBtn.addEventListener("click", async () => {
      await pasteFromClipboard();
    });

    term.attachCustomKeyEventHandler((event) => {
      if (event.type !== "keydown") return true;
      const key = event.key.toLowerCase();
      const isAccel = event.ctrlKey || event.metaKey;
      if (isAccel && event.shiftKey && key === "c" && term.getSelection()) {
        void copySelection();
        return false;
      }
      if (isAccel && event.shiftKey && key === "v") {
        void pasteFromClipboard();
        return false;
      }
      if (event.shiftKey && key === "insert") {
        void pasteFromClipboard();
        return false;
      }
      if (event.ctrlKey && key === "insert" && term.getSelection()) {
        void copySelection();
        return false;
      }
      return true;
    });

    terminalShell.addEventListener("paste", (event) => {
      const text = event.clipboardData ? event.clipboardData.getData("text") : "";
      if (!text) return;
      event.preventDefault();
      void pasteClipboardText(text);
    });

    terminalShell.addEventListener("click", focusTerminal);
    terminalShell.addEventListener("focusin", focusTerminal);
    terminalShell.addEventListener("focusout", blurTerminal);
    window.addEventListener("focus", focusTerminal);
    window.addEventListener("resize", scheduleFit);
    if (typeof ResizeObserver !== "undefined") {
      new ResizeObserver(scheduleFit).observe(terminalShell);
    }
    if (document.fonts && document.fonts.ready) {
      document.fonts.ready.then(scheduleFit).catch(() => {});
    }
    if (bootCommand) {
      const cleanUrl = new URL(location.href);
      cleanUrl.searchParams.delete("command");
      window.history.replaceState(null, "", cleanUrl.toString());
    }
    copyBtn.disabled = true;
    pasteBtn.disabled = false;
    fitTerminal();
    focusTerminal();
  </script>
</body>
</html>"##
            .replace("__BRAND_MARK__", &openclaw_brand_svg("brand-mark"))
            .replace("__BOOT_COMMAND__", &boot_command),
    )
}

async fn terminal_xterm_js() -> impl IntoResponse {
    cached_file_response(
        "/usr/local/lib/node_modules/@xterm/xterm/lib/xterm.js",
        "application/javascript; charset=utf-8",
    )
    .await
}

async fn terminal_xterm_css() -> impl IntoResponse {
    cached_file_response(
        "/usr/local/lib/node_modules/@xterm/xterm/css/xterm.css",
        "text/css; charset=utf-8",
    )
    .await
}

async fn terminal_xterm_addon_fit_js() -> impl IntoResponse {
    cached_file_response(
        "/usr/local/lib/node_modules/@xterm/addon-fit/lib/addon-fit.js",
        "application/javascript; charset=utf-8",
    )
    .await
}

async fn terminal_ws(State(state): State<AppState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    if !state.enable_terminal {
        return StatusCode::NOT_FOUND.into_response();
    }
    println!("ingressd: terminal websocket upgrade requested");
    ws.on_upgrade(handle_terminal_socket).into_response()
}

async fn handle_terminal_socket(socket: WebSocket) {
    println!("ingressd: terminal websocket connected");
    let pty_system = native_pty_system();
    let Ok(pair) = pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }) else {
        eprintln!("ingressd: failed to open PTY");
        return;
    };

    let shell = env::var("SHELL").unwrap_or_else(|_| "bash".to_string());
    let cmd = CommandBuilder::new(shell);
    let Ok(mut child) = pair.slave.spawn_command(cmd) else {
        eprintln!("ingressd: failed to spawn shell in PTY");
        return;
    };
    drop(pair.slave);

    let Ok(mut reader) = pair.master.try_clone_reader() else {
        eprintln!("ingressd: failed to clone PTY reader");
        let _ = child.kill();
        return;
    };
    let Ok(writer) = pair.master.take_writer() else {
        eprintln!("ingressd: failed to take PTY writer");
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
        let master = pair.master;
        tokio::spawn(async move {
            while let Some(Ok(message)) = receiver.next().await {
                match message {
                    AxumWsMessage::Text(text) => match parse_terminal_client_text(&text) {
                        TerminalClientAction::Input(data) => {
                            if let Ok(mut handle) = writer.lock() {
                                let _ = handle.write_all(&data);
                                let _ = handle.flush();
                            }
                        }
                        TerminalClientAction::Resize(size) => {
                            let _ = master.resize(size);
                        }
                    },
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
    println!("ingressd: terminal websocket disconnected");
    let _ = child.kill();
}

async fn proxy_health(State(state): State<AppState>, request: Request) -> impl IntoResponse {
    proxy_http_request(&state.client, &state.action_base, request, false, None).await
}

async fn proxy_action(
    State(state): State<AppState>,
    Path(action): Path<String>,
    request: Request,
) -> impl IntoResponse {
    let _ = action;
    proxy_http_request(&state.client, &state.action_base, request, false, None).await
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

/// Like `file_response` but adds a 1-day browser cache header.
/// Used for npm-installed static assets that never change between restarts.
async fn cached_file_response(path: &str, content_type: &str) -> impl IntoResponse {
    match fs::read(path) {
        Ok(bytes) => (
            [
                (axum::http::header::CONTENT_TYPE, content_type),
                (
                    axum::http::header::CACHE_CONTROL,
                    "public, max-age=86400, immutable",
                ),
            ],
            bytes,
        )
            .into_response(),
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
        <a class="btn" href="./terminal/">Open Terminal</a>
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

enum TerminalClientAction {
    Input(Vec<u8>),
    Resize(PtySize),
}

fn parse_terminal_client_text(text: &str) -> TerminalClientAction {
    match serde_json::from_str::<TerminalClientMessage>(text) {
        Ok(TerminalClientMessage::Input { data }) => TerminalClientAction::Input(data.into_bytes()),
        Ok(TerminalClientMessage::Resize { cols, rows }) => {
            TerminalClientAction::Resize(normalized_terminal_size(cols, rows))
        }
        Err(_) => TerminalClientAction::Input(text.as_bytes().to_vec()),
    }
}

fn normalized_terminal_size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows: rows.max(2),
        cols: cols.max(10),
        pixel_width: 0,
        pixel_height: 0,
    }
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

    #[test]
    fn terminal_protocol_parses_resize_messages() {
        let action = parse_terminal_client_text(r#"{"type":"resize","cols":132,"rows":43}"#);

        match action {
            TerminalClientAction::Resize(size) => {
                assert_eq!(size.cols, 132);
                assert_eq!(size.rows, 43);
            }
            TerminalClientAction::Input(_) => panic!("expected resize action"),
        }
    }

    #[test]
    fn terminal_protocol_keeps_plain_input_backward_compatible() {
        let action = parse_terminal_client_text("echo hello\n");

        match action {
            TerminalClientAction::Input(data) => assert_eq!(data, b"echo hello\n"),
            TerminalClientAction::Resize(_) => panic!("expected input action"),
        }
    }

    #[test]
    fn brand_logo_uses_fixed_aspect_svg() {
        let svg = openclaw_brand_svg("brand-mark");

        assert!(svg.contains("class=\"brand-mark\""));
        assert!(svg.contains("preserveAspectRatio=\"xMidYMid meet\""));
    }
}
