use clap::{Args, Parser, Subcommand};
use rand::random;
use serde::Deserialize;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command as StdCommand, ExitCode},
    time::Duration,
};
use tokio::{process::Command, signal, sync::watch, time::sleep};
use url::Url;

#[derive(Parser)]
#[command(name = "addon-supervisor")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Plan,
    HaosEntry(HaosEntryArgs),
    RenderNginx {
        #[arg(long, default_value = "/etc/nginx/nginx.conf")]
        output: PathBuf,
    },
    RunServices {
        #[arg(long, default_value = "openclaw")]
        gateway_bin: String,
        #[arg(long, default_value = "haos-ui")]
        ui_bin: String,
        #[arg(long, default_value = "actiond")]
        action_bin: String,
        #[arg(long, default_value = "ingressd")]
        ingress_bin: String,
        #[arg(long, default_value = "local")]
        gateway_mode: String,
        #[arg(long, default_value = "")]
        gateway_remote_url: String,
        #[arg(long, default_value_t = true)]
        auto_approve_pairing: bool,
        #[arg(long, default_value_t = true)]
        run_doctor_on_start: bool,
        #[arg(long, default_value_t = 7681)]
        terminal_port: u16,
        #[arg(long, default_value_t = true)]
        enable_terminal: bool,
    },
}

#[derive(Args, Clone, Debug)]
struct HaosEntryArgs {
    #[arg(long, default_value = "/data/options.json")]
    options_file: PathBuf,
    #[arg(long, default_value = "/config/.openclaw")]
    openclaw_config_dir: PathBuf,
    #[arg(long, default_value = "/config/.openclaw/openclaw.json")]
    openclaw_config_path: PathBuf,
    #[arg(long, default_value = "/config/.openclaw/workspace")]
    openclaw_workspace_dir: PathBuf,
    #[arg(long, default_value = "/config/.mcporter")]
    mcporter_home_dir: PathBuf,
    #[arg(long, default_value = "/config/.mcporter/mcporter.json")]
    mcporter_config: PathBuf,
    #[arg(long, default_value = "/config/certs")]
    cert_dir: PathBuf,
    #[arg(long, default_value = "/share/openclaw-backup/latest")]
    backup_dir: PathBuf,
    #[arg(long, default_value = "/etc/nginx/html")]
    nginx_html_dir: PathBuf,
    #[arg(long, default_value_t = 18790)]
    gateway_internal_port: u16,
    #[arg(long, default_value_t = 48100)]
    action_server_port: u16,
    #[arg(long, default_value_t = 48101)]
    ui_port: u16,
    #[arg(long, default_value = "openclaw")]
    gateway_bin: String,
    #[arg(long, default_value = "oc-config")]
    oc_config_bin: String,
    #[arg(long, default_value = "mcporter")]
    mcporter_bin: String,
    #[arg(long, default_value = "haos-ui")]
    ui_bin: String,
    #[arg(long, default_value = "actiond")]
    action_bin: String,
    #[arg(long, default_value = "ingressd")]
    ingress_bin: String,
    #[arg(long, default_value = "/etc/nginx/nginx.conf")]
    nginx_conf: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct AddonOptions {
    timezone: Option<String>,
    enable_terminal: Option<bool>,
    terminal_port: Option<u16>,
    gateway_mode: Option<String>,
    gateway_remote_url: Option<String>,
    gateway_public_url: Option<String>,
    gateway_port: Option<u16>,
    access_mode: Option<String>,
    enable_openai_api: Option<bool>,
    auto_configure_mcp: Option<bool>,
    homeassistant_token: Option<String>,
    auto_approve_device_pairing: Option<bool>,
    run_doctor_on_start: Option<bool>,
}

#[derive(Debug, Clone)]
struct RuntimeSettings {
    timezone: String,
    enable_terminal: bool,
    terminal_port: u16,
    gateway_mode: String,
    gateway_remote_url: String,
    gateway_public_url: String,
    https_port: u16,
    access_mode: String,
    enable_openai_api: bool,
    auto_configure_mcp: bool,
    homeassistant_token: String,
    auto_approve_pairing: bool,
    run_doctor_on_start: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Plan) => {
            println!("Planned Rust replacements:");
            println!("- shell bootstrap in run.sh");
            println!("- nginx render pipeline");
            println!("- process supervision for openclaw, ttyd, nginx, and UI services");
            ExitCode::SUCCESS
        }
        Some(Commands::HaosEntry(args)) => haos_entry(args),
        Some(Commands::RenderNginx { output }) => render_nginx(output),
        Some(Commands::RunServices {
            gateway_bin,
            ui_bin,
            action_bin,
            ingress_bin,
            gateway_mode,
            gateway_remote_url,
            auto_approve_pairing,
            run_doctor_on_start,
            terminal_port,
            enable_terminal,
        }) => run_services(
            gateway_bin,
            ui_bin,
            action_bin,
            ingress_bin,
            gateway_mode,
            gateway_remote_url,
            auto_approve_pairing,
            run_doctor_on_start,
            terminal_port,
            enable_terminal,
        ),
        None => {
            println!("addon-supervisor scaffold ready");
            println!("Next step: replace run.sh orchestration with Rust process control.");
            ExitCode::SUCCESS
        }
    }
}

fn haos_entry(args: HaosEntryArgs) -> ExitCode {
    let options = load_options(&args.options_file);
    let settings = runtime_settings(&options);

    if let Err(err) = prepare_directories(&args) {
        eprintln!("addon-supervisor: failed to prepare directories: {err}");
        return ExitCode::from(1);
    }

    if let Err(err) = ensure_home_symlinks(&args) {
        eprintln!("addon-supervisor: failed to prepare home links: {err}");
        return ExitCode::from(1);
    }

    if let Err(err) = bootstrap_openclaw_config(&args, &settings) {
        eprintln!("addon-supervisor: failed to bootstrap OpenClaw config: {err}");
        return ExitCode::from(1);
    }

    apply_runtime_env(&args, &settings);

    if !apply_gateway_settings(&args, &settings) {
        return ExitCode::from(1);
    }

    let gateway_token =
        run_capture(&args.oc_config_bin, &["get", "gateway.auth.token"]).unwrap_or_default();

    if !write_gateway_token_file(&args, &gateway_token) {
        return ExitCode::from(1);
    }

    if !ensure_certificate_files(&args) {
        return ExitCode::from(1);
    }

    let mut mcp_status = "disabled".to_string();
    if settings.auto_configure_mcp
        && !settings.homeassistant_token.is_empty()
        && command_exists(&args.mcporter_bin)
    {
        let _ = run_status(
            &args.mcporter_bin,
            &[
                "add",
                "HA",
                "--http-url",
                "http://supervisor/core/api/mcp",
                "--header",
                &format!("Authorization: Bearer {}", settings.homeassistant_token),
            ],
        );
        mcp_status = "HA configured".to_string();
    }

    let web_provider = run_capture(&args.oc_config_bin, &["get", "tools.web.search.provider"])
        .unwrap_or_else(|| "disabled".to_string());
    let memory_provider = run_capture(
        &args.oc_config_bin,
        &["get", "agents.defaults.memorySearch.provider"],
    )
    .unwrap_or_else(|| "disabled".to_string());

    let add_on_version = detect_addon_version();
    let openclaw_version = detect_openclaw_version(&args.gateway_bin);
    apply_status_env(
        &add_on_version,
        &openclaw_version,
        &mcp_status,
        &web_provider,
        &memory_provider,
    );
    backup_state(&args);

    run_services(
        args.gateway_bin,
        args.ui_bin,
        args.action_bin,
        args.ingress_bin,
        settings.gateway_mode,
        settings.gateway_remote_url,
        settings.auto_approve_pairing,
        settings.run_doctor_on_start,
        settings.terminal_port,
        settings.enable_terminal,
    )
}

fn load_options(path: &Path) -> AddonOptions {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<AddonOptions>(&text).ok())
        .unwrap_or_default()
}

fn runtime_settings(options: &AddonOptions) -> RuntimeSettings {
    RuntimeSettings {
        timezone: options
            .timezone
            .clone()
            .unwrap_or_else(|| "Asia/Shanghai".to_string()),
        enable_terminal: options.enable_terminal.unwrap_or(true),
        terminal_port: options.terminal_port.unwrap_or(7681),
        gateway_mode: options
            .gateway_mode
            .clone()
            .unwrap_or_else(|| "local".to_string()),
        gateway_remote_url: options.gateway_remote_url.clone().unwrap_or_default(),
        gateway_public_url: options.gateway_public_url.clone().unwrap_or_default(),
        https_port: options.gateway_port.unwrap_or(18789),
        access_mode: options
            .access_mode
            .clone()
            .unwrap_or_else(|| "lan_https".to_string()),
        enable_openai_api: options.enable_openai_api.unwrap_or(true),
        auto_configure_mcp: options.auto_configure_mcp.unwrap_or(true),
        homeassistant_token: options.homeassistant_token.clone().unwrap_or_default(),
        auto_approve_pairing: options.auto_approve_device_pairing.unwrap_or(true),
        run_doctor_on_start: options.run_doctor_on_start.unwrap_or(true),
    }
}

fn prepare_directories(args: &HaosEntryArgs) -> std::io::Result<()> {
    for path in [
        &args.openclaw_config_dir,
        &args.openclaw_config_dir.join("agents"),
        &args.openclaw_config_dir.join("agents/main"),
        &args.openclaw_config_dir.join("agents/main/sessions"),
        &args.openclaw_config_dir.join("agents/main/agent"),
        &args.openclaw_config_dir.join("identity"),
        &args.openclaw_workspace_dir,
        &args.openclaw_workspace_dir.join("memory"),
        &args.mcporter_home_dir,
        &args.cert_dir,
        &args.backup_dir,
        &args.nginx_html_dir,
    ] {
        fs::create_dir_all(path)?;
    }
    Ok(())
}

fn ensure_home_symlinks(args: &HaosEntryArgs) -> std::io::Result<()> {
    let root_openclaw = Path::new("/root/.openclaw");
    if root_openclaw.exists() {
        let metadata = fs::symlink_metadata(root_openclaw)?;
        if metadata.file_type().is_symlink() {
            let current = fs::read_link(root_openclaw)?;
            if current != args.openclaw_config_dir {
                fs::remove_file(root_openclaw)?;
                create_dir_symlink(&args.openclaw_config_dir, root_openclaw)?;
            }
        } else if metadata.is_dir() {
            for entry in fs::read_dir(root_openclaw)? {
                let entry = entry?;
                let target = args.openclaw_config_dir.join(entry.file_name());
                if !target.exists() {
                    if entry.file_type()?.is_dir() {
                        fs::create_dir_all(&target)?;
                    } else {
                        let _ = fs::copy(entry.path(), &target);
                    }
                }
            }
        }
    } else {
        create_dir_symlink(&args.openclaw_config_dir, root_openclaw)?;
    }
    Ok(())
}

#[cfg(unix)]
fn create_dir_symlink(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(src, dst)
}

#[cfg(not(unix))]
fn create_dir_symlink(_src: &Path, _dst: &Path) -> std::io::Result<()> {
    Ok(())
}

fn bootstrap_openclaw_config(
    args: &HaosEntryArgs,
    settings: &RuntimeSettings,
) -> std::io::Result<()> {
    if args.openclaw_config_path.exists() {
        let existing = fs::read_to_string(&args.openclaw_config_path)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
        if let Some(mut config) = existing {
            let mut changed = false;
            if config.get("workspaceDir").is_some() {
                if let Some(object) = config.as_object_mut() {
                    object.remove("workspaceDir");
                    changed = true;
                }
            }
            if changed {
                ensure_workspace_path(&mut config, args);
                fs::write(
                    &args.openclaw_config_path,
                    format!(
                        "{}\n",
                        serde_json::to_string_pretty(&config).unwrap_or_else(|_| "{}".to_string())
                    ),
                )?;
            }
        }
        return Ok(());
    }

    let token = generate_gateway_token();
    let mut config = serde_json::json!({
        "gateway": {
            "mode": "local",
            "bind": "loopback",
            "port": args.gateway_internal_port,
            "trustedProxies": ["127.0.0.1/32", "::1/128"],
            "auth": {
                "mode": "token",
                "token": token
            },
            "http": {
                "endpoints": {
                    "chatCompletions": {
                        "enabled": settings.enable_openai_api
                    }
                }
            }
        }
    });
    ensure_workspace_path(&mut config, args);

    if let Some(parent) = args.openclaw_config_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        &args.openclaw_config_path,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&config).unwrap_or_else(|_| "{}".to_string())
        ),
    )?;
    Ok(())
}

fn ensure_workspace_path(config: &mut serde_json::Value, args: &HaosEntryArgs) {
    if !config.is_object() {
        *config = serde_json::json!({});
    }

    let root = config.as_object_mut().expect("config object");
    let agents = root
        .entry("agents".to_string())
        .or_insert_with(|| serde_json::json!({}));
    if !agents.is_object() {
        *agents = serde_json::json!({});
    }

    let defaults = agents
        .as_object_mut()
        .expect("agents object")
        .entry("defaults".to_string())
        .or_insert_with(|| serde_json::json!({}));
    if !defaults.is_object() {
        *defaults = serde_json::json!({});
    }

    defaults.as_object_mut().expect("defaults object").insert(
        "workspace".to_string(),
        serde_json::Value::String(args.openclaw_workspace_dir.display().to_string()),
    );
}

fn generate_gateway_token() -> String {
    let bytes: [u8; 24] = random();
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn apply_runtime_env(args: &HaosEntryArgs, settings: &RuntimeSettings) {
    unsafe {
        env::set_var("HOME", "/config");
        env::set_var("TZ", &settings.timezone);
        env::set_var("OPENCLAW_CONFIG_DIR", &args.openclaw_config_dir);
        env::set_var("OPENCLAW_CONFIG_PATH", &args.openclaw_config_path);
        env::set_var("OPENCLAW_WORKSPACE_DIR", &args.openclaw_workspace_dir);
        env::set_var("XDG_CONFIG_HOME", "/config");
        env::set_var("OPENCLAW_NO_RESPAWN", "1");
        env::set_var("MCPORTER_HOME_DIR", &args.mcporter_home_dir);
        env::set_var("MCPORTER_CONFIG", &args.mcporter_config);
        env::set_var("ACTION_SERVER_PORT", args.action_server_port.to_string());
        env::set_var("UI_PORT", args.ui_port.to_string());
        env::set_var("INGRESS_PORT", "48099");
        env::set_var("ACCESS_MODE", &settings.access_mode);
        env::set_var("GATEWAY_MODE", &settings.gateway_mode);
        env::set_var("GW_PUBLIC_URL", &settings.gateway_public_url);
        env::set_var("HTTPS_PORT", settings.https_port.to_string());
        env::set_var(
            "ENABLE_TERMINAL",
            if settings.enable_terminal {
                "true"
            } else {
                "false"
            },
        );
        env::set_var("ENABLE_HTTPS_PROXY", "true");
        env::set_var("HTTPS_PROXY_PORT", settings.https_port.to_string());
        env::set_var(
            "GATEWAY_INTERNAL_PORT",
            args.gateway_internal_port.to_string(),
        );
        env::set_var("NGINX_LOG_LEVEL", "minimal");
    }
}

fn apply_status_env(
    add_on_version: &str,
    openclaw_version: &str,
    mcp_status: &str,
    web_provider: &str,
    memory_provider: &str,
) {
    unsafe {
        env::set_var("ADDON_VERSION", add_on_version);
        env::set_var("OPENCLAW_VERSION", openclaw_version);
        env::set_var("MCP_STATUS", mcp_status);
        env::set_var("WEB_SEARCH_PROVIDER", web_provider);
        env::set_var("MEMORY_SEARCH_PROVIDER", memory_provider);
    }
}

fn apply_gateway_settings(args: &HaosEntryArgs, settings: &RuntimeSettings) -> bool {
    run_status(
        &args.oc_config_bin,
        &[
            "apply-gateway-settings",
            &settings.gateway_mode,
            &settings.gateway_remote_url,
            "loopback",
            &args.gateway_internal_port.to_string(),
            if settings.enable_openai_api {
                "true"
            } else {
                "false"
            },
            "token",
            "127.0.0.1/32,::1/128",
        ],
    )
}

fn write_gateway_token_file(args: &HaosEntryArgs, token: &str) -> bool {
    let path = args.nginx_html_dir.join("gateway.token");
    if let Err(err) = fs::write(&path, token) {
        eprintln!(
            "addon-supervisor: failed to write gateway token file {}: {}",
            path.display(),
            err
        );
        return false;
    }
    let _ = set_mode_600(&path);
    true
}

fn ensure_certificate_files(args: &HaosEntryArgs) -> bool {
    let gateway_key = args.cert_dir.join("gateway.key");
    let gateway_crt = args.cert_dir.join("gateway.crt");
    if !gateway_key.exists() || !gateway_crt.exists() {
        let status = StdCommand::new("openssl")
            .args(["req", "-x509", "-nodes", "-newkey", "rsa:2048", "-keyout"])
            .arg(&gateway_key)
            .args(["-out"])
            .arg(&gateway_crt)
            .args(["-days", "3650", "-subj", "/CN=OpenClawHAOSAddon-Rust"])
            .status();
        match status {
            Ok(result) if result.success() => {}
            Ok(result) => {
                eprintln!(
                    "addon-supervisor: openssl exited with status {:?}",
                    result.code()
                );
                return false;
            }
            Err(err) => {
                eprintln!("addon-supervisor: failed to invoke openssl: {err}");
                return false;
            }
        }
    }

    let ca_target = args.nginx_html_dir.join("openclaw-ca.crt");
    if let Err(err) = fs::copy(&gateway_crt, &ca_target) {
        eprintln!(
            "addon-supervisor: failed to copy certificate to {}: {}",
            ca_target.display(),
            err
        );
        return false;
    }
    let _ = set_mode_600(&gateway_key);
    let _ = set_mode_600(&args.openclaw_config_path);
    true
}

#[cfg(unix)]
fn set_mode_600(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, PermissionsExt::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_mode_600(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

fn detect_openclaw_version(gateway_bin: &str) -> String {
    run_capture(gateway_bin, &["--version"])
        .and_then(|output| {
            output
                .split_whitespace()
                .map(|token| token.trim_matches(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '-')))
                .find(|token| {
                    let mut parts = token.split('.');
                    let first = parts.next().unwrap_or_default();
                    let second = parts.next().unwrap_or_default();
                    first.chars().all(|c| c.is_ascii_digit())
                        && second.chars().all(|c| c.is_ascii_digit())
                })
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn detect_addon_version() -> String {
    env::var("ADDON_VERSION").unwrap_or_else(|_| "unknown".to_string())
}

fn backup_state(args: &HaosEntryArgs) {
    let openclaw_target = args.backup_dir.join(".openclaw");
    let mcporter_target = args.backup_dir.join(".mcporter");
    let _ = run_status(
        "rsync",
        &[
            "-a",
            "--delete",
            &format!("{}/", args.openclaw_config_dir.display()),
            &format!("{}/", openclaw_target.display()),
        ],
    );
    let _ = run_status(
        "rsync",
        &[
            "-a",
            "--delete",
            &format!("{}/", args.mcporter_home_dir.display()),
            &format!("{}/", mcporter_target.display()),
        ],
    );
}

fn run_capture(program: &str, args: &[&str]) -> Option<String> {
    let output = StdCommand::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

fn run_status(program: &str, args: &[&str]) -> bool {
    match StdCommand::new(program).args(args).status() {
        Ok(status) => status.success(),
        Err(err) => {
            eprintln!("addon-supervisor: failed to run {}: {}", program, err);
            false
        }
    }
}

fn command_exists(program: &str) -> bool {
    StdCommand::new(program)
        .arg("--help")
        .output()
        .map(|_| true)
        .unwrap_or(false)
}

fn render_nginx(output: PathBuf) -> ExitCode {
    let terminal_port = env_value("TERMINAL_PORT", "7681");
    let action_port = env_value("ACTION_SERVER_PORT", "48100");
    let ui_port = env_value("UI_PORT", "48101");
    let enable_https = env::var("ENABLE_HTTPS_PROXY")
        .map(|value| value == "true")
        .unwrap_or(false);
    let https_port = env_value("HTTPS_PROXY_PORT", "");
    let internal_gw_port = env_value("GATEWAY_INTERNAL_PORT", "");
    let nginx_log_level = env_value("NGINX_LOG_LEVEL", "minimal");

    let access_log_block = if nginx_log_level == "minimal" {
        r#"# Suppress repetitive HA health-check / polling requests
  map $http_user_agent $loggable {
    ~HomeAssistant 0;
    default 1;
  }
  access_log /dev/stdout combined if=$loggable;"#
            .to_string()
    } else {
        "access_log /dev/stdout;".to_string()
    };

    let https_block = if enable_https && !https_port.is_empty() && !internal_gw_port.is_empty() {
        format!(
            r#"
  server {{
    listen {https_port} ssl;

    ssl_certificate     /config/certs/gateway.crt;
    ssl_certificate_key /config/certs/gateway.key;
    ssl_protocols       TLSv1.2 TLSv1.3;
    ssl_ciphers         HIGH:!aNULL:!MD5;

    location / {{
      proxy_pass http://127.0.0.1:{internal_gw_port};
      proxy_http_version 1.1;
      proxy_set_header Upgrade $http_upgrade;
      proxy_set_header Connection "upgrade";
      proxy_set_header Host $host;
      proxy_set_header X-Real-IP $remote_addr;
      proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
      proxy_set_header X-Forwarded-Proto https;
      proxy_read_timeout 86400s;
      proxy_send_timeout 86400s;
      proxy_buffering off;
    }}

    location = /cert/ca.crt {{
      alias /etc/nginx/html/openclaw-ca.crt;
      default_type application/x-x509-ca-cert;
      add_header Content-Disposition 'attachment; filename="openclaw-ca.crt"';
    }}
  }}
"#
        )
    } else {
        String::new()
    };

    let conf = format!(
        r#"worker_processes 1;

error_log /dev/stderr notice;

events {{ worker_connections 1024; }}

http {{
  include       /etc/nginx/mime.types;
  default_type  application/octet-stream;

  {access_log_block}
  error_log /dev/stderr notice;

  sendfile on;
  keepalive_timeout 65;

  server {{
    listen 48099;

    location = /terminal {{
      return 302 /terminal/;
    }}

    location ^~ /terminal/ {{
      proxy_pass http://127.0.0.1:{terminal_port};
      proxy_http_version 1.1;
      proxy_set_header Upgrade $http_upgrade;
      proxy_set_header Connection "upgrade";
      proxy_set_header Host $host;
      proxy_set_header X-Real-IP $remote_addr;
      proxy_set_header X-Forwarded-For $remote_addr;
      proxy_set_header X-Forwarded-Proto $scheme;
      proxy_read_timeout 3600s;
      proxy_send_timeout 3600s;
    }}

    location = /token {{
      alias /etc/nginx/html/gateway.token;
      default_type text/plain;
      add_header Cache-Control "no-store";
    }}

    location = /openclaw-ca.crt {{
      alias /etc/nginx/html/openclaw-ca.crt;
      default_type application/x-x509-ca-cert;
      add_header Content-Disposition 'attachment; filename="openclaw-ca.crt"';
    }}

    location = /health {{
      proxy_pass http://127.0.0.1:{action_port}/health;
      proxy_http_version 1.1;
      proxy_set_header Host $host;
      proxy_set_header X-Forwarded-Proto $scheme;
      proxy_read_timeout 30s;
    }}

    location ~ ^/action/(status|restart)$ {{
      proxy_pass http://127.0.0.1:{action_port};
      proxy_http_version 1.1;
      proxy_set_header Host $host;
      proxy_set_header X-Forwarded-Proto $scheme;
      proxy_read_timeout 70s;
    }}

    location / {{
      proxy_pass http://127.0.0.1:{ui_port};
      proxy_http_version 1.1;
      proxy_set_header Host $host;
      proxy_set_header X-Forwarded-Proto $scheme;
      proxy_read_timeout 60s;
    }}
  }}
{https_block}
}}
"#
    );

    if let Some(parent) = output.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            eprintln!("failed to create nginx output dir: {err}");
            return ExitCode::from(1);
        }
    }
    if let Err(err) = fs::write(&output, conf) {
        eprintln!("failed to write nginx config: {err}");
        return ExitCode::from(1);
    }
    println!("wrote nginx config to {}", output.display());
    ExitCode::SUCCESS
}

fn env_value(key: &str, fallback: &str) -> String {
    env::var(key).unwrap_or_else(|_| fallback.to_string())
}

fn ensure_runtime_dir() -> std::io::Result<()> {
    fs::create_dir_all("/run/openclaw-rs")
}

fn pid_file_path(name: &str) -> PathBuf {
    Path::new("/run/openclaw-rs").join(format!("{name}.pid"))
}

fn write_pid_file(name: &str, pid: u32) {
    if let Err(err) = ensure_runtime_dir() {
        eprintln!("addon-supervisor: failed to create runtime dir: {err}");
        return;
    }

    let path = pid_file_path(name);
    if let Err(err) = fs::write(&path, pid.to_string()) {
        eprintln!(
            "addon-supervisor: failed to write pid file for {} at {}: {}",
            name,
            path.display(),
            err
        );
    }
}

fn remove_pid_file(name: &str) {
    let path = pid_file_path(name);
    if let Err(err) = fs::remove_file(&path) {
        if err.kind() != std::io::ErrorKind::NotFound {
            eprintln!(
                "addon-supervisor: failed to remove pid file for {} at {}: {}",
                name,
                path.display(),
                err
            );
        }
    }
}

fn run_services(
    gateway_bin: String,
    ui_bin: String,
    action_bin: String,
    ingress_bin: String,
    gateway_mode: String,
    gateway_remote_url: String,
    auto_approve_pairing: bool,
    run_doctor_on_start: bool,
    _terminal_port: u16,
    _enable_terminal: bool,
) -> ExitCode {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(async move {
        if let Err(err) = ensure_runtime_dir() {
            eprintln!("addon-supervisor: failed to initialize runtime dir: {err}");
            return ExitCode::from(1);
        }
        for name in [
            "openclaw-gateway",
            "openclaw-node",
            "haos-ui",
            "actiond",
            "ingressd",
        ] {
            remove_pid_file(name);
        }

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let mut handles = Vec::new();

        let gateway_spec =
            match build_gateway_spec(gateway_bin.clone(), &gateway_mode, &gateway_remote_url) {
                Ok(spec) => spec,
                Err(err) => {
                    eprintln!("addon-supervisor: invalid gateway runtime settings: {err}");
                    return ExitCode::from(1);
                }
            };

        handles.push(tokio::spawn(run_managed_process(
            gateway_spec,
            shutdown_rx.clone(),
        )));
        handles.push(tokio::spawn(run_managed_process(
            ProcessSpec::new("haos-ui", ui_bin, vec![]),
            shutdown_rx.clone(),
        )));
        handles.push(tokio::spawn(run_managed_process(
            ProcessSpec::new("actiond", action_bin, vec![]),
            shutdown_rx.clone(),
        )));

        tokio::time::sleep(Duration::from_millis(800)).await;

        handles.push(tokio::spawn(run_managed_process(
            ProcessSpec::new("ingressd", ingress_bin, vec![]),
            shutdown_rx,
        )));

        if auto_approve_pairing {
            handles.push(tokio::spawn(run_pairing_auto_approver(
                gateway_bin.clone(),
                gateway_mode.clone(),
                shutdown_tx.subscribe(),
            )));
        }

        if run_doctor_on_start {
            handles.push(tokio::spawn(run_startup_doctor(
                gateway_bin.clone(),
                shutdown_tx.subscribe(),
            )));
        }

        println!("addon-supervisor: services started; waiting for Ctrl+C");
        let _ = signal::ctrl_c().await;
        let _ = shutdown_tx.send(true);

        for handle in handles {
            let _ = handle.await;
        }
        ExitCode::SUCCESS
    })
}

fn build_gateway_spec(
    gateway_bin: String,
    gateway_mode: &str,
    gateway_remote_url: &str,
) -> Result<ProcessSpec, String> {
    match gateway_mode {
        "remote" => {
            let parsed = Url::parse(gateway_remote_url)
                .map_err(|err| format!("failed to parse gateway_remote_url: {err}"))?;
            let scheme = parsed.scheme();
            if scheme != "ws" && scheme != "wss" {
                return Err(format!(
                    "gateway_remote_url must use ws:// or wss://, got {scheme}://"
                ));
            }
            let host = parsed
                .host_str()
                .ok_or_else(|| "gateway_remote_url is missing host".to_string())?;
            let port = parsed
                .port_or_known_default()
                .ok_or_else(|| "gateway_remote_url is missing port".to_string())?;

            let mut args = vec![
                "node".to_string(),
                "run".to_string(),
                "--host".to_string(),
                host.to_string(),
                "--port".to_string(),
                port.to_string(),
            ];
            if scheme == "wss" {
                args.push("--tls".to_string());
            }
            Ok(ProcessSpec::new("openclaw-node", gateway_bin, args))
        }
        "local" | "" => Ok(ProcessSpec::new(
            "openclaw-gateway",
            gateway_bin,
            vec!["gateway".to_string(), "run".to_string()],
        )),
        other => Err(format!(
            "unsupported gateway_mode '{other}' (expected local or remote)"
        )),
    }
}

#[derive(Clone)]
struct ProcessSpec {
    name: String,
    program: String,
    args: Vec<String>,
    render_nginx_conf: Option<PathBuf>,
}

impl ProcessSpec {
    fn new(name: &str, program: String, args: Vec<String>) -> Self {
        Self {
            name: name.to_string(),
            program,
            args,
            render_nginx_conf: None,
        }
    }
}

async fn run_managed_process(spec: ProcessSpec, mut shutdown_rx: watch::Receiver<bool>) {
    loop {
        if *shutdown_rx.borrow() {
            break;
        }

        if let Some(path) = spec.render_nginx_conf.clone() {
            let code = render_nginx(path);
            if code != ExitCode::SUCCESS {
                eprintln!("addon-supervisor: failed to render nginx config");
                sleep(Duration::from_secs(2)).await;
                continue;
            }
        }

        let mut command = Command::new(&spec.program);
        apply_child_env(&mut command);
        command.args(&spec.args);

        let Ok(mut child) = command.spawn() else {
            eprintln!("addon-supervisor: failed to start {}", spec.name);
            sleep(Duration::from_secs(2)).await;
            continue;
        };

        let pid = child.id().unwrap_or_default();
        println!("addon-supervisor: started {} (pid {})", spec.name, pid);
        if pid != 0 {
            write_pid_file(&spec.name, pid);
        }

        tokio::select! {
            _ = shutdown_rx.changed() => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                remove_pid_file(&spec.name);
                println!("addon-supervisor: stopped {}", spec.name);
                break;
            }
            status = child.wait() => {
                remove_pid_file(&spec.name);
                match status {
                    Ok(exit) => {
                        eprintln!("addon-supervisor: {} exited with {:?}", spec.name, exit.code());
                    }
                    Err(err) => {
                        eprintln!("addon-supervisor: {} wait failed: {}", spec.name, err);
                    }
                }
                if *shutdown_rx.borrow() {
                    break;
                }
                sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

async fn run_pairing_auto_approver(
    gateway_bin: String,
    gateway_mode: String,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    if gateway_mode == "remote" {
        println!("addon-supervisor: auto-approve pairing skipped in remote mode");
        return;
    }

    sleep(Duration::from_secs(20)).await;
    loop {
        if *shutdown_rx.borrow() {
            break;
        }

        let mut command = Command::new(&gateway_bin);
        apply_child_env(&mut command);
        let mut child = match command.args(["devices", "approve", "--latest"]).spawn() {
            Ok(child) => child,
            Err(err) => {
                eprintln!("addon-supervisor: failed to start auto-approve helper: {err}");
                sleep(Duration::from_secs(6)).await;
                continue;
            }
        };

        tokio::select! {
            _ = shutdown_rx.changed() => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                break;
            }
            result = child.wait() => {
                if let Err(err) = result {
                    eprintln!("addon-supervisor: auto-approve helper failed: {err}");
                }
            }
        }

        sleep(Duration::from_secs(6)).await;
    }
}

async fn run_startup_doctor(gateway_bin: String, mut shutdown_rx: watch::Receiver<bool>) {
    tokio::select! {
        _ = shutdown_rx.changed() => return,
        _ = sleep(Duration::from_secs(15)) => {}
    }

    println!("--- openclaw doctor ---");
    let mut command = Command::new(&gateway_bin);
    apply_child_env(&mut command);
    let mut child = match command.arg("doctor").spawn() {
        Ok(child) => child,
        Err(err) => {
            eprintln!("addon-supervisor: failed to start doctor: {err}");
            println!("--- end doctor ---");
            return;
        }
    };

    tokio::select! {
        _ = shutdown_rx.changed() => {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        result = child.wait() => {
            if let Err(err) = result {
                eprintln!("addon-supervisor: doctor wait failed: {err}");
            }
        }
    }
    println!("--- end doctor ---");
}

fn apply_child_env(command: &mut Command) {
    for key in [
        "HOME",
        "TZ",
        "OPENCLAW_CONFIG_DIR",
        "OPENCLAW_CONFIG_PATH",
        "OPENCLAW_WORKSPACE_DIR",
        "XDG_CONFIG_HOME",
        "OPENCLAW_NO_RESPAWN",
        "MCPORTER_HOME_DIR",
        "MCPORTER_CONFIG",
        "ACTION_SERVER_PORT",
        "UI_PORT",
        "INGRESS_PORT",
        "ACCESS_MODE",
        "GATEWAY_MODE",
        "GW_PUBLIC_URL",
        "HTTPS_PORT",
        "ENABLE_TERMINAL",
        "ENABLE_HTTPS_PROXY",
        "HTTPS_PROXY_PORT",
        "GATEWAY_INTERNAL_PORT",
        "ADDON_VERSION",
        "OPENCLAW_VERSION",
        "MCP_STATUS",
        "WEB_SEARCH_PROVIDER",
        "MEMORY_SEARCH_PROVIDER",
    ] {
        if let Ok(value) = env::var(key) {
            command.env(key, value);
        }
    }
}
