use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde::Serialize;
use std::{collections::HashMap, env, fs, net::SocketAddr, process::Stdio, sync::Arc};
use tokio::process::Command;
use tokio::time::{Duration, sleep};

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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut commands = HashMap::new();
    commands.insert("status", vec!["openclaw", "gateway", "status", "--deep"]);

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
        .route("/action/{action}", post(run_action))
        .with_state(state);

    let addr: SocketAddr = format!("{host}:{port}").parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("actiond: listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> impl IntoResponse {
    run_command("health", &["openclaw", "health", "--json"], 20).await
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
    let Some((runtime_name, pid_path, current_pid)) = active_runtime_pid() else {
        return (
            StatusCode::BAD_GATEWAY,
            Json(ActionResponse {
                ok: false,
                action: "restart".to_string(),
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                error: Some("no_managed_runtime_pid".to_string()),
            }),
        );
    };

    if current_pid.is_empty() {
        return (
            StatusCode::BAD_GATEWAY,
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

    let output = match Command::new("kill")
        .args(["-TERM", &current_pid])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
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
                    error: Some(format!("failed_to_signal_gateway: {err}")),
                }),
            );
        }
    };

    if !output.status.success() {
        return (
            StatusCode::BAD_GATEWAY,
            Json(ActionResponse {
                ok: false,
                action: "restart".to_string(),
                exit_code: output.status.code(),
                stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
                error: Some("restart_signal_failed".to_string()),
            }),
        );
    }

    for _ in 0..40 {
        sleep(Duration::from_millis(500)).await;
        if let Ok(new_pid) = fs::read_to_string(pid_path) {
            let new_pid = new_pid.trim().to_string();
            if !new_pid.is_empty() && new_pid != current_pid {
                return (
                    StatusCode::OK,
                    Json(ActionResponse {
                        ok: true,
                        action: "restart".to_string(),
                        exit_code: Some(0),
                        stdout: format!("{runtime_name} restarted: {current_pid} -> {new_pid}"),
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
            stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            error: Some("gateway_pid_did_not_change".to_string()),
        }),
    )
}

fn active_runtime_pid() -> Option<(&'static str, &'static str, String)> {
    for (runtime_name, pid_path) in [
        ("gateway", "/run/openclaw-rs/openclaw-gateway.pid"),
        ("node", "/run/openclaw-rs/openclaw-node.pid"),
    ] {
        if let Ok(value) = fs::read_to_string(pid_path) {
            let pid = value.trim().to_string();
            if !pid.is_empty() {
                return Some((runtime_name, pid_path, pid));
            }
        }
    }
    None
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
