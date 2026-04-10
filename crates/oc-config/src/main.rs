use clap::{Parser, Subcommand};
use serde_json::{Map, Value, json};
use std::{env, fs, path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(name = "oc-config")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Get {
        path: String,
    },
    Set {
        path: String,
        value: String,
        #[arg(long)]
        json: bool,
    },
    ApplyGatewaySettings {
        mode: String,
        remote_url: String,
        bind_mode: String,
        port: u16,
        enable_openai_api: String,
        auth_mode: String,
        trusted_proxies_csv: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let config_file = config_path();
    let mut cfg = load_config(&config_file);

    match cli.command {
        Command::Get { path } => {
            if let Some(value) = get_path(&cfg, &path) {
                if value.is_string() {
                    println!("{}", value.as_str().unwrap_or_default());
                } else {
                    println!("{value}");
                }
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Command::Set { path, value, json } => {
            let parsed = if json {
                serde_json::from_str(&value).unwrap_or(Value::Null)
            } else {
                Value::String(value)
            };
            set_path(&mut cfg, &path, parsed);
            save_config(&config_file, &cfg)
        }
        Command::ApplyGatewaySettings {
            mode,
            remote_url,
            bind_mode,
            port,
            enable_openai_api,
            auth_mode,
            trusted_proxies_csv,
        } => {
            let enable_openai_api = parse_boolish(&enable_openai_api);
            apply_gateway_settings(
                &mut cfg,
                &mode,
                &remote_url,
                &bind_mode,
                port,
                enable_openai_api,
                &auth_mode,
                &trusted_proxies_csv,
            );
            save_config(&config_file, &cfg)
        }
    }
}

fn config_path() -> PathBuf {
    env::var("OPENCLAW_CONFIG_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/config/.openclaw/openclaw.json"))
}

fn load_config(path: &PathBuf) -> Value {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or_else(|| json!({}))
}

fn save_config(path: &PathBuf, cfg: &Value) -> ExitCode {
    if let Some(parent) = path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            eprintln!("ERROR: Failed to create config directory: {err}");
            return ExitCode::from(1);
        }
    }
    match serde_json::to_string_pretty(cfg) {
        Ok(text) => {
            if let Err(err) = fs::write(path, format!("{text}\n")) {
                eprintln!("ERROR: Failed to write config: {err}");
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(err) => {
            eprintln!("ERROR: Failed to serialize config: {err}");
            ExitCode::from(1)
        }
    }
}

fn get_path<'a>(cfg: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = cfg;
    for part in path.split('.').filter(|part| !part.is_empty()) {
        current = current.get(part)?;
    }
    Some(current)
}

fn set_path(cfg: &mut Value, path: &str, value: Value) {
    let parts: Vec<&str> = path.split('.').filter(|part| !part.is_empty()).collect();
    if parts.is_empty() {
        return;
    }

    let mut current = cfg;
    for part in &parts[..parts.len() - 1] {
        if !current.is_object() {
            *current = Value::Object(Map::new());
        }
        let object = current.as_object_mut().expect("object");
        current = object
            .entry((*part).to_string())
            .or_insert_with(|| Value::Object(Map::new()));
    }
    if !current.is_object() {
        *current = Value::Object(Map::new());
    }
    current
        .as_object_mut()
        .expect("object")
        .insert(parts[parts.len() - 1].to_string(), value);
}

fn parse_boolish(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "y" | "on"
    )
}

fn apply_gateway_settings(
    cfg: &mut Value,
    mode: &str,
    remote_url: &str,
    bind_mode: &str,
    port: u16,
    enable_openai_api: bool,
    auth_mode: &str,
    trusted_proxies_csv: &str,
) {
    set_path(cfg, "gateway.mode", Value::String(mode.to_string()));
    set_path(
        cfg,
        "gateway.remote.url",
        Value::String(remote_url.to_string()),
    );
    set_path(cfg, "gateway.bind", Value::String(bind_mode.to_string()));
    set_path(cfg, "gateway.port", Value::Number(port.into()));
    set_path(
        cfg,
        "gateway.http.endpoints.chatCompletions.enabled",
        Value::Bool(enable_openai_api),
    );
    set_path(
        cfg,
        "gateway.auth.mode",
        Value::String(auth_mode.to_string()),
    );
    let proxies = trusted_proxies_csv
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| Value::String(item.to_string()))
        .collect::<Vec<_>>();
    set_path(cfg, "gateway.trustedProxies", Value::Array(proxies));
}
