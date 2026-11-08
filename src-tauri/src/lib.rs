use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::Manager;
#[cfg(not(target_os = "android"))]
use tauri_plugin_shell::ShellExt;

mod go_client;
mod mihomo;
#[cfg(any(windows, test))]
mod sysproxy;
#[cfg(any(target_os = "macos", all(test, target_os = "linux")))]
mod system_dns;
#[cfg(any(target_os = "macos", all(test, target_os = "linux")))]
mod tun_helper;

use go_client::{GoClientConfig, GoClientManager};
use mihomo::MihomoManager;

include!(concat!(env!("OUT_DIR"), "/sidecar_hashes.rs"));

#[cfg(not(target_os = "android"))]
fn sidecar_sha256(path: &std::path::Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).ok()?;
    let mut h = Sha256::new();
    h.update(&bytes);
    Some(format!("{:x}", h.finalize()))
}

#[cfg(not(target_os = "android"))]
fn verify_sidecar(path: &std::path::Path) -> Result<(), String> {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let expected = SIDECAR_HASHES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, h)| *h);
    let Some(expected) = expected else {
        eprintln!(
            "[sidecar] no baked hash for {} (skipping verification)",
            name
        );
        return Ok(());
    };
    let actual = sidecar_sha256(path).ok_or_else(|| format!("sidecar {} not found", name))?;
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(format!(
            "sidecar {} hash mismatch (expected {}, got {})",
            name, expected, actual
        ));
    }
    eprintln!("[sidecar] {} hash OK", name);
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RoutingRule {
    id: String,
    kind: String,
    value: String,
    action: String,
}

fn build_client_rules_json(settings: &AppSettings) -> String {
    let mut entries: Vec<serde_json::Value> = Vec::new();
    let push = |dst: &mut Vec<serde_json::Value>, r: &RoutingRule, force_action: Option<&str>| {
        let action = force_action.unwrap_or(&r.action).to_uppercase();
        let mapped = match action.as_str() {
            "DIRECT" => "DIRECT",
            "PROXY" => "PROXY",
            "REJECT" | "BLOCK" => "REJECT",
            _ => "DIRECT",
        };
        let kind = match r.kind.as_str() {
            "domain-keyword" => "domain-keyword",
            "domain-full" => "domain-exact",
            "ip" => "ip-cidr",
            "process" => "process-name",
            _ => "domain-suffix",
        };
        let val_field = match kind {
            "domain-suffix" => ("suffix", r.value.clone()),
            "domain-keyword" => ("keyword", r.value.clone()),
            "domain-exact" => ("domain", r.value.clone()),
            "ip-cidr" => {
                let v = if r.value.contains('/') {
                    r.value.clone()
                } else {
                    format!("{}/32", r.value)
                };
                ("cidr", v)
            }
            "process-name" => ("name", r.value.clone()),
            _ => ("suffix", r.value.clone()),
        };
        let mut obj = serde_json::Map::new();
        obj.insert(
            "kind".to_string(),
            serde_json::Value::String(kind.to_string()),
        );
        obj.insert(
            val_field.0.to_string(),
            serde_json::Value::String(val_field.1),
        );
        obj.insert(
            "action".to_string(),
            serde_json::Value::String(mapped.to_string()),
        );
        dst.push(serde_json::Value::Object(obj));
    };
    if settings.bypass_ru {
        for suffix in &["ru", "su", "рф"] {
            let mut obj = serde_json::Map::new();
            obj.insert("kind".into(), "domain-suffix".into());
            obj.insert("suffix".into(), (*suffix).into());
            obj.insert("action".into(), "DIRECT".into());
            entries.push(serde_json::Value::Object(obj));
        }
        let mut geo = serde_json::Map::new();
        geo.insert("kind".into(), "geoip".into());
        geo.insert("action".into(), "DIRECT".into());
        entries.push(serde_json::Value::Object(geo));
    }
    for r in &settings.routing_rules {
        push(&mut entries, r, None);
    }
    for r in &settings.blocklist {
        push(&mut entries, r, Some("REJECT"));
    }
    serde_json::to_string(&entries).unwrap_or_else(|_| "[]".to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppSettings {
    conn_key: String,
    auto_connect: bool,
    theme: String,
    mihomo_port: u16,
    socks_addr: String,
    kill_switch: bool,
    #[serde(default)]
    dns_redirect: bool,
    #[serde(default = "default_true")]
    ipv6: bool,
    #[serde(default = "default_true")]
    ipv6_fallback: bool,
    #[serde(default)]
    ipv6_only: bool,
    #[serde(default = "default_tun")]
    tun_stack: String,
    #[serde(default = "default_true")]
    hwid: bool,
    #[serde(default = "default_true")]
    auth_tip: bool,
    #[serde(default)]
    secret: String,
    #[serde(default)]
    routing_rules: Vec<RoutingRule>,
    #[serde(default)]
    blocklist: Vec<RoutingRule>,
    #[serde(default)]
    custom_dns: Vec<String>,
    #[serde(default)]
    vpn_dns: String,
    #[serde(default)]
    tls_fingerprint: String,
    #[serde(default)]
    external_link: String,
    #[serde(default = "default_true")]
    bypass_ru: bool,
    #[serde(default)]
    socks_user: String,
    #[serde(default)]
    socks_pass: String,
    #[serde(default)]
    allow_lan: bool,
    #[serde(default)]
    log_level: String,
    #[serde(default)]
    routing_mode: String,
    #[serde(default = "default_dns_mode")]
    dns_mode: String,
    #[serde(default = "default_dns_strategy")]
    dns_strategy: String,
    #[serde(default = "default_mtu")]
    mtu: u16,
    #[serde(default)]
    tls_fragment: bool,
    #[serde(default = "default_true")]
    sub_auto_update: bool,
    #[serde(default)]
    quic: bool,
    #[serde(default = "default_ping_mode")]
    ping_mode: String,
    #[serde(default = "default_tun_android")]
    tun_stack_android: String,
    #[serde(default = "default_proxy_mode")]
    proxy_mode: String,
}

const PROXY_MODE_TUN: &str = "tun";
const PROXY_MODE_SYSTEM: &str = "system";
#[cfg(windows)]
const SYSTEM_PROXY_BACKUP: &str = "system-proxy-backup.json";

#[cfg(target_os = "android")]
fn server_ipv6_mode(fallback: bool, only: bool) -> &'static str {
    match (only, fallback) {
        (true, _) => "only",
        (false, true) => "auto",
        (false, false) => "off",
    }
}

fn default_true() -> bool {
    true
}
fn default_proxy_mode() -> String {
    PROXY_MODE_TUN.to_string()
}

fn tun_enabled(settings: &AppSettings) -> bool {
    !cfg!(windows) || settings.proxy_mode != PROXY_MODE_SYSTEM
}

#[cfg(windows)]
fn system_proxy_backup(app: &tauri::AppHandle) -> PathBuf {
    mihomo_config_path(app).with_file_name(SYSTEM_PROXY_BACKUP)
}

fn apply_proxy_mode(app: &tauri::AppHandle, settings: &AppSettings) -> Result<(), String> {
    #[cfg(windows)]
    if !tun_enabled(settings) {
        let server = format!("127.0.0.1:{}", settings.mihomo_port);
        return sysproxy::apply(
            &sysproxy::InternetOptions,
            &system_proxy_backup(app),
            &sysproxy::manual(&server),
        );
    }
    #[cfg(not(windows))]
    let _ = settings;
    restore_system_proxy(app);
    Ok(())
}

fn restore_system_proxy(app: &tauri::AppHandle) {
    #[cfg(windows)]
    if let Err(e) = sysproxy::restore(&sysproxy::InternetOptions, &system_proxy_backup(app)) {
        eprintln!("[system proxy] {e}");
    }
    #[cfg(not(windows))]
    let _ = app;
}
fn default_tun() -> String {
    "Mixed".to_string()
}
fn default_tun_android() -> String {
    "gvisor".to_string()
}
fn default_ping_mode() -> String {
    "tcp".to_string()
}
fn default_dns_mode() -> String {
    "tcp".to_string()
}
fn default_dns_strategy() -> String {
    "fakeip".to_string()
}
fn default_mtu() -> u16 {
    1500
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            conn_key: String::new(),
            auto_connect: false,
            theme: "dark".to_string(),
            mihomo_port: 9887,
            socks_addr: "127.0.0.1:1081".to_string(),
            kill_switch: false,
            dns_redirect: false,
            ipv6: true,
            ipv6_fallback: true,
            ipv6_only: false,
            tun_stack: "Mixed".to_string(),
            hwid: true,
            auth_tip: true,
            secret: String::new(),
            routing_rules: Vec::new(),
            blocklist: Vec::new(),
            custom_dns: Vec::new(),
            vpn_dns: String::new(),
            tls_fingerprint: String::new(),
            external_link: String::new(),
            bypass_ru: true,
            quic: false,
            ping_mode: "tcp".to_string(),
            tun_stack_android: "gvisor".to_string(),
            socks_user: String::new(),
            socks_pass: String::new(),
            allow_lan: false,
            log_level: String::new(),
            routing_mode: String::new(),
            dns_mode: "tcp".to_string(),
            dns_strategy: "fakeip".to_string(),
            mtu: 1500,
            tls_fragment: false,
            sub_auto_update: true,
            proxy_mode: default_proxy_mode(),
        }
    }
}

struct AppState {
    mihomo: Mutex<MihomoManager>,
    go_client: Mutex<GoClientManager>,
    external_engine: std::sync::atomic::AtomicBool,
}

fn settings_path(app: &tauri::AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    fs::create_dir_all(&dir).ok();
    dir.join("settings.json")
}

fn mihomo_config_path(app: &tauri::AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    fs::create_dir_all(&dir).ok();
    dir.join("mihomo_config.yaml")
}

#[tauri::command]
fn get_app_settings(app: tauri::AppHandle) -> Result<AppSettings, String> {
    let path = settings_path(&app);
    if path.exists() {
        let data = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let settings: AppSettings = serde_json::from_str(&data).map_err(|e| e.to_string())?;
        Ok(settings)
    } else {
        Ok(AppSettings::default())
    }
}

#[tauri::command]
fn save_app_setting(app: tauri::AppHandle, mut settings: AppSettings) -> Result<(), String> {
    let path = settings_path(&app);
    if path.exists() {
        if let Ok(raw) = fs::read_to_string(&path) {
            if let Ok(existing) = serde_json::from_str::<AppSettings>(&raw) {
                settings.routing_rules = existing.routing_rules;
                settings.blocklist = existing.blocklist;
                if settings.secret.is_empty() {
                    settings.secret = existing.secret;
                }
            }
        }
    }
    #[cfg(windows)]
    let enabling_autostart = settings.auto_connect
        && !fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<AppSettings>(&raw).ok())
            .is_some_and(|existing| existing.auto_connect);
    let data = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    fs::write(&path, data).map_err(|e| e.to_string())?;
    #[cfg(desktop)]
    sync_autostart(&app, settings.auto_connect)?;
    #[cfg(windows)]
    if enabling_autostart {
        install_tunnel_service_in_background(&app);
    }
    #[cfg(target_os = "android")]
    {
        let enabled = settings.auto_connect;
        std::panic::catch_unwind(move || whisp_vpn_android::service_intent::set_autostart(enabled))
            .map_err(|_| "panic in set_autostart".to_string())??;
    }
    Ok(())
}

#[cfg(windows)]
fn install_tunnel_service_in_background(app: &tauri::AppHandle) {
    let app = app.clone();
    let config_path = mihomo_config_path(&app);
    tauri::async_runtime::spawn_blocking(move || {
        let state: tauri::State<AppState> = app.state();
        let installed = match state.mihomo.lock() {
            Ok(mgr) => mgr.ensure_persistent_service(&config_path),
            Err(e) => Err(e.to_string()),
        };
        if let Err(e) = installed {
            eprintln!("[autostart] tunnel service: {e}");
        }
    });
}

#[cfg(desktop)]
fn sync_autostart(app: &tauri::AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let launcher = app.autolaunch();
    if launcher.is_enabled().map_err(|e| e.to_string())? == enabled {
        return Ok(());
    }
    if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    }
    .map_err(|e| format!("autostart: {e}"))
}

#[cfg(desktop)]
const CONTROLLER_SECRET_BYTES: usize = 16;

#[cfg(desktop)]
fn ensure_controller_secret(app: &tauri::AppHandle) -> Result<(), String> {
    let mut settings = get_app_settings(app.clone())?;
    if !settings.secret.is_empty() {
        return Ok(());
    }
    let mut raw = [0u8; CONTROLLER_SECRET_BYTES];
    getrandom::fill(&mut raw).map_err(|e| e.to_string())?;
    settings.secret = raw.iter().map(|b| format!("{b:02x}")).collect();
    let data = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    fs::write(settings_path(app), data).map_err(|e| e.to_string())
}

async fn wait_go_client_ready(
    state: &tauri::State<'_, AppState>,
    log_mark: u64,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        let alive = state
            .go_client
            .lock()
            .map_err(|e| e.to_string())?
            .is_running();
        if !alive {
            let reason = go_client::startup_failure(log_mark);
            eprintln!("[connect] {}", reason);
            return Err(reason);
        }
        if go_client::control_reachable() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Ok(())
}

#[tauri::command]
async fn connect(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    #[cfg(target_os = "android")]
    {
        let settings = get_app_settings(app.clone())?;
        let rules_json = build_client_rules_json(&settings);
        let conn_key = settings.conn_key.clone();
        let vpn_dns = settings
            .custom_dns
            .iter()
            .map(|s| s.trim())
            .find(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| settings.vpn_dns.clone());
        let ipv6 = settings.ipv6;
        let hwid = settings.hwid;
        let tls_fingerprint = settings.tls_fingerprint.clone();
        let mixed_port = settings.mihomo_port;
        let allow_lan = settings.allow_lan;
        let socks_user = settings.socks_user.clone();
        let socks_pass = settings.socks_pass.clone();
        let dns_mode = settings.dns_mode.clone();
        let dns_strategy = settings.dns_strategy.clone();
        let mtu = settings.mtu;
        let tls_fragment = settings.tls_fragment;
        let auto_connect = settings.auto_connect;
        let tun_stack = settings.tun_stack_android.to_lowercase();
        let quic = settings.quic;
        let server_ipv6 = server_ipv6_mode(settings.ipv6_fallback, settings.ipv6_only);

        let prepared = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            whisp_vpn_android::service_intent::is_vpn_prepared,
        ))
        .unwrap_or(Ok(false))
        .unwrap_or(false);

        if !prepared {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                whisp_vpn_android::service_intent::save_pending_start(
                    &rules_json,
                    &conn_key,
                    &vpn_dns,
                    ipv6,
                    hwid,
                    &tls_fingerprint,
                    mixed_port,
                    allow_lan,
                    &socks_user,
                    &socks_pass,
                    &dns_mode,
                    &dns_strategy,
                    mtu,
                    tls_fragment,
                    auto_connect,
                    &tun_stack,
                    quic,
                    server_ipv6,
                )
            }));
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                whisp_vpn_android::service_intent::request_vpn_permission,
            ));
            return Ok("Android VPN starting".to_string());
        }

        let res = tokio::task::spawn_blocking(move || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                whisp_vpn_android::service_intent::start_vpn_service(
                    &rules_json,
                    &conn_key,
                    &vpn_dns,
                    ipv6,
                    hwid,
                    &tls_fingerprint,
                    mixed_port,
                    allow_lan,
                    &socks_user,
                    &socks_pass,
                    &dns_mode,
                    &dns_strategy,
                    mtu,
                    tls_fragment,
                    auto_connect,
                    &tun_stack,
                    quic,
                    server_ipv6,
                )
            }))
        })
        .await
        .map_err(|e| format!("spawn_blocking: {}", e))?;
        match res {
            Ok(Ok(())) => return Ok("Android VPN starting".to_string()),
            Ok(Err(e)) => return Err(format!("start_vpn_service: {}", e)),
            Err(p) => {
                let msg = if let Some(s) = p.downcast_ref::<&str>() {
                    *s
                } else if let Some(s) = p.downcast_ref::<String>() {
                    s.as_str()
                } else {
                    "unknown panic"
                };
                return Err(format!("panic in start_vpn_service: {}", msg));
            }
        }
    }

    #[allow(unreachable_code)]
    let settings = get_app_settings(app.clone())?;

    if settings.conn_key.is_empty() {
        return Err("Connection key is required (whispera://...)".to_string());
    }

    let socks_addr = if settings.socks_addr.contains(':') {
        settings.socks_addr.clone()
    } else {
        format!("{}:1080", settings.socks_addr)
    };

    // With an external profile the routing engine dials the remote itself,
    // so our own tunnel process is not started at all.
    let external = !settings.external_link.trim().is_empty();
    if external && mihomo::external_proxy_yaml(&settings.external_link, "whisp-server").is_none() {
        return Err("external profile could not be parsed: fix or clear it in settings".into());
    }
    state
        .external_engine
        .store(external, std::sync::atomic::Ordering::Relaxed);
    let started = {
        let mut gc = state.go_client.lock().map_err(|e| e.to_string())?;
        if external {
            gc.stop().ok();
            eprintln!("[connect] external profile active, go-client not started");
            None
        } else {
            eprintln!(
                "[connect] starting go-client, socks={}, key_len={}",
                socks_addr,
                settings.conn_key.len()
            );
            let log_mark = go_client::log_len();
            gc.start(&GoClientConfig {
                conn_key: &settings.conn_key,
                server_addr: "",
                socks_addr: &socks_addr,
                transport: "",
                vpn_dns: &settings.vpn_dns,
                hwid: settings.hwid,
                tls_fingerprint: &settings.tls_fingerprint,
                tls_fragment: settings.tls_fragment,
                ipv6_fallback: settings.ipv6_fallback,
                ipv6_only: settings.ipv6_only,
                split_rules: &build_client_rules_json(&settings),
            })
            .map_err(|e| {
                eprintln!("[connect] go-client start FAILED: {}", e);
                e
            })?;
            eprintln!("[connect] go-client started OK");
            Some(log_mark)
        }
    };
    if let Some(log_mark) = started {
        wait_go_client_ready(&state, log_mark).await?;
    }

    let config_path = mihomo_config_path(&app);
    let mut routing_rules: Vec<mihomo::MihomoRoutingRule> = Vec::new();
    for r in &settings.blocklist {
        routing_rules.push(mihomo::MihomoRoutingRule {
            kind: r.kind.clone(),
            value: r.value.clone(),
            action: "REJECT".to_string(),
        });
    }
    for r in &settings.routing_rules {
        routing_rules.push(mihomo::MihomoRoutingRule {
            kind: r.kind.clone(),
            value: r.value.clone(),
            action: r.action.clone(),
        });
    }

    let mihomo_config = mihomo::generate_config(&mihomo::MihomoConfig {
        socks_addr: &socks_addr,
        server_host: &server_host_from_key(&settings.conn_key),
        mixed_port: settings.mihomo_port,
        tun: tun_enabled(&settings),
        tun_stack: &settings.tun_stack,
        dns_redirect: settings.dns_redirect,
        ipv6: settings.ipv6,
        routing_rules: &routing_rules,
        extra_socks_addrs: &[],
        custom_dns: &settings.custom_dns,
        socks_user: &settings.socks_user,
        socks_pass: &settings.socks_pass,
        allow_lan: settings.allow_lan,
        log_level: &settings.log_level,
        routing_mode: &settings.routing_mode,
        bypass_ru: settings.bypass_ru,
        external_link: &settings.external_link,
        secret: &settings.secret,
        kill_switch: settings.kill_switch,
        vpn_dns: &settings.vpn_dns,
    });
    fs::write(&config_path, &mihomo_config).map_err(|e| e.to_string())?;

    let mut mgr = state.mihomo.lock().map_err(|e| e.to_string())?;
    mgr.start(&config_path, tun_enabled(&settings))?;
    apply_proxy_mode(&app, &settings)?;

    Ok(format!(
        "Connected via key (socks5: {}) | mihomo port {}",
        settings.socks_addr, settings.mihomo_port
    ))
}

#[tauri::command]
async fn disconnect(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    #[cfg(target_os = "android")]
    {
        let _ = &app;
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            whisp_vpn_android::service_intent::stop_vpn_service,
        ));
        match res {
            Ok(Ok(())) => return Ok("Android VPN stopping".to_string()),
            Ok(Err(e)) => return Err(format!("stop_vpn_service: {}", e)),
            Err(_) => return Err("panic in stop_vpn_service".to_string()),
        }
    }

    #[allow(unreachable_code)]
    {
        restore_system_proxy(&app);
        let mut mihomo = state.mihomo.lock().map_err(|e| e.to_string())?;
        mihomo.stop_tun()?;

        let mut gc = state.go_client.lock().map_err(|e| e.to_string())?;
        gc.stop()?;

        Ok("Disconnected".to_string())
    }
}

static LAST_STATUS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[tauri::command]
fn get_status(state: tauri::State<AppState>) -> Result<bool, String> {
    #[cfg(target_os = "android")]
    {
        let _ = state;
        let active = whisp_vpn_android::service_intent::is_vpn_active()
            || whisp_vpn_android::service_intent::is_vpn_service_running();
        if active {
            whisp_vpn_android::service_intent::set_vpn_active(true);
        }
        return Ok(active);
    }
    #[allow(unreachable_code)]
    {
        // Connect and disconnect hold these locks, sometimes across a UAC
        // prompt. This command runs on the UI thread, so it reports the last
        // known answer instead of waiting for them.
        let last = std::sync::atomic::Ordering::Relaxed;
        let Ok(mut mihomo) = state.mihomo.try_lock() else {
            return Ok(LAST_STATUS.load(last));
        };
        if state.external_engine.load(last) {
            let up = mihomo.is_running();
            LAST_STATUS.store(up, last);
            return Ok(up);
        }
        let Ok(mut gc) = state.go_client.try_lock() else {
            return Ok(LAST_STATUS.load(last));
        };
        let up = mihomo.is_running() && gc.is_running();
        LAST_STATUS.store(up, last);
        Ok(up)
    }
}

#[tauri::command]
fn get_tunnel_state() -> serde_json::Value {
    match go_client::tunnel_state() {
        Some(t) => serde_json::json!({
            "known": true,
            "up": t.up,
            "error": t.error,
            "failure": t.failure,
            "connected_at": t.connected_at,
        }),
        None => serde_json::json!({
            "known": false,
            "up": false,
            "error": null,
            "failure": null,
            "connected_at": null,
        }),
    }
}

#[derive(Serialize)]
struct SystemInfoResponse {
    os: String,
    uptime: String,
    version: String,
    admin: bool,
}

#[tauri::command]
fn get_system_info(app: tauri::AppHandle) -> Result<SystemInfoResponse, String> {
    #[cfg(target_os = "android")]
    let os_info = format!("Android ({})", std::env::consts::ARCH);
    #[cfg(not(target_os = "android"))]
    let os_info = format!("Windows ({})", std::env::consts::ARCH);

    let uptime_ms = winapi_uptime();
    let uptime_secs = uptime_ms / 1000;
    let hours = uptime_secs / 3600;
    let minutes = (uptime_secs % 3600) / 60;
    let uptime = format!("{}h {}m", hours, minutes);

    let admin = is_elevated();

    Ok(SystemInfoResponse {
        os: os_info,
        uptime,
        version: format!("v{}", app.package_info().version),
        admin,
    })
}

fn winapi_uptime() -> u64 {
    #[cfg(target_os = "windows")]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetTickCount64() -> u64;
        }
        unsafe { GetTickCount64() }
    }
    #[cfg(not(target_os = "windows"))]
    {
        0
    }
}

fn is_elevated() -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        Command::new("net")
            .arg("session")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

#[tauri::command]
fn open_config_dir(app: tauri::AppHandle) -> Result<(), String> {
    let dir = app
        .path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    fs::create_dir_all(&dir).ok();
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(dir.to_string_lossy().to_string())
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&dir)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&dir)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn validate_external_url(url: &str) -> Result<(), String> {
    if url.len() > 2048 {
        return Err("url too long".into());
    }
    if url
        .chars()
        .any(|c| c.is_control() || matches!(c, '"' | '\'' | '\\' | '\n' | '\r' | '\0'))
    {
        return Err("url contains forbidden characters".into());
    }
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("mailto:"))
    {
        return Err("url scheme not allowed".into());
    }
    Ok(())
}

#[tauri::command]
fn read_clipboard_text() -> String {
    #[cfg(target_os = "android")]
    {
        return whisp_vpn_android::service_intent::read_clipboard().unwrap_or_default();
    }
    #[cfg(not(target_os = "android"))]
    {
        String::new()
    }
}

#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    validate_external_url(&url)?;

    #[cfg(target_os = "android")]
    {
        return whisp_vpn_android::service_intent::open_url_android(&url);
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/c", "start", "", &url])
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn get_routing_rules(app: tauri::AppHandle) -> Result<Vec<RoutingRule>, String> {
    let settings = get_app_settings(app)?;
    Ok(settings.routing_rules)
}

fn mihomo_is_idle(app: &tauri::AppHandle) -> bool {
    let state: tauri::State<AppState> = app.state();
    let idle = state.mihomo.try_lock().is_ok_and(|m| m.is_idle());
    idle
}

#[tauri::command]
async fn save_routing_rules(app: tauri::AppHandle, rules: Vec<RoutingRule>) -> Result<(), String> {
    let path = settings_path(&app);
    let mut settings = get_app_settings(app.clone())?;
    settings.routing_rules = rules;
    let data = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    fs::write(&path, &data).map_err(|e| e.to_string())?;

    let config_path = mihomo_config_path(&app);
    let socks_addr = if settings.socks_addr.contains(':') {
        settings.socks_addr.clone()
    } else {
        format!("{}:1080", settings.socks_addr)
    };
    let mut routing_rules: Vec<mihomo::MihomoRoutingRule> = Vec::new();
    for r in &settings.blocklist {
        routing_rules.push(mihomo::MihomoRoutingRule {
            kind: r.kind.clone(),
            value: r.value.clone(),
            action: "REJECT".to_string(),
        });
    }
    for r in &settings.routing_rules {
        routing_rules.push(mihomo::MihomoRoutingRule {
            kind: r.kind.clone(),
            value: r.value.clone(),
            action: r.action.clone(),
        });
    }
    let mihomo_config = mihomo::generate_config(&mihomo::MihomoConfig {
        socks_addr: &socks_addr,
        server_host: &server_host_from_key(&settings.conn_key),
        mixed_port: settings.mihomo_port,
        tun: tun_enabled(&settings),
        tun_stack: &settings.tun_stack,
        dns_redirect: settings.dns_redirect,
        ipv6: settings.ipv6,
        routing_rules: &routing_rules,
        extra_socks_addrs: &[],
        custom_dns: &settings.custom_dns,
        socks_user: &settings.socks_user,
        socks_pass: &settings.socks_pass,
        allow_lan: settings.allow_lan,
        log_level: &settings.log_level,
        routing_mode: &settings.routing_mode,
        bypass_ru: settings.bypass_ru,
        external_link: &settings.external_link,
        secret: &settings.secret,
        kill_switch: settings.kill_switch,
        vpn_dns: &settings.vpn_dns,
    });
    fs::write(&config_path, &mihomo_config).map_err(|e| e.to_string())?;

    if mihomo_is_idle(&app) {
        return Ok(());
    }
    let config_str = config_path.to_string_lossy().replace('\\', "/");
    let _ = reqwest::Client::new()
        .put("http://127.0.0.1:9090/configs?force=true")
        .bearer_auth(&settings.secret)
        .json(&serde_json::json!({ "path": config_str }))
        .timeout(Duration::from_secs(3))
        .send()
        .await;

    Ok(())
}

// The go-client process itself writes its logs to this file (-log-file flag
// in WhispVpnService.launchGoClient). It runs in the app's own UID, so the
// file is readable from any process of this app — including the main/UI
// process, which is where this Tauri command runs.
static GO_CLIENT_LOG_OFFSET: std::sync::Mutex<u64> = std::sync::Mutex::new(0);

fn go_client_log_path() -> Option<std::path::PathBuf> {
    for candidate in [
        "/data/data/com.whispera.whisp/files/go-client.log",
        "/data/user/0/com.whispera.whisp/files/go-client.log",
    ] {
        let p = std::path::PathBuf::from(candidate);
        if p.exists() {
            return Some(p);
        }
    }
    let desktop = std::env::temp_dir().join("whispera-go-client.log");
    if desktop.exists() {
        return Some(desktop);
    }
    None
}

// read_vpn_log_history returns the tail (~last hour's worth) of the persistent
// go-client log file so the UI can restore recent logs on launch regardless of
// connection state, then rebases the incremental offset to EOF so subsequent
// get_vpn_log polls don't re-deliver the same lines.
#[tauri::command]
fn read_vpn_log_history() -> Vec<String> {
    {
        use std::io::{Read, Seek, SeekFrom};
        let Some(path) = go_client_log_path() else {
            return Vec::new();
        };
        let Ok(mut f) = std::fs::File::open(&path) else {
            return Vec::new();
        };
        let Ok(meta) = f.metadata() else {
            return Vec::new();
        };
        let len = meta.len();
        const TAIL: u64 = 32 * 1024;
        const HISTORY_LINES: usize = 140;
        let start = len.saturating_sub(TAIL);
        if f.seek(SeekFrom::Start(start)).is_err() {
            return Vec::new();
        }
        let mut buf = Vec::new();
        if f.read_to_end(&mut buf).is_err() {
            return Vec::new();
        }
        if let Ok(mut offset) = GO_CLIENT_LOG_OFFSET.lock() {
            *offset = len;
        }
        let text = if start > 0 {
            match buf.iter().position(|&b| b == b'\n') {
                Some(nl) => String::from_utf8_lossy(&buf[nl + 1..]).into_owned(),
                None => String::new(),
            }
        } else {
            String::from_utf8_lossy(&buf).into_owned()
        };
        let all: Vec<String> = text.lines().map(|l| l.to_string()).collect();
        let start = all.len().saturating_sub(HISTORY_LINES);
        return all[start..].to_vec();
    }
    #[allow(unreachable_code)]
    Vec::new()
}

#[tauri::command]
fn get_vpn_log() -> Vec<String> {
    {
        use std::io::{Read, Seek, SeekFrom};
        let Some(path) = go_client_log_path() else {
            return Vec::new();
        };
        let Ok(mut f) = std::fs::File::open(&path) else {
            return Vec::new();
        };
        let Ok(meta) = f.metadata() else {
            return Vec::new();
        };
        let len = meta.len();
        let Ok(mut offset) = GO_CLIENT_LOG_OFFSET.lock() else {
            return Vec::new();
        };
        if len < *offset {
            *offset = len;
            return Vec::new();
        }
        if f.seek(SeekFrom::Start(*offset)).is_err() {
            return Vec::new();
        }
        let mut buf = Vec::new();
        if f.read_to_end(&mut buf).is_err() {
            return Vec::new();
        }
        let Some(last_newline) = buf.iter().rposition(|&b| b == b'\n') else {
            return Vec::new();
        };
        *offset += (last_newline + 1) as u64;
        let text = String::from_utf8_lossy(&buf[..=last_newline]);
        return text.lines().map(|l| l.to_string()).collect();
    }
    #[allow(unreachable_code)]
    Vec::new()
}

#[tauri::command]
fn clear_vpn_log() -> Result<(), String> {
    let Some(path) = go_client_log_path() else {
        return Ok(());
    };
    std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    if let Ok(mut offset) = GO_CLIENT_LOG_OFFSET.lock() {
        *offset = 0;
    }
    Ok(())
}

#[tauri::command]
fn get_blocklist(app: tauri::AppHandle) -> Result<Vec<RoutingRule>, String> {
    let settings = get_app_settings(app)?;
    Ok(settings.blocklist)
}

#[tauri::command]
async fn save_blocklist(app: tauri::AppHandle, rules: Vec<RoutingRule>) -> Result<(), String> {
    let path = settings_path(&app);
    let mut settings = get_app_settings(app.clone())?;
    settings.blocklist = rules;
    let data = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    fs::write(&path, &data).map_err(|e| e.to_string())?;

    let config_path = mihomo_config_path(&app);
    let socks_addr = if settings.socks_addr.contains(':') {
        settings.socks_addr.clone()
    } else {
        format!("{}:1080", settings.socks_addr)
    };
    let mut routing_rules: Vec<mihomo::MihomoRoutingRule> = Vec::new();
    for r in &settings.blocklist {
        routing_rules.push(mihomo::MihomoRoutingRule {
            kind: r.kind.clone(),
            value: r.value.clone(),
            action: "REJECT".to_string(),
        });
    }
    for r in &settings.routing_rules {
        routing_rules.push(mihomo::MihomoRoutingRule {
            kind: r.kind.clone(),
            value: r.value.clone(),
            action: r.action.clone(),
        });
    }
    let mihomo_config = mihomo::generate_config(&mihomo::MihomoConfig {
        socks_addr: &socks_addr,
        server_host: &server_host_from_key(&settings.conn_key),
        mixed_port: settings.mihomo_port,
        tun: tun_enabled(&settings),
        tun_stack: &settings.tun_stack,
        dns_redirect: settings.dns_redirect,
        ipv6: settings.ipv6,
        routing_rules: &routing_rules,
        extra_socks_addrs: &[],
        custom_dns: &settings.custom_dns,
        socks_user: &settings.socks_user,
        socks_pass: &settings.socks_pass,
        allow_lan: settings.allow_lan,
        log_level: &settings.log_level,
        routing_mode: &settings.routing_mode,
        bypass_ru: settings.bypass_ru,
        external_link: &settings.external_link,
        secret: &settings.secret,
        kill_switch: settings.kill_switch,
        vpn_dns: &settings.vpn_dns,
    });
    fs::write(&config_path, &mihomo_config).map_err(|e| e.to_string())?;

    if mihomo_is_idle(&app) {
        return Ok(());
    }
    let config_str = config_path.to_string_lossy().replace('\\', "/");
    let _ = reqwest::Client::new()
        .put("http://127.0.0.1:9090/configs?force=true")
        .bearer_auth(&settings.secret)
        .json(&serde_json::json!({ "path": config_str }))
        .timeout(Duration::from_secs(3))
        .send()
        .await;

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SubscriptionEntry {
    id: String,
    name: String,
    url: String,
    keys: Vec<String>,
    servers: Vec<serde_json::Value>,
    updated: String,
}

fn subscriptions_path(app: &tauri::AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    fs::create_dir_all(&dir).ok();
    dir.join("subscriptions.json")
}

fn load_subs(app: &tauri::AppHandle) -> Vec<SubscriptionEntry> {
    let path = subscriptions_path(app);
    if !path.exists() {
        return Vec::new();
    }
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_subs(app: &tauri::AppHandle, subs: &[SubscriptionEntry]) {
    if let Ok(data) = serde_json::to_string_pretty(subs) {
        fs::write(subscriptions_path(app), data).ok();
    }
}

fn validate_subscription_url(url: &str) -> Result<(), String> {
    if url.len() > 2048 {
        return Err("url too long".into());
    }
    if url.chars().any(|c| c.is_control()) {
        return Err("url contains control characters".into());
    }
    let lower = url.to_ascii_lowercase();
    if !lower.starts_with("https://") {
        return Err("only https:// subscription urls are accepted".into());
    }
    let after_scheme = &url[8..];
    let host_end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    let authority = &after_scheme[..host_end];
    let host = authority
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(authority);
    let host_only = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
    let bare = host_only.trim_start_matches('[').trim_end_matches(']');
    if bare.is_empty() {
        return Err("url host empty".into());
    }
    if let Ok(ip) = bare.parse::<std::net::IpAddr>() {
        let blocked = match ip {
            std::net::IpAddr::V4(v4) => {
                v4.is_loopback()
                    || v4.is_private()
                    || v4.is_link_local()
                    || v4.is_broadcast()
                    || v4.is_unspecified()
                    || v4.is_multicast()
                    || v4.octets()[0] == 0
            }
            std::net::IpAddr::V6(v6) => {
                v6.is_loopback()
                    || v6.is_unspecified()
                    || v6.is_multicast()
                    || (v6.segments()[0] & 0xfe00) == 0xfc00
                    || (v6.segments()[0] & 0xffc0) == 0xfe80
            }
        };
        if blocked {
            return Err("subscription host points to a private/loopback address".into());
        }
    } else {
        let h = bare.to_ascii_lowercase();
        if h == "localhost"
            || h.ends_with(".localhost")
            || h.ends_with(".local")
            || h.ends_with(".internal")
        {
            return Err("subscription host points to a local address".into());
        }
    }
    Ok(())
}

const SUB_MAX_BYTES: usize = 1024 * 1024;

fn collect_proxy_links(body: &str) -> Vec<String> {
    const SCHEMES: [&str; 5] = ["vless://", "trojan://", "ss://", "vmess://", "hysteria2://"];
    body.lines()
        .map(|l| l.trim())
        .filter(|l| SCHEMES.iter().any(|s| l.starts_with(s)))
        .map(|l| l.to_string())
        .collect()
}

async fn fetch_sub_url(url: &str) -> Result<SubscriptionEntry, String> {
    use base64::Engine as _;
    validate_subscription_url(url)?;
    let client = reqwest::Client::builder()
        .min_tls_version(reqwest::tls::Version::TLS_1_2)
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(12))
        // Services move their feeds around, so a plain 301 must not read as an
        // empty subscription. Every hop is validated like the original URL, which
        // keeps a redirect from being a way into private addresses.
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 3 {
                return attempt.error("too many redirects");
            }
            match validate_subscription_url(attempt.url().as_str()) {
                Ok(()) => attempt.follow(),
                Err(e) => attempt.error(e),
            }
        }))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(url)
        .header("User-Agent", "Whisp/1.0")
        .send()
        .await
        .map_err(|e| format!("fetch failed: {}", e))?;
    if let Some(len) = resp.content_length() {
        if (len as usize) > SUB_MAX_BYTES {
            return Err("subscription response too large".into());
        }
    }
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    if bytes.len() > SUB_MAX_BYTES {
        return Err("subscription response too large".into());
    }
    let text = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(text.trim())
        .unwrap_or_else(|_| text.as_bytes().to_vec());

    // Two shapes are accepted: our own JSON feed, and the widespread
    // "base64 of newline-separated proxy links" used by other services.
    let payload: serde_json::Value = match serde_json::from_slice(&decoded) {
        Ok(v) => v,
        Err(_) => {
            let body = String::from_utf8_lossy(&decoded);
            let links = collect_proxy_links(&body);
            if links.is_empty() {
                return Err(
                    "subscription is neither our JSON feed nor a list of proxy links".into(),
                );
            }
            return Ok(SubscriptionEntry {
                id: String::new(),
                name: String::new(),
                url: url.to_string(),
                keys: links,
                servers: Vec::new(),
                updated: String::new(),
            });
        }
    };
    let keys = payload["keys"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let servers = payload["servers"].as_array().cloned().unwrap_or_default();
    let name = payload["name"].as_str().unwrap_or("").to_string();
    let updated = payload["updated"].as_str().unwrap_or("").to_string();
    Ok(SubscriptionEntry {
        id: String::new(),
        name,
        url: url.to_string(),
        keys,
        servers,
        updated,
    })
}

#[tauri::command]
fn get_subscriptions(app: tauri::AppHandle) -> Vec<SubscriptionEntry> {
    load_subs(&app)
}

#[tauri::command]
async fn add_subscription(
    app: tauri::AppHandle,
    name: String,
    url: String,
) -> Result<SubscriptionEntry, String> {
    let mut entry = fetch_sub_url(&url).await?;
    entry.id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string();
    if !name.is_empty() {
        entry.name = name;
    }
    entry.url = url;
    let mut subs = load_subs(&app);
    subs.push(entry.clone());
    save_subs(&app, &subs);
    Ok(entry)
}

#[tauri::command]
fn import_subscriptions(
    app: tauri::AppHandle,
    entries: Vec<SubscriptionEntry>,
) -> Result<Vec<SubscriptionEntry>, String> {
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    for e in &entries {
        validate_subscription_url(&e.url)?;
    }
    let base_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let mut subs = load_subs(&app);
    let mut imported: Vec<SubscriptionEntry> = Vec::with_capacity(entries.len());
    for (i, mut entry) in entries.into_iter().enumerate() {
        entry.id = (base_ms + i as u128).to_string();
        subs.push(entry.clone());
        imported.push(entry);
    }
    save_subs(&app, &subs);
    Ok(imported)
}

#[tauri::command]
async fn refresh_subscription(
    app: tauri::AppHandle,
    id: String,
) -> Result<SubscriptionEntry, String> {
    let mut subs = load_subs(&app);
    let idx = subs
        .iter()
        .position(|s| s.id == id)
        .ok_or("Subscription not found")?;
    let url = subs[idx].url.clone();
    let fresh = fetch_sub_url(&url).await?;
    subs[idx].keys = fresh.keys;
    subs[idx].servers = fresh.servers;
    subs[idx].updated = fresh.updated;
    if subs[idx].name.is_empty() && !fresh.name.is_empty() {
        subs[idx].name = fresh.name;
    }
    let result = subs[idx].clone();
    save_subs(&app, &subs);
    Ok(result)
}

#[tauri::command]
fn delete_subscription(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let mut subs = load_subs(&app);
    subs.retain(|s| s.id != id);
    save_subs(&app, &subs);
    Ok(())
}

#[tauri::command]
fn rename_subscription(app: tauri::AppHandle, id: String, name: String) -> Result<(), String> {
    let mut subs = load_subs(&app);
    let idx = subs.iter().position(|s| s.id == id).ok_or("not found")?;
    subs[idx].name = name;
    save_subs(&app, &subs);
    Ok(())
}

#[tauri::command]
fn delete_sub_key(
    app: tauri::AppHandle,
    id: String,
    index: usize,
) -> Result<SubscriptionEntry, String> {
    let mut subs = load_subs(&app);
    let idx = subs
        .iter()
        .position(|s| s.id == id)
        .ok_or("Subscription not found")?;
    if index >= subs[idx].keys.len() {
        return Err("key index out of range".into());
    }
    subs[idx].keys.remove(index);
    if index < subs[idx].servers.len() {
        subs[idx].servers.remove(index);
    }
    let result = subs[idx].clone();
    save_subs(&app, &subs);
    Ok(result)
}

#[tauri::command]
async fn check_subscription_update(
    app: tauri::AppHandle,
    id: String,
) -> Result<SubscriptionEntry, String> {
    let subs = load_subs(&app);
    let sub = subs.iter().find(|s| s.id == id).ok_or("not found")?;
    let url = sub.url.clone();
    let fresh = fetch_sub_url(&url).await?;
    let mut result = sub.clone();
    result.updated = fresh.updated;
    Ok(result)
}

// Keys come base64-encoded far more often than as a bare host:port, so anything
// reaching for the server address must decode first — parsing the raw body only
// ever worked for the legacy form.
fn server_addr_from_key(key: &str) -> Option<(String, u16)> {
    use base64::Engine as _;

    // Foreign links carry the endpoint after the userinfo: scheme://id@host:port?...
    for scheme in ["vless://", "trojan://", "ss://", "vmess://"] {
        let Some(rest) = key.trim().strip_prefix(scheme) else {
            continue;
        };
        let rest = rest.split(['?', '#']).next().unwrap_or(rest);
        let hostport = rest.rsplit_once('@').map(|(_, h)| h).unwrap_or(rest);
        let (h, p) = hostport.rsplit_once(':')?;
        let host = h.trim_matches(|c| c == '[' || c == ']');
        if host.is_empty() {
            return None;
        }
        return Some((host.to_string(), p.parse().unwrap_or(443)));
    }
    let body = key.trim().strip_prefix("whispera://").unwrap_or(key.trim());
    let b64 = body.split(['?', '#']).next().unwrap_or(body);

    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) {
            for field in ["whispera_addr", "server"] {
                let Some(addr) = v.get(field).and_then(|x| x.as_str()) else {
                    continue;
                };
                match addr.rsplit_once(':') {
                    Some((h, p)) if !h.is_empty() => {
                        return Some((h.to_string(), p.parse().unwrap_or(443)))
                    }
                    _ if !addr.is_empty() => return Some((addr.to_string(), 443)),
                    _ => {}
                }
            }
        }
    }

    match b64.rsplit_once(':') {
        Some((h, p)) if !h.is_empty() => Some((h.to_string(), p.parse().unwrap_or(443))),
        _ => None,
    }
}

fn server_host_from_key(key: &str) -> String {
    server_addr_from_key(key)
        .map(|(h, _)| h)
        .unwrap_or_default()
}

#[tauri::command]
fn vpn_lockdown() -> bool {
    #[cfg(target_os = "android")]
    {
        return std::panic::catch_unwind(whisp_vpn_android::service_intent::is_lockdown_enabled)
            .unwrap_or(Ok(false))
            .unwrap_or(false);
    }
    #[cfg(not(target_os = "android"))]
    false
}

#[tauri::command]
fn open_vpn_settings() -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        std::panic::catch_unwind(whisp_vpn_android::service_intent::open_vpn_settings)
            .map_err(|_| "panic".to_string())??;
        return Ok(());
    }
    #[cfg(not(target_os = "android"))]
    Err("android only".to_string())
}

#[tauri::command]
fn set_app_language(lang: String) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        std::panic::catch_unwind(move || whisp_vpn_android::service_intent::set_language(&lang))
            .map_err(|_| "panic in set_language".to_string())??;
    }
    #[cfg(not(target_os = "android"))]
    let _ = lang;
    Ok(())
}

#[tauri::command]
async fn ping_key(key: String, mode: Option<String>) -> Result<u64, String> {
    let (host, port) = server_addr_from_key(&key).ok_or("cannot parse server address")?;
    let (host, port) = (host.as_str(), port);
    let host_port = format!("{}:{}", host, port);

    let mode = mode.unwrap_or_else(|| "tcp".to_string());

    // Always measure the raw path to this key's server, never through the active
    // tunnel: the SOCKS proxy confirms a connection before it exists, so a
    // tunnelled reading times localhost instead of the server, and keys stop
    // being comparable with each other — which is the whole point of the check.
    if mode == "icmp" {
        return icmp_ping(host).await;
    }

    let start = std::time::Instant::now();
    match mode.as_str() {
        "get" | "head" => http_ping(&mode, host, port).await?,
        _ => {
            tokio::time::timeout(
                Duration::from_secs(5),
                tokio::net::TcpStream::connect(&host_port),
            )
            .await
            .map_err(|_| "timeout".to_string())?
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(start.elapsed().as_millis() as u64)
}

async fn http_ping(mode: &str, host: &str, port: u16) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .danger_accept_invalid_certs(true)
        .no_proxy()
        .build()
        .map_err(|e| e.to_string())?;

    let url = format!("https://{}:{}/", host, port);
    let req = if mode == "head" {
        client.head(&url)
    } else {
        client.get(&url)
    };
    // The server camouflages as a site, so any answer — even a 404 — is a
    // complete round trip and times it honestly.
    req.send().await.map(|_| ()).map_err(|e| {
        if e.is_timeout() {
            "timeout".to_string()
        } else {
            e.to_string()
        }
    })
}

async fn icmp_ping(host: &str) -> Result<u64, String> {
    let host = host.to_string();
    tokio::task::spawn_blocking(move || icmp_ping_blocking(&host))
        .await
        .map_err(|e| e.to_string())?
}

const ICMP_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(not(windows))]
const ICMP_ECHO_REPLY_TYPE: u8 = 0;

fn icmp_ping_blocking(host: &str) -> Result<u64, String> {
    use std::net::{IpAddr, ToSocketAddrs};

    let addr = (host, 0u16)
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .find(|a| a.is_ipv4())
        .ok_or("no IPv4 address")?;
    let IpAddr::V4(v4) = addr.ip() else {
        return Err("no IPv4 address".to_string());
    };
    icmp_echo(v4)
}

#[cfg(windows)]
fn icmp_echo(dest: std::net::Ipv4Addr) -> Result<u64, String> {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, ICMP_ECHO_REPLY, IP_SUCCESS,
    };
    const PAYLOAD_LEN: usize = 8;
    const ICMP_ERROR_LEN: usize = 8;

    let payload = [0u8; PAYLOAD_LEN];
    let mut reply =
        vec![0u8; std::mem::size_of::<ICMP_ECHO_REPLY>() + PAYLOAD_LEN + ICMP_ERROR_LEN];
    let handle = unsafe { IcmpCreateFile() };
    if handle == INVALID_HANDLE_VALUE {
        return Err(format!("icmp handle: {}", std::io::Error::last_os_error()));
    }
    let replies = unsafe {
        IcmpSendEcho(
            handle,
            u32::from_ne_bytes(dest.octets()),
            payload.as_ptr().cast(),
            PAYLOAD_LEN as u16,
            std::ptr::null(),
            reply.as_mut_ptr().cast(),
            reply.len() as u32,
            ICMP_TIMEOUT.as_millis() as u32,
        )
    };
    unsafe { IcmpCloseHandle(handle) };
    if replies == 0 {
        return Err("timeout".to_string());
    }
    let echo = unsafe { std::ptr::read_unaligned(reply.as_ptr().cast::<ICMP_ECHO_REPLY>()) };
    if echo.Status != IP_SUCCESS {
        return Err(format!("icmp status {}", echo.Status));
    }
    Ok(echo.RoundTripTime as u64)
}

#[cfg(not(windows))]
fn ip_header_len(first_byte: u8) -> usize {
    if first_byte >> 4 == 4 {
        (first_byte & 0x0f) as usize * 4
    } else {
        0
    }
}

#[cfg(not(windows))]
fn icmp_echo(v4: std::net::Ipv4Addr) -> Result<u64, String> {
    use socket2::{Domain, Protocol, Socket, Type};
    use std::net::{IpAddr, SocketAddr};

    let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::ICMPV4))
        .map_err(|e| format!("icmp socket: {}", e))?;
    sock.set_read_timeout(Some(ICMP_TIMEOUT))
        .map_err(|e| e.to_string())?;

    let ident: u16 = std::process::id() as u16;
    let mut packet = [0u8; 16];
    packet[0] = 8;
    packet[4..6].copy_from_slice(&ident.to_be_bytes());
    packet[6..8].copy_from_slice(&1u16.to_be_bytes());
    let sum = icmp_checksum(&packet);
    packet[2..4].copy_from_slice(&sum.to_be_bytes());

    let start = std::time::Instant::now();
    sock.send_to(&packet, &SocketAddr::new(IpAddr::V4(v4), 0).into())
        .map_err(|e| format!("icmp send: {}", e))?;

    // Anything may land on the socket — an unreachable error answers instantly and
    // would be timed as a successful round trip. Only an echo reply counts.
    let deadline = start + ICMP_TIMEOUT;
    let mut buf = [std::mem::MaybeUninit::<u8>::uninit(); 128];
    loop {
        if std::time::Instant::now() >= deadline {
            return Err("timeout".to_string());
        }
        let n = sock.recv(&mut buf).map_err(|_| "timeout".to_string())?;
        if n == 0 {
            continue;
        }
        let at = ip_header_len(unsafe { buf[0].assume_init() });
        if n > at && unsafe { buf[at].assume_init() } == ICMP_ECHO_REPLY_TYPE {
            return Ok(start.elapsed().as_millis() as u64);
        }
    }
}

#[cfg(not(windows))]
fn icmp_checksum(data: &[u8]) -> u16 {
    let mut sum = 0u32;
    for chunk in data.chunks(2) {
        let word = if chunk.len() == 2 {
            u16::from_be_bytes([chunk[0], chunk[1]]) as u32
        } else {
            (chunk[0] as u32) << 8
        };
        sum = sum.wrapping_add(word);
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

#[derive(Debug, Clone, Serialize)]
struct ProcessInfo {
    name: String,
    label: String,
    pid: u32,
}

#[cfg(any(target_os = "macos", test))]
fn macos_processes(ps_output: &str) -> Vec<ProcessInfo> {
    let bundle_marker = format!("{}/", mihomo::APP_BUNDLE_SUFFIX);
    let mut seen = std::collections::HashSet::new();
    let mut processes = Vec::new();
    for line in ps_output.lines() {
        let Some((pid, command)) = line.trim().split_once(char::is_whitespace) else {
            continue;
        };
        let Ok(pid) = pid.parse::<u32>() else {
            continue;
        };
        let command = command.trim();
        let (name, label) = match command.find(&bundle_marker) {
            Some(end) => {
                let bundle = &command[..end + mihomo::APP_BUNDLE_SUFFIX.len()];
                let label = std::path::Path::new(bundle)
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or(bundle);
                (bundle.to_string(), label.to_string())
            }
            None => {
                let name = std::path::Path::new(command)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(command);
                (name.to_string(), name.to_string())
            }
        };
        if !name.is_empty() && seen.insert(name.to_lowercase()) {
            processes.push(ProcessInfo { name, label, pid });
        }
    }
    processes
}

#[tauri::command]
fn list_processes() -> Result<Vec<ProcessInfo>, String> {
    let mut result = Vec::new();

    #[cfg(target_os = "android")]
    {
        let apps = whisp_vpn_android::pkg_list::list_user_packages()
            .map_err(|e| format!("Ошибка PackageManager: {}", e))?;
        for (i, a) in apps.into_iter().enumerate() {
            result.push(ProcessInfo {
                name: a.package,
                label: a.label,
                pid: i as u32,
            });
        }
        return Ok(result);
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let out = std::process::Command::new("tasklist")
            .args(["/FO", "CSV", "/NH"])
            .creation_flags(0x08000000u32)
            .output();
        if let Ok(out) = out {
            let text = String::from_utf8_lossy(&out.stdout);
            let mut seen = std::collections::HashSet::new();
            for line in text.lines() {
                let line = line.trim().trim_matches('"');
                let parts: Vec<&str> = line.splitn(6, "\",\"").collect();
                if parts.len() >= 2 {
                    let name = parts[0].trim_matches('"').to_string();
                    let pid: u32 = parts[1].trim_matches('"').parse().unwrap_or(0);
                    if !name.is_empty() && seen.insert(name.to_lowercase()) {
                        result.push(ProcessInfo {
                            label: name.clone(),
                            name,
                            pid,
                        });
                    }
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("ps")
            .args(["-axo", "pid=,comm="])
            .output();
        if let Ok(out) = out {
            result.extend(macos_processes(&String::from_utf8_lossy(&out.stdout)));
        }
    }

    #[cfg(all(
        not(target_os = "windows"),
        not(target_os = "android"),
        not(target_os = "macos")
    ))]
    {
        let out = std::process::Command::new("ps")
            .args(["-eo", "comm,pid", "--no-headers"])
            .output();
        if let Ok(out) = out {
            let text = String::from_utf8_lossy(&out.stdout);
            let mut seen = std::collections::HashSet::new();
            for line in text.lines() {
                let parts: Vec<&str> = line.trim().splitn(2, ' ').collect();
                if parts.len() == 2 {
                    let name = parts[0].trim().to_string();
                    let pid: u32 = parts[1].trim().parse().unwrap_or(0);
                    if !name.is_empty() && seen.insert(name.to_lowercase()) {
                        result.push(ProcessInfo {
                            label: name.clone(),
                            name,
                            pid,
                        });
                    }
                }
            }
        }
    }

    #[cfg(not(target_os = "android"))]
    {
        result.sort_by_key(|p| p.label.to_lowercase());
        Ok(result)
    }
}

#[derive(Serialize, Clone)]
struct UpdateInfo {
    tag: String,
    name: String,
    body: String,
    html_url: String,
    download_url: String,
    is_newer: bool,
}

fn is_newer_version(tag: &str, current: &str) -> bool {
    let tag = tag.trim_start_matches('v');
    let parse = |s: &str| -> Vec<u64> { s.split('.').filter_map(|p| p.parse().ok()).collect() };
    parse(tag) > parse(current)
}

fn find_asset_url(assets: &serde_json::Value) -> String {
    let arr = match assets.as_array() {
        Some(a) => a,
        None => return String::new(),
    };
    let names: Vec<(&str, &str)> = arr
        .iter()
        .filter_map(|a| {
            let name = a["name"].as_str()?;
            let url = a["browser_download_url"].as_str()?;
            Some((name, url))
        })
        .collect();

    #[cfg(target_os = "windows")]
    {
        if let Some((_, url)) = names.iter().find(|(n, _)| n.ends_with("-setup.exe")) {
            return url.to_string();
        }
        if let Some((_, url)) = names.iter().find(|(n, _)| n.ends_with(".exe")) {
            return url.to_string();
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Some((_, url)) = names.iter().find(|(n, _)| n.ends_with(".AppImage")) {
            return url.to_string();
        }
        if let Some((_, url)) = names.iter().find(|(n, _)| n.ends_with(".deb")) {
            return url.to_string();
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some((_, url)) = names.iter().find(|(n, _)| n.ends_with(".app.tar.gz")) {
            return url.to_string();
        }
        if let Some((_, url)) = names.iter().find(|(n, _)| n.ends_with(".dmg")) {
            return url.to_string();
        }
    }
    #[cfg(target_os = "android")]
    {
        if let Some((_, url)) = names.iter().find(|(n, _)| n.ends_with(".apk")) {
            return url.to_string();
        }
    }
    String::new()
}

const UPDATE_USER_AGENT: &str = "whisp-updater/1.0";
const UPDATE_CHECK_TIMEOUT: Duration = Duration::from_secs(10);
const GITHUB_SITE: &str = "https://github.com";
const RELEASES_PATH: &str = "/nekoskin/Whisp/releases";
const LATEST_RELEASE_API: &str = "https://api.github.com/repos/nekoskin/Whisp/releases/latest";

async fn latest_release_from_site() -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .user_agent(UPDATE_USER_AGENT)
        .timeout(UPDATE_CHECK_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let latest = client
        .get(format!("{GITHUB_SITE}{RELEASES_PATH}/latest"))
        .send()
        .await
        .map_err(|e| format!("github.com unavailable: {e}"))?;
    let tag = latest
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|location| location.to_str().ok())
        .and_then(|location| location.rsplit_once("/tag/"))
        .map(|(_, tag)| tag.to_string())
        .ok_or("github.com did not name the latest release")?;
    let page = client
        .get(format!(
            "{GITHUB_SITE}{RELEASES_PATH}/expanded_assets/{tag}"
        ))
        .send()
        .await
        .map_err(|e| format!("github.com unavailable: {e}"))?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    Ok(release_from_assets_page(&tag, &page))
}

fn release_from_assets_page(tag: &str, page: &str) -> serde_json::Value {
    let download_prefix = format!("{RELEASES_PATH}/download/{tag}/");
    let assets: Vec<serde_json::Value> = page
        .split("href=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .filter_map(|href| {
            let name = href.strip_prefix(&download_prefix)?;
            Some(serde_json::json!({
                "name": name,
                "browser_download_url": format!("{GITHUB_SITE}{href}"),
            }))
        })
        .collect();
    serde_json::json!({
        "tag_name": tag,
        "html_url": format!("{GITHUB_SITE}{RELEASES_PATH}/tag/{tag}"),
        "assets": assets,
    })
}

#[tauri::command]
async fn check_for_updates(app: tauri::AppHandle) -> Result<UpdateInfo, String> {
    let client = reqwest::Client::builder()
        .user_agent(UPDATE_USER_AGENT)
        .timeout(UPDATE_CHECK_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;

    let json: serde_json::Value = match client.get(LATEST_RELEASE_API).send().await {
        Ok(resp) if resp.status().as_u16() == 404 => {
            return Ok(UpdateInfo {
                tag: String::new(),
                name: String::new(),
                body: String::new(),
                html_url: String::new(),
                download_url: String::new(),
                is_newer: false,
            });
        }
        Ok(resp) if resp.status().is_success() => resp.json().await.map_err(|e| e.to_string())?,
        Ok(resp) => {
            let api_error = format!("GitHub API: HTTP {}", resp.status().as_u16());
            latest_release_from_site()
                .await
                .map_err(|e| format!("{api_error}; {e}"))?
        }
        Err(e) => {
            let api_error = format!("GitHub API unavailable: {}", e);
            latest_release_from_site()
                .await
                .map_err(|e| format!("{api_error}; {e}"))?
        }
    };

    let tag = json["tag_name"].as_str().unwrap_or("").to_string();
    let name = json["name"].as_str().unwrap_or(&tag).to_string();
    let body = json["body"].as_str().unwrap_or("").to_string();
    let html_url = json["html_url"].as_str().unwrap_or("").to_string();
    let download_url = find_asset_url(&json["assets"]);
    let running = app.package_info().version.to_string();
    let is_newer = is_newer_version(&tag, &running);

    Ok(UpdateInfo {
        tag,
        name,
        body,
        html_url,
        download_url,
        is_newer,
    })
}

#[tauri::command]
#[allow(unreachable_code)]
async fn install_update(
    app: tauri::AppHandle,
    download_url: String,
    html_url: String,
) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        let _ = app;
        let url = if !download_url.is_empty() {
            &download_url
        } else {
            &html_url
        };
        validate_external_url(url)?;
        return whisp_vpn_android::service_intent::open_url_android(url);
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let _ = download_url;
        return install_signed_update(&app, &html_url).await;
    }

    if download_url.is_empty() {
        #[cfg(not(target_os = "android"))]
        #[allow(deprecated)]
        app.shell()
            .open(&html_url, None)
            .map_err(|e| e.to_string())?;
        return Ok(());
    }

    let client = reqwest::Client::builder()
        .user_agent("whisp-updater/1.0")
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| e.to_string())?;

    let filename = download_url
        .split('/')
        .next_back()
        .unwrap_or("whisp-installer");
    let tmp_path = std::env::temp_dir().join(filename);

    let bytes = client
        .get(&download_url)
        .send()
        .await
        .map_err(|e| format!("Download failed: {}", e))?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;

    std::fs::write(&tmp_path, &bytes).map_err(|e| format!("Write failed: {}", e))?;

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new(&tmp_path)
            .spawn()
            .map_err(|e| format!("Launch failed: {}", e))?;
        std::process::exit(0);
    }

    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
const UPDATER_PLUGIN: &str = "updater";

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn updates_are_signed(config: &tauri::Config) -> bool {
    config.plugins.0.contains_key(UPDATER_PLUGIN)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
async fn install_signed_update(app: &tauri::AppHandle, release_page: &str) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;

    let installed = if updates_are_signed(app.config()) {
        async {
            let update = app
                .updater()
                .map_err(|e| e.to_string())?
                .check()
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "no signed update is published for this build".to_string())?;
            update
                .download_and_install(|_, _| {}, || {})
                .await
                .map_err(|e| e.to_string())
        }
        .await
    } else {
        Err("this build carries no update signing key".to_string())
    };
    match installed {
        Ok(()) => app.restart(),
        Err(e) => {
            eprintln!("[updater] {e}, opening the release page instead");
            #[allow(deprecated)]
            app.shell()
                .open(release_page, None)
                .map_err(|e| e.to_string())
        }
    }
}

#[cfg(desktop)]
fn show_main_window(app: &tauri::AppHandle) {
    use tauri::Manager;
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn stop_sidecars_and_exit(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            std::process::exit(0);
        });
        restore_system_proxy(&app);
        let m_app = app.clone();
        let m = std::thread::spawn(move || {
            let state: tauri::State<AppState> = m_app.state();
            let _ = state.mihomo.try_lock().map(|mut m| m.stop_tun().ok());
        });
        let g_app = app.clone();
        let g = std::thread::spawn(move || {
            let state: tauri::State<AppState> = g_app.state();
            let _ = state.go_client.try_lock().map(|mut gc| gc.stop().ok());
        });
        let _ = m.join();
        let _ = g.join();
        std::process::exit(0);
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "macos")]
    if let Some(served) = tun_helper::run_if_requested() {
        if let Err(e) = served {
            eprintln!("{} {e}", tun_helper::HELPER_FLAG);
            std::process::exit(1);
        }
        return;
    }

    #[cfg(all(target_os = "linux", not(target_os = "android")))]
    {
        for (k, v) in [
            ("WEBKIT_DISABLE_DMABUF_RENDERER", "1"),
            ("WEBKIT_DISABLE_COMPOSITING_MODE", "1"),
        ] {
            if std::env::var_os(k).is_none() {
                std::env::set_var(k, v);
            }
        }
    }

    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    #[cfg(all(target_os = "windows", not(target_os = "android")))]
    const EXE_EXT: &str = ".exe";
    #[cfg(all(not(target_os = "windows"), not(target_os = "android")))]
    const EXE_EXT: &str = "";

    #[cfg(target_os = "android")]
    let android_native_lib_dir: Option<PathBuf> = (|| {
        let maps = std::fs::read_to_string("/proc/self/maps").ok()?;
        for line in maps.lines() {
            let parts: Vec<&str> = line.splitn(6, ' ').collect();
            let path_str = parts.get(5)?.trim();
            if path_str.contains("com.whispera.whisp") && path_str.ends_with(".so") {
                let p = std::path::Path::new(path_str);
                if p.exists() {
                    return p.parent().map(|d| d.to_path_buf());
                }
            }
        }
        None
    })();

    #[cfg(target_os = "android")]
    let resolve_android_sidecar = |name: &str| -> PathBuf {
        let so_name = format!("lib{}.so", name);
        if let Some(ref lib_dir) = android_native_lib_dir {
            let p = lib_dir.join(&so_name);
            if p.exists() {
                return p;
            }
        }
        let candidates = [
            exe_dir.join(&so_name),
            exe_dir.join(name),
            PathBuf::from(format!("/data/data/com.whispera.whisp/lib/{}", so_name)),
            PathBuf::from(format!(
                "/data/app/com.whispera.whisp/lib/arm64/{}",
                so_name
            )),
        ];
        candidates
            .iter()
            .find(|p| p.exists())
            .cloned()
            .unwrap_or_else(|| candidates[0].clone())
    };

    #[cfg(not(target_os = "android"))]
    let mihomo_path = exe_dir.join(format!("mihomo{}", EXE_EXT));
    #[cfg(target_os = "android")]
    let mihomo_path = resolve_android_sidecar("mihomo");

    #[cfg(not(target_os = "android"))]
    let go_client_path = exe_dir.join(format!("whispera-go-client{}", EXE_EXT));
    #[cfg(target_os = "android")]
    let go_client_path = resolve_android_sidecar("whispera-go-client");

    #[cfg(not(target_os = "android"))]
    for p in [&mihomo_path, &go_client_path] {
        if let Err(e) = verify_sidecar(p) {
            eprintln!("[sidecar] refusing to start: {}", e);
            std::process::exit(1);
        }
    }

    let context = tauri::generate_context!();
    let builder = tauri::Builder::default();

    #[cfg(desktop)]
    let builder = builder
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_autostart::Builder::new().build());

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    let builder = if updates_are_signed(context.config()) {
        builder.plugin(tauri_plugin_updater::Builder::new().build())
    } else {
        builder
    };

    builder
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(AppState {
            mihomo: Mutex::new(MihomoManager::new(mihomo_path)),
            go_client: Mutex::new(GoClientManager::new(go_client_path)),
            external_engine: std::sync::atomic::AtomicBool::new(false),
        })
        .setup(|app| {
            let _ = &app;
            #[cfg(desktop)]
            {
                let state: tauri::State<AppState> = app.state();
                if let Ok(gc) = state.go_client.lock() {
                    gc.kill_all_by_name();
                }
                if let Ok(m) = state.mihomo.lock() {
                    m.kill_all_by_name();
                }
                restore_system_proxy(app.handle());
                #[allow(clippy::drop_non_drop)]
                drop(state);
                if let Err(e) = ensure_controller_secret(app.handle()) {
                    eprintln!("[secret] {e}");
                }
                if let Ok(settings) = get_app_settings(app.handle().clone()) {
                    if let Err(e) = sync_autostart(app.handle(), settings.auto_connect) {
                        eprintln!("[autostart] {e}");
                    }
                }
            }
            #[cfg(target_os = "android")]
            {
                app.handle().plugin(tauri_plugin_barcode_scanner::init())?;
            }

            #[cfg(desktop)]
            {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.unmaximize();
                    let _ = win.set_size(tauri::LogicalSize::new(1200.0, 600.0));
                    let _ = win.center();
                }
            }

            #[cfg(desktop)]
            {
                use tauri::menu::{Menu, MenuItem};
                use tauri::tray::TrayIconBuilder;

                let show = MenuItem::with_id(app, "show", "Открыть", true, None::<&str>)?;
                let quit = MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&show, &quit])?;

                let mut tray = TrayIconBuilder::new()
                    .tooltip("Whisp")
                    .menu(&menu)
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "show" => show_main_window(app),
                        "quit" => stop_sidecars_and_exit(app.clone()),
                        _ => {}
                    });
                if let Some(icon) = app.default_window_icon().cloned() {
                    tray = tray.icon(icon);
                }
                tray.build(app)?;
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                #[cfg(desktop)]
                {
                    let _ = window.hide();
                }
                #[cfg(target_os = "android")]
                {
                    stop_sidecars_and_exit(window.app_handle().clone());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            read_clipboard_text,
            list_processes,
            get_app_settings,
            save_app_setting,
            connect,
            disconnect,
            get_status,
            get_tunnel_state,
            get_system_info,
            open_config_dir,
            open_url,
            get_routing_rules,
            save_routing_rules,
            get_vpn_log,
            read_vpn_log_history,
            clear_vpn_log,
            get_blocklist,
            save_blocklist,
            get_subscriptions,
            add_subscription,
            import_subscriptions,
            refresh_subscription,
            delete_subscription,
            rename_subscription,
            delete_sub_key,
            check_subscription_update,
            ping_key,
            vpn_lockdown,
            open_vpn_settings,
            set_app_language,
            check_for_updates,
            install_update,
        ])
        .run(context)
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    #[test]
    fn app_version_is_read_from_tauri_conf() {
        let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
            .expect("tauri.conf.json must be valid JSON");
        let declared = conf["version"]
            .as_str()
            .expect("tauri.conf.json must declare a version");
        assert_eq!(context.package_info().version.to_string(), declared);
    }

    #[cfg(not(windows))]
    #[test]
    fn echo_reply_is_read_past_a_bsd_ip_header() {
        assert_eq!(super::ip_header_len(0x45), 20);
        assert_eq!(super::ip_header_len(0x46), 24);
        assert_eq!(super::ip_header_len(super::ICMP_ECHO_REPLY_TYPE), 0);
    }

    #[test]
    #[cfg_attr(
        target_os = "linux",
        ignore = "needs net.ipv4.ping_group_range to cover this user; run docker/run-checks.sh"
    )]
    fn icmp_echo_is_answered_by_loopback() {
        let rtt = super::icmp_echo(std::net::Ipv4Addr::LOCALHOST);
        assert!(rtt.is_ok(), "{rtt:?}");
    }

    #[test]
    fn release_assets_are_read_from_the_site_page() {
        let page = r#"<li><a href="/nekoskin/Whisp/releases/download/v0.0.10/Whisp_0.0.10_x64-setup.exe" rel="nofollow">
<a href="/nekoskin/Whisp/releases/download/v0.0.10/app-universal-debug.apk" rel="nofollow">
<a href="/nekoskin/Whisp/archive/refs/tags/v0.0.10.zip" rel="nofollow">"#;
        let release = super::release_from_assets_page("v0.0.10", page);
        assert_eq!(release["tag_name"], "v0.0.10");
        assert_eq!(
            release["html_url"],
            "https://github.com/nekoskin/Whisp/releases/tag/v0.0.10"
        );
        let assets: Vec<(&str, &str)> = release["assets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| {
                (
                    a["name"].as_str().unwrap(),
                    a["browser_download_url"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            assets,
            [
                (
                    "Whisp_0.0.10_x64-setup.exe",
                    "https://github.com/nekoskin/Whisp/releases/download/v0.0.10/Whisp_0.0.10_x64-setup.exe"
                ),
                (
                    "app-universal-debug.apk",
                    "https://github.com/nekoskin/Whisp/releases/download/v0.0.10/app-universal-debug.apk"
                ),
            ]
        );
    }

    #[test]
    fn macos_apps_are_listed_once_with_their_helpers() {
        let ps = "    1 /sbin/launchd\n\
  512 /Applications/Google Chrome.app/Contents/MacOS/Google Chrome\n\
  515 /Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Framework.framework/Helpers/Google Chrome Helper.app/Contents/MacOS/Google Chrome Helper\n\
  700 /usr/bin/curl\n\
  801 Telegram\n\
  not-a-pid /bin/ls\n";
        let listed: Vec<(String, String)> = super::macos_processes(ps)
            .into_iter()
            .map(|p| (p.name, p.label))
            .collect();
        let expected = [
            ("launchd", "launchd"),
            ("/Applications/Google Chrome.app", "Google Chrome"),
            ("curl", "curl"),
            ("Telegram", "Telegram"),
        ];
        assert_eq!(
            listed,
            expected
                .iter()
                .map(|(name, label)| (name.to_string(), label.to_string()))
                .collect::<Vec<_>>()
        );
    }
}
