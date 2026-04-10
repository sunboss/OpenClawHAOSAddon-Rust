use axum::{
    Json, Router,
    extract::{Path, State},
    http::{StatusCode, header},
    response::IntoResponse,
    routing::{get, post},
};
use serde::Serialize;
use std::{collections::HashMap, env, fs, net::SocketAddr, process::Stdio, sync::Arc};
use tokio::net::TcpStream;
use tokio::process::Command;
use tokio::time::{Duration, sleep, timeout};

#[derive(Clone)]
struct AppState {
    commands: Arc<HashMap<&'static str, Vec<&'static str>>>,
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
    let mut commands = HashMap::new();
    commands.insert("status", vec!["openclaw", "gateway", "status", "--deep"]);
    commands.insert("restart", vec!["openclaw", "gateway", "restart"]);

    let state = AppState {
        commands: Arc::new(commands),
    };

    let host = env::var("ACTION_SERVER_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = env::var("ACTION_SERVER_PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(48100);

    let app = Router::new()
        .route("/health", get(health))
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/action/{action}", post(run_action))
        .with_state(state);

    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .expect("valid bind address");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind action server");
    println!("actiond: listening on http://{addr}");
    axum::serve(listener, app).await.expect("serve actiond");
}

async fn health() -> impl IntoResponse {
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

async fn healthz() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        "ok\n",
    )
}

async fn readyz() -> impl IntoResponse {
    let probe = local_gateway_probe().await;
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

async fn run_action(
    State(state): State<AppState>,
    Path(action): Path<String>,
) -> impl IntoResponse {
    if action == "restart" {
        return restart_managed_gateway().await;
    }

    let Some(cmd) = state.commands.get(action.as_str()) else {
        return (
            StatusCode::NOT_FOUND,
            Json(ActionResponse {
                ok: false,
                action,
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                error: Some("unknown_action".to_string()),
            }),
        );
    };

    let timeout_secs = if action == "restart" { 60 } else { 20 };
    run_command(&action, cmd, timeout_secs).await
}

async fn restart_managed_gateway() -> (StatusCode, Json<ActionResponse>) {
    let pid_path = "/run/openclaw-rs/openclaw-gateway.pid";
    let old_pid = match fs::read_to_string(pid_path) {
        Ok(value) => value.trim().to_string(),
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ActionResponse {
                    ok: false,
                    action: "restart".to_string(),
                    exit_code: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    error: Some(format!("missing_pid_file: {err}")),
                }),
            );
        }
    };

    if old_pid.is_empty() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ActionResponse {
                ok: false,
                action: "restart".to_string(),
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                error: Some("empty_gateway_pid".to_string()),
            }),
        );
    }

    let kill_output = match Command::new("kill")
        .args(["-TERM", &old_pid])
        .output()
        .await
    {
        Ok(output) => output,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ActionResponse {
                    ok: false,
                    action: "restart".to_string(),
                    exit_code: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    error: Some(err.to_string()),
                }),
            );
        }
    };

    if !kill_output.status.success() {
        return (
            StatusCode::BAD_GATEWAY,
            Json(ActionResponse {
                ok: false,
                action: "restart".to_string(),
                exit_code: kill_output.status.code(),
                stdout: String::from_utf8_lossy(&kill_output.stdout)
                    .trim()
                    .to_string(),
                stderr: String::from_utf8_lossy(&kill_output.stderr)
                    .trim()
                    .to_string(),
                error: Some("kill_failed".to_string()),
            }),
        );
    }

    for _ in 0..100 {
        sleep(Duration::from_millis(200)).await;
        if let Ok(value) = fs::read_to_string(pid_path) {
            let new_pid = value.trim().to_string();
            if !new_pid.is_empty() && new_pid != old_pid {
                return (
                    StatusCode::OK,
                    Json(ActionResponse {
                        ok: true,
                        action: "restart".to_string(),
                        exit_code: Some(0),
                        stdout: format!("gateway restarted: {old_pid} -> {new_pid}"),
                        stderr: String::new(),
                        error: None,
                    }),
                );
            }
        }
    }

    (
        StatusCode::GATEWAY_TIMEOUT,
        Json(ActionResponse {
            ok: false,
            action: "restart".to_string(),
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error: Some("restart_timeout".to_string()),
        }),
    )
}

async fn run_command(
    action: &str,
    argv: &[&str],
    timeout_secs: u64,
) -> (StatusCode, Json<ActionResponse>) {
    let Some((program, args)) = argv.split_first() else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ActionResponse {
                ok: false,
                action: action.to_string(),
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                error: Some("empty_command".to_string()),
            }),
        );
    };

    let mut command = Command::new(program);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        command.output(),
    )
    .await
    {
        Ok(Ok(output)) => {
            let ok = output.status.success();
            let status = if ok {
                StatusCode::OK
            } else {
                StatusCode::BAD_GATEWAY
            };
            (
                status,
                Json(ActionResponse {
                    ok,
                    action: action.to_string(),
                    exit_code: output.status.code(),
                    stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
                    error: None,
                }),
            )
        }
        Ok(Err(err)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ActionResponse {
                ok: false,
                action: action.to_string(),
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(err.to_string()),
            }),
        ),
        Err(_) => (
            StatusCode::GATEWAY_TIMEOUT,
            Json(ActionResponse {
                ok: false,
                action: action.to_string(),
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                error: Some("timeout".to_string()),
            }),
        ),
    }
}

async fn local_gateway_probe() -> GatewayProbe {
    let port = env::var("GATEWAY_INTERNAL_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(18790);
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
    use super::{gateway_process_requires_local_port, select_gateway_process};

    #[test]
    fn select_gateway_process_prefers_gateway_pid() {
        assert_eq!(
            Some(("openclaw-gateway", "55".to_string())),
            select_gateway_process(Some("55".to_string()), Some("66".to_string()))
        );
    }

    #[test]
    fn select_gateway_process_falls_back_to_node_pid() {
        assert_eq!(
            Some(("openclaw-node", "66".to_string())),
            select_gateway_process(None, Some("66".to_string()))
        );
    }

    #[test]
    fn select_gateway_process_returns_none_without_pid_files() {
        assert_eq!(None, select_gateway_process(None, None));
    }

    #[test]
    fn remote_node_mode_does_not_require_local_probe_port() {
        assert!(!gateway_process_requires_local_port("remote", "openclaw-node"));
    }

    #[test]
    fn local_gateway_mode_requires_local_probe_port() {
        assert!(gateway_process_requires_local_port("local", "openclaw-gateway"));
    }
}
