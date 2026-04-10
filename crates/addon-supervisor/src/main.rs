use clap::{Args, Parser, Subcommand};
use rand::random;
use serde::Deserialize;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command as StdCommand, ExitCode, Stdio},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::Command,
    signal,
    sync::watch,
    time::sleep,
};
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

    if let Err(err) = ensure_mcporter_config(&args) {
        eprintln!("addon-supervisor: failed to prepare MCPorter config: {err}");
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
        if configure_home_assistant_mcp(&args.mcporter_bin, &settings.homeassistant_token) {
            mcp_status = "HA configured".to_string();
        } else {
            mcp_status = "HA config failed".to_string();
        }
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
        &PathBuf::from("/var/tmp/openclaw-compile-cache"),
    ] {
        fs::create_dir_all(path)?;
    }
    Ok(())
}

fn ensure_mcporter_config(args: &HaosEntryArgs) -> std::io::Result<()> {
    if args.mcporter_config.exists() {
        return Ok(());
    }

    if let Some(parent) = args.mcporter_config.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&args.mcporter_config, "{\"mcpServers\":{}}\n")
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
        env::set_var("OPENCLAW_STATE_DIR", &args.openclaw_config_dir);
        env::set_var("OPENCLAW_WORKSPACE_DIR", &args.openclaw_workspace_dir);
        env::set_var("OPENCLAW_RUNTIME_DIR", "/run/openclaw-rs");
        env::set_var("XDG_CONFIG_HOME", "/config");
        env::set_var("OPENCLAW_NO_RESPAWN", "1");
        env::set_var("NODE_COMPILE_CACHE", "/var/tmp/openclaw-compile-cache");
        env::set_var("MCPORTER_HOME_DIR", &args.mcporter_home_dir);
        env::set_var("MCPORTER_CONFIG", &args.mcporter_config);
        env::set_var("BACKUP_DIR", &args.backup_dir);
        env::set_var("CERT_DIR", &args.cert_dir);
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
    let applied = run_status(
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
    );
    if !applied {
        return false;
    }

    let allowed_origins = build_control_ui_allowed_origins(settings);
    let allowed_origins_json =
        serde_json::to_string(&allowed_origins).unwrap_or_else(|_| "[]".to_string());

    run_status(
        &args.oc_config_bin,
        &[
            "set",
            "gateway.controlUi.allowedOrigins",
            &allowed_origins_json,
            "--json",
        ],
    ) && run_status(
        &args.oc_config_bin,
        &[
            "set",
            "gateway.controlUi.allowInsecureAuth",
            "false",
            "--json",
        ],
    ) && run_status(
        &args.oc_config_bin,
        &[
            "set",
            "gateway.controlUi.dangerouslyDisableDeviceAuth",
            "false",
            "--json",
        ],
    )
}

fn build_control_ui_allowed_origins(settings: &RuntimeSettings) -> Vec<String> {
    let mut origins = Vec::<String>::new();
    let https_port = settings.https_port;

    if settings.access_mode == "lan_https" {
        for ip in detect_lan_ips() {
            origins.push(format!("https://{ip}:{https_port}"));
        }
        origins.push(format!("https://localhost:{https_port}"));
        origins.push(format!("https://127.0.0.1:{https_port}"));
        origins.push(format!("https://homeassistant.local:{https_port}"));
        origins.push(format!("https://homeassistant:{https_port}"));
    }

    if !settings.gateway_public_url.trim().is_empty()
        && let Ok(parsed) = Url::parse(settings.gateway_public_url.trim())
        && let Some(host) = parsed.host_str()
    {
        let origin = if let Some(port) = parsed.port() {
            format!("{}://{}:{}", parsed.scheme(), host, port)
        } else {
            format!("{}://{}", parsed.scheme(), host)
        };
        origins.push(origin);
    }

    origins.sort();
    origins.dedup();
    origins
}

fn detect_lan_ips() -> Vec<String> {
    let mut ips = detect_lan_ips_from_ip_command();
    if ips.is_empty()
        && let Some(ip) = detect_lan_ip()
    {
        ips.push(ip);
    }
    ips.sort();
    ips.dedup();
    ips
}

fn detect_lan_ips_from_ip_command() -> Vec<String> {
    let output = match StdCommand::new("ip")
        .args(["-o", "-4", "addr", "show", "up", "scope", "global"])
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return Vec::new(),
    };

    parse_ipv4_addrs_from_ip_addr_output(&String::from_utf8_lossy(&output.stdout))
}

fn parse_ipv4_addrs_from_ip_addr_output(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            while let Some(part) = parts.next() {
                if part == "inet" {
                    let cidr = parts.next()?;
                    return cidr.split('/').next().map(|ip| ip.to_string());
                }
            }
            None
        })
        .collect()
}

fn detect_lan_ip() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let local = socket.local_addr().ok()?;
    Some(local.ip().to_string())
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
    if !gateway_key.exists()
        || !gateway_crt.exists()
        || certificate_needs_regeneration(&gateway_crt)
    {
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
    let _ = set_mode_600(&ca_target);
    let _ = set_mode_600(&args.openclaw_config_path);
    true
}

fn certificate_renew_before_seconds() -> u64 {
    30 * 24 * 60 * 60
}

fn certificate_needs_regeneration(cert_path: &Path) -> bool {
    let renew_before = certificate_renew_before_seconds().to_string();
    let status = StdCommand::new("openssl")
        .args(["x509", "-checkend", &renew_before, "-noout", "-in"])
        .arg(cert_path)
        .status();

    match status {
        Ok(status) => !status.success(),
        Err(_) => true,
    }
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
                .map(|token| {
                    token.trim_matches(|c: char| {
                        !(c.is_ascii_alphanumeric() || c == '.' || c == '-')
                    })
                })
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

fn configure_home_assistant_mcp(mcporter_bin: &str, homeassistant_token: &str) -> bool {
    let header = format!("Authorization: Bearer {}", homeassistant_token);
    let modern_args = [
        "config",
        "add",
        "HA",
        "--http-url",
        "http://supervisor/core/api/mcp",
        "--header",
        header.as_str(),
    ];
    if run_status(mcporter_bin, &modern_args) {
        return true;
    }

    let legacy_args = [
        "add",
        "HA",
        "--http-url",
        "http://supervisor/core/api/mcp",
        "--header",
        header.as_str(),
    ];
    if run_status(mcporter_bin, &legacy_args) {
        return true;
    }

    eprintln!(
        "addon-supervisor: failed to configure Home Assistant MCP server with current and legacy mcporter commands"
    );
    false
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
    // Exponential backoff: 2s → 4s → 8s → 16s → 32s → 64s (max).
    // Resets to 2s after the process has been alive for at least this many
    // seconds, meaning a successful long-running run clears the failure count.
    const STABLE_SECS: u64 = 30;
    const BACKOFF_BASE: u64 = 2;
    const BACKOFF_MAX: u64 = 64;
    let mut consecutive_failures: u32 = 0;

    loop {
        if *shutdown_rx.borrow() {
            break;
        }

        if let Some(path) = spec.render_nginx_conf.clone() {
            let code = render_nginx(path);
            if code != ExitCode::SUCCESS {
                eprintln!("addon-supervisor: failed to render nginx config");
                sleep(Duration::from_secs(BACKOFF_BASE)).await;
                continue;
            }
        }

        let mut command = Command::new(&spec.program);
        apply_child_env(&mut command);
        command.args(&spec.args);

        let Ok(mut child) = command.spawn() else {
            eprintln!("addon-supervisor: failed to start {}", spec.name);
            sleep(Duration::from_secs(BACKOFF_BASE)).await;
            continue;
        };

        let pid = child.id().unwrap_or_default();
        println!("addon-supervisor: started {} (pid {})", spec.name, pid);
        if pid != 0 {
            write_pid_file(&spec.name, pid);
        }

        let started_at = std::time::Instant::now();

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
                let lived_secs = started_at.elapsed().as_secs();
                match status {
                    Ok(exit) => {
                        eprintln!("addon-supervisor: {} exited with {:?} (lived {}s)", spec.name, exit.code(), lived_secs);
                    }
                    Err(err) => {
                        eprintln!("addon-supervisor: {} wait failed: {} (lived {}s)", spec.name, err, lived_secs);
                    }
                }
                if *shutdown_rx.borrow() {
                    break;
                }
                if lived_secs >= STABLE_SECS {
                    consecutive_failures = 0;
                } else {
                    consecutive_failures = consecutive_failures.saturating_add(1);
                }
                let delay = (BACKOFF_BASE << consecutive_failures.min(5)).min(BACKOFF_MAX);
                if consecutive_failures > 1 {
                    eprintln!(
                        "addon-supervisor: {} backing off {}s (failure #{})",
                        spec.name, delay, consecutive_failures
                    );
                }
                sleep(Duration::from_secs(delay)).await;
            }
        }
    }
}

async fn run_startup_doctor(gateway_bin: String, mut shutdown_rx: watch::Receiver<bool>) {
    tokio::select! {
        _ = shutdown_rx.changed() => return,
        _ = sleep(Duration::from_secs(15)) => {}
    }

    println!("--- openclaw doctor --fix ---");
    let mut command = Command::new(&gateway_bin);
    apply_child_env(&mut command);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match command.args(startup_doctor_args()).spawn() {
        Ok(child) => child,
        Err(err) => {
            eprintln!("addon-supervisor: failed to start doctor: {err}");
            println!("--- end doctor ---");
            return;
        }
    };
    let stdout_task = child
        .stdout
        .take()
        .map(|stdout| tokio::spawn(stream_startup_doctor_output(stdout, false)));
    let stderr_task = child
        .stderr
        .take()
        .map(|stderr| tokio::spawn(stream_startup_doctor_output(stderr, true)));

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
    if let Some(task) = stdout_task {
        let _ = task.await;
    }
    if let Some(task) = stderr_task {
        let _ = task.await;
    }
    println!("--- end doctor ---");
}

fn startup_doctor_args() -> [&'static str; 2] {
    ["doctor", "--fix"]
}

async fn stream_startup_doctor_output<R>(reader: R, is_stderr: bool)
where
    R: AsyncRead + Unpin,
{
    let mut lines = BufReader::new(reader).lines();
    let mut suppress_health_details = false;
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                if should_suppress_startup_doctor_line(&line, &mut suppress_health_details) {
                    continue;
                }
                if is_stderr {
                    eprintln!("{line}");
                } else {
                    println!("{line}");
                }
            }
            Ok(None) => break,
            Err(err) => {
                eprintln!("addon-supervisor: failed to read startup doctor output: {err}");
                break;
            }
        }
    }
}

fn should_suppress_startup_doctor_line(line: &str, suppress_health_details: &mut bool) -> bool {
    let trimmed = line.trim();

    if *suppress_health_details {
        if is_startup_doctor_health_detail_line(trimmed) {
            return true;
        }
        *suppress_health_details = false;
    }

    if trimmed.starts_with("Health check failed: Error: gateway timeout after ") {
        *suppress_health_details = true;
        return true;
    }

    matches!(
        trimmed,
        "Memory search is enabled, but no embedding provider is ready."
            | "Semantic recall needs at least one embedding provider."
            | "systemd user services are unavailable; install/enable systemd or run the gateway under your supervisor."
            | "If you're in a container, run the gateway in the foreground instead of `openclaw gateway`."
            | "Port 18790 is already in use."
            | "Gateway already running locally. Stop it (openclaw gateway stop) or use a different port."
    )
}

fn is_startup_doctor_health_detail_line(line: &str) -> bool {
    line.starts_with("Gateway target:")
        || line.starts_with("Source:")
        || line.starts_with("Config:")
        || line.starts_with("Bind:")
}

fn apply_child_env(command: &mut Command) {
    for key in [
        "HOME",
        "TZ",
        "OPENCLAW_CONFIG_DIR",
        "OPENCLAW_CONFIG_PATH",
        "OPENCLAW_STATE_DIR",
        "OPENCLAW_WORKSPACE_DIR",
        "OPENCLAW_RUNTIME_DIR",
        "XDG_CONFIG_HOME",
        "OPENCLAW_NO_RESPAWN",
        "NODE_COMPILE_CACHE",
        "MCPORTER_HOME_DIR",
        "MCPORTER_CONFIG",
        "BACKUP_DIR",
        "CERT_DIR",
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn sample_settings() -> RuntimeSettings {
        RuntimeSettings {
            timezone: "Asia/Shanghai".to_string(),
            enable_terminal: true,
            terminal_port: 7681,
            gateway_mode: "gateway".to_string(),
            gateway_remote_url: String::new(),
            gateway_public_url: String::new(),
            https_port: 9443,
            access_mode: "lan_https".to_string(),
            enable_openai_api: false,
            auto_configure_mcp: true,
            homeassistant_token: "token".to_string(),
            run_doctor_on_start: false,
        }
    }

    #[test]
    fn allowed_origins_include_expected_lan_and_public_hosts() {
        let mut settings = sample_settings();
        settings.gateway_public_url = "https://gateway.example.com/ui?x=1".to_string();

        let origins = build_control_ui_allowed_origins(&settings);

        assert!(origins.contains(&"https://gateway.example.com".to_string()));
        assert!(origins.contains(&"https://homeassistant.local:9443".to_string()));
        assert!(origins.contains(&"https://homeassistant:9443".to_string()));

        let unique_count = origins.iter().collect::<HashSet<_>>().len();
        assert_eq!(origins.len(), unique_count);

        let mut sorted = origins.clone();
        sorted.sort();
        assert_eq!(origins, sorted);
    }

    #[test]
    fn allowed_origins_ignore_invalid_public_url_without_lan_mode() {
        let mut settings = sample_settings();
        settings.access_mode = "remote_https".to_string();
        settings.gateway_public_url = "not-a-url".to_string();

        let origins = build_control_ui_allowed_origins(&settings);

        assert!(origins.is_empty());
    }

    #[test]
    fn startup_doctor_runs_in_fix_mode() {
        assert_eq!(startup_doctor_args(), ["doctor", "--fix"]);
    }

    #[test]
    fn startup_doctor_suppresses_health_timeout_block() {
        let mut suppress_health_details = false;

        assert!(should_suppress_startup_doctor_line(
            "Health check failed: Error: gateway timeout after 10000ms",
            &mut suppress_health_details
        ));
        assert!(suppress_health_details);
        assert!(should_suppress_startup_doctor_line(
            "Gateway target: ws://127.0.0.1:18790",
            &mut suppress_health_details
        ));
        assert!(should_suppress_startup_doctor_line(
            "Source: local loopback",
            &mut suppress_health_details
        ));
        assert!(should_suppress_startup_doctor_line(
            "Config: /config/.openclaw/openclaw.json",
            &mut suppress_health_details
        ));
        assert!(should_suppress_startup_doctor_line(
            "Bind: loopback",
            &mut suppress_health_details
        ));
        assert!(!should_suppress_startup_doctor_line(
            "Doctor complete.",
            &mut suppress_health_details
        ));
        assert!(!suppress_health_details);
    }

    #[test]
    fn startup_doctor_suppresses_other_known_noise_lines() {
        let mut suppress_health_details = false;

        for line in [
            "Memory search is enabled, but no embedding provider is ready.",
            "Semantic recall needs at least one embedding provider.",
            "systemd user services are unavailable; install/enable systemd or run the gateway under your supervisor.",
            "If you're in a container, run the gateway in the foreground instead of `openclaw gateway`.",
            "Port 18790 is already in use.",
            "Gateway already running locally. Stop it (openclaw gateway stop) or use a different port.",
        ] {
            assert!(should_suppress_startup_doctor_line(
                line,
                &mut suppress_health_details
            ));
        }
    }

    #[test]
    fn parse_ipv4_addrs_extracts_all_global_addresses() {
        let output = "\
2: end0    inet 192.168.1.122/24 brd 192.168.1.255 scope global dynamic end0\n\
3: wlan0   inet 10.0.0.8/24 brd 10.0.0.255 scope global wlan0\n";

        let ips = parse_ipv4_addrs_from_ip_addr_output(output);

        assert_eq!(
            ips,
            vec!["192.168.1.122".to_string(), "10.0.0.8".to_string()]
        );
    }

    #[test]
    fn certificate_renew_window_is_thirty_days() {
        assert_eq!(certificate_renew_before_seconds(), 2_592_000);
    }

    #[test]
    fn ensure_mcporter_config_creates_seed_file() {
        let unique = format!("openclaw-test-{}", random::<u64>());
        let root = std::env::temp_dir().join(unique);
        let args = HaosEntryArgs {
            options_file: root.join("options.json"),
            openclaw_config_dir: root.join(".openclaw"),
            openclaw_config_path: root.join(".openclaw").join("openclaw.json"),
            openclaw_workspace_dir: root.join(".openclaw").join("workspace"),
            mcporter_home_dir: root.join(".mcporter"),
            mcporter_config: root.join(".mcporter").join("mcporter.json"),
            cert_dir: root.join("certs"),
            backup_dir: root.join("backup"),
            nginx_html_dir: root.join("html"),
            gateway_internal_port: 18790,
            action_server_port: 48100,
            ui_port: 48101,
            gateway_bin: "openclaw".to_string(),
            oc_config_bin: "oc-config".to_string(),
            mcporter_bin: "mcporter".to_string(),
            ui_bin: "haos-ui".to_string(),
            action_bin: "actiond".to_string(),
            ingress_bin: "ingressd".to_string(),
            nginx_conf: root.join("nginx.conf"),
        };

        ensure_mcporter_config(&args).expect("seed mcporter config");

        let contents = fs::read_to_string(&args.mcporter_config).expect("mcporter config");
        assert_eq!(contents, "{\"mcpServers\":{}}\n");

        let _ = fs::remove_dir_all(root);
    }

}
