//! Отправка intent на WhispVpnService.ACTION_START / ACTION_STOP.
//!
//! Без этого Connect-кнопка ни к чему не подключена: Kotlin-сервис написан,
//! permission в манифесте есть, но никто не запускает сервис.
//!
//! Контекст и JavaVM достаём через ndk_context — Tauri 2 на Android их
//! инициализирует в своём JNI_OnLoad, так что здесь просто читаем готовое.
//!
//! Скомпилируется только под `target_os = "android"`. На desktop тушится
//! через cfg в lib.rs.

use jni::objects::{JObject, JValue};
use jni::JavaVM;
use std::sync::atomic::{AtomicBool, Ordering};

static VPN_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn is_vpn_active() -> bool {
    VPN_ACTIVE.load(Ordering::SeqCst)
}
pub fn set_vpn_active(v: bool) {
    VPN_ACTIVE.store(v, Ordering::SeqCst);
}

const SERVICE_CLASS: &str = "com/whispera/whisp/WhispVpnService";
const ACTION_START: &str = "com.whispera.whisp.ACTION_VPN_START";
const ACTION_STOP: &str = "com.whispera.whisp.ACTION_VPN_STOP";
const EXTRA_RULES_JSON: &str = "com.whispera.whisp.EXTRA_RULES_JSON";
const EXTRA_CONN_KEY: &str = "com.whispera.whisp.EXTRA_CONN_KEY";
const EXTRA_VPN_DNS: &str = "com.whispera.whisp.EXTRA_VPN_DNS";
const EXTRA_IPV6: &str = "com.whispera.whisp.EXTRA_IPV6";
const EXTRA_HWID: &str = "com.whispera.whisp.EXTRA_HWID";
const EXTRA_TLS_FINGERPRINT: &str = "com.whispera.whisp.EXTRA_TLS_FINGERPRINT";
const EXTRA_MIXED_PORT: &str = "com.whispera.whisp.EXTRA_MIXED_PORT";
const EXTRA_ALLOW_LAN: &str = "com.whispera.whisp.EXTRA_ALLOW_LAN";
const EXTRA_SOCKS_USER: &str = "com.whispera.whisp.EXTRA_SOCKS_USER";
const EXTRA_SOCKS_PASS: &str = "com.whispera.whisp.EXTRA_SOCKS_PASS";
const EXTRA_DNS_MODE: &str = "com.whispera.whisp.EXTRA_DNS_MODE";
const EXTRA_DNS_STRATEGY: &str = "com.whispera.whisp.EXTRA_DNS_STRATEGY";
const EXTRA_MTU: &str = "com.whispera.whisp.EXTRA_MTU";
const EXTRA_TLS_FRAGMENT: &str = "com.whispera.whisp.EXTRA_TLS_FRAGMENT";
const EXTRA_AUTO_CONNECT: &str = "com.whispera.whisp.EXTRA_AUTO_CONNECT";
const EXTRA_TUN_STACK: &str = "com.whispera.whisp.EXTRA_TUN_STACK";
const EXTRA_QUIC: &str = "com.whispera.whisp.EXTRA_QUIC";
const EXTRA_SERVER_IPV6: &str = "com.whispera.whisp.EXTRA_SERVER_IPV6";

fn vm_and_ctx() -> Result<(JavaVM, *mut std::ffi::c_void), String> {
    // SAFETY: ndk_context::android_context() возвращает указатели,
    // которые валидны до выгрузки приложения. JavaVM::from_raw — unsafe,
    // потому что нужно гарантировать, что ndk_context уже инициализирован
    // (Tauri 2 это делает).
    let ctx = ndk_context::android_context();
    if ctx.vm().is_null() || ctx.context().is_null() {
        return Err("ndk_context not initialized (no VM/Context)".to_string());
    }
    let vm = unsafe { JavaVM::from_raw(ctx.vm() as *mut _) }
        .map_err(|e| format!("JavaVM::from_raw: {}", e))?;
    Ok((vm, ctx.context()))
}

fn send_action(
    action: &str,
    rules_json: Option<&str>,
    conn_key: Option<&str>,
    vpn_dns: Option<&str>,
    ipv6: Option<bool>,
    hwid: Option<bool>,
    tls_fingerprint: Option<&str>,
    mixed_port: Option<u16>,
    allow_lan: Option<bool>,
    socks_user: Option<&str>,
    socks_pass: Option<&str>,
    dns_mode: Option<&str>,
    dns_strategy: Option<&str>,
    mtu: Option<u16>,
    tls_fragment: Option<bool>,
    auto_connect: Option<bool>,
    tun_stack: Option<&str>,
    quic: Option<bool>,
    server_ipv6: Option<&str>,
    _stop: bool,
) -> Result<(), String> {
    let (vm, ctx_ptr) = vm_and_ctx()?;
    let mut env = vm
        .attach_current_thread()
        .map_err(|e| format!("attach_current_thread: {}", e))?;

    let context = unsafe { JObject::from_raw(ctx_ptr as jni::sys::jobject) };

    // ClassLoader app_loader = context.getClassLoader()
    // Без этого find_class из non-Java thread'а не находит наши Kotlin-классы:
    // JVM использует system loader, а наши классы только в app's DEX.
    let app_loader = env
        .call_method(&context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
        .and_then(|v| v.l())
        .map_err(|e| format!("getClassLoader: {}", e))?;
    let svc_name = env
        .new_string(SERVICE_CLASS.replace('/', "."))
        .map_err(|e| format!("new_string svc: {}", e))?;
    let svc_class = env
        .call_method(
            &app_loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&svc_name.into())],
        )
        .and_then(|v| v.l())
        .map_err(|e| format!("loadClass {}: {}", SERVICE_CLASS, e))?;

    let intent_class = env
        .find_class("android/content/Intent")
        .map_err(|e| format!("find_class Intent: {}", e))?;
    let intent = env
        .new_object(
            &intent_class,
            "(Landroid/content/Context;Ljava/lang/Class;)V",
            &[JValue::Object(&context), JValue::Object(&svc_class)],
        )
        .map_err(|e| format!("new Intent: {}", e))?;

    // intent.setAction(action)
    let action_jstr = env
        .new_string(action)
        .map_err(|e| format!("new_string action: {}", e))?;
    env.call_method(
        &intent,
        "setAction",
        "(Ljava/lang/String;)Landroid/content/Intent;",
        &[JValue::Object(&action_jstr.into())],
    )
    .map_err(|e| format!("setAction: {}", e))?;

    let mut put_extra = |k: &str, v: &str| -> Result<(), String> {
        let kj = env.new_string(k).map_err(|e| e.to_string())?;
        let vj = env.new_string(v).map_err(|e| e.to_string())?;
        env.call_method(
            &intent,
            "putExtra",
            "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/Intent;",
            &[JValue::Object(&kj.into()), JValue::Object(&vj.into())],
        )
        .map_err(|e| format!("putExtra {}: {}", k, e))?;
        Ok(())
    };
    if let Some(rules) = rules_json {
        put_extra(EXTRA_RULES_JSON, rules)?;
    }
    if let Some(key) = conn_key {
        put_extra(EXTRA_CONN_KEY, key)?;
    }
    if let Some(dns) = vpn_dns {
        put_extra(EXTRA_VPN_DNS, dns)?;
    }
    if let Some(v6) = ipv6 {
        put_extra(EXTRA_IPV6, if v6 { "1" } else { "0" })?;
    }
    if let Some(h) = hwid {
        put_extra(EXTRA_HWID, if h { "1" } else { "0" })?;
    }
    if let Some(s) = tun_stack {
        put_extra(EXTRA_TUN_STACK, s)?;
    }
    if let Some(q) = quic {
        put_extra(EXTRA_QUIC, if q { "1" } else { "0" })?;
    }
    if let Some(v6) = server_ipv6 {
        put_extra(EXTRA_SERVER_IPV6, v6)?;
    }
    if let Some(fp) = tls_fingerprint {
        put_extra(EXTRA_TLS_FINGERPRINT, fp)?;
    }
    if let Some(p) = mixed_port {
        put_extra(EXTRA_MIXED_PORT, &p.to_string())?;
    }
    if let Some(lan) = allow_lan {
        put_extra(EXTRA_ALLOW_LAN, if lan { "1" } else { "0" })?;
    }
    if let Some(u) = socks_user {
        put_extra(EXTRA_SOCKS_USER, u)?;
    }
    if let Some(pw) = socks_pass {
        put_extra(EXTRA_SOCKS_PASS, pw)?;
    }
    if let Some(dm) = dns_mode {
        put_extra(EXTRA_DNS_MODE, dm)?;
    }
    if let Some(ds) = dns_strategy {
        put_extra(EXTRA_DNS_STRATEGY, ds)?;
    }
    if let Some(m) = mtu {
        put_extra(EXTRA_MTU, &m.to_string())?;
    }
    if let Some(tf) = tls_fragment {
        put_extra(EXTRA_TLS_FRAGMENT, if tf { "1" } else { "0" })?;
    }
    if let Some(ac) = auto_connect {
        put_extra(EXTRA_AUTO_CONNECT, if ac { "1" } else { "0" })?;
    }

    // Для старта и для стопа используем startForegroundService:
    // stopService() не вызывает onStartCommand, поэтому ACTION_STOP не доходит.
    // startForegroundService → onStartCommand → наш when(action) обрабатывает оба кейса.
    env.call_method(
        &context,
        "startForegroundService",
        "(Landroid/content/Intent;)Landroid/content/ComponentName;",
        &[JValue::Object(&intent)],
    )
    .map_err(|e| format!("startForegroundService: {}", e))?;

    Ok(())
}

pub fn start_vpn_service(
    rules_json: &str,
    conn_key: &str,
    vpn_dns: &str,
    ipv6: bool,
    hwid: bool,
    tls_fingerprint: &str,
    mixed_port: u16,
    allow_lan: bool,
    socks_user: &str,
    socks_pass: &str,
    dns_mode: &str,
    dns_strategy: &str,
    mtu: u16,
    tls_fragment: bool,
    auto_connect: bool,
    tun_stack: &str,
    quic: bool,
    server_ipv6: &str,
) -> Result<(), String> {
    let dns = if vpn_dns.is_empty() {
        None
    } else {
        Some(vpn_dns)
    };
    let fp = if tls_fingerprint.is_empty() {
        None
    } else {
        Some(tls_fingerprint)
    };
    let user = if socks_user.is_empty() {
        None
    } else {
        Some(socks_user)
    };
    let pass = if socks_pass.is_empty() {
        None
    } else {
        Some(socks_pass)
    };
    let dm = if dns_mode.is_empty() {
        None
    } else {
        Some(dns_mode)
    };
    let ds = if dns_strategy.is_empty() {
        None
    } else {
        Some(dns_strategy)
    };
    let mtu_opt = if mtu > 0 { Some(mtu) } else { None };
    let r = send_action(
        ACTION_START,
        Some(rules_json),
        Some(conn_key),
        dns,
        Some(ipv6),
        Some(hwid),
        fp,
        Some(mixed_port),
        Some(allow_lan),
        user,
        pass,
        dm,
        ds,
        mtu_opt,
        Some(tls_fragment),
        Some(auto_connect),
        Some(tun_stack),
        Some(quic),
        Some(server_ipv6),
        false,
    );
    if r.is_ok() {
        set_vpn_active(true);
    }
    r
}

pub fn stop_vpn_service() -> Result<(), String> {
    let r = send_action(
        ACTION_STOP,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        true,
    );
    set_vpn_active(false);
    r
}

const PREP_CLASS: &str = "com/whispera/whisp/WhispVpnPrep";

/// Возвращает true если VPN permission уже выдан, false — нужно вызвать request_vpn_permission.
pub fn is_lockdown_enabled() -> Result<bool, String> {
    prep_static_bool("isLockdown")
}

pub fn open_vpn_settings() -> Result<i32, String> {
    prep_static_int("openVpnSettings")
}

pub fn set_autostart(on: bool) -> Result<bool, String> {
    let (vm, ctx_ptr) = vm_and_ctx()?;
    let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
    let cls = prep_class(&mut env, ctx_ptr)?;
    env.call_static_method(
        &cls,
        "setAutostart",
        "(Z)Z",
        &[JValue::Bool(if on { 1 } else { 0 })],
    )
    .and_then(|v| v.z())
    .map_err(|e| format!("setAutostart: {}", e))
}

pub fn set_language(lang: &str) -> Result<bool, String> {
    let (vm, ctx_ptr) = vm_and_ctx()?;
    let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
    let cls = prep_class(&mut env, ctx_ptr)?;
    let jlang = env.new_string(lang).map_err(|e| e.to_string())?;
    env.call_static_method(
        &cls,
        "setLanguage",
        "(Ljava/lang/String;)Z",
        &[JValue::Object(&jlang.into())],
    )
    .and_then(|v| v.z())
    .map_err(|e| format!("setLanguage: {}", e))
}

fn prep_class<'a>(
    env: &mut jni::JNIEnv<'a>,
    ctx_ptr: *mut std::ffi::c_void,
) -> Result<jni::objects::JClass<'a>, String> {
    let context = unsafe { JObject::from_raw(ctx_ptr as jni::sys::jobject) };
    let app_loader = env
        .call_method(&context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
        .and_then(|v| v.l())
        .map_err(|e| format!("getClassLoader: {}", e))?;
    let cls_name = env
        .new_string(PREP_CLASS.replace('/', "."))
        .map_err(|e| e.to_string())?;
    let cls = env
        .call_method(
            &app_loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&cls_name.into())],
        )
        .and_then(|v| v.l())
        .map_err(|e| format!("loadClass {}: {}", PREP_CLASS, e))?;
    Ok(cls.into())
}

fn prep_static_bool(name: &str) -> Result<bool, String> {
    let (vm, ctx_ptr) = vm_and_ctx()?;
    let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
    let cls = prep_class(&mut env, ctx_ptr)?;
    env.call_static_method(&cls, name, "()Z", &[])
        .and_then(|v| v.z())
        .map_err(|e| format!("{}: {}", name, e))
}

fn prep_static_int(name: &str) -> Result<i32, String> {
    let (vm, ctx_ptr) = vm_and_ctx()?;
    let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
    let cls = prep_class(&mut env, ctx_ptr)?;
    env.call_static_method(&cls, name, "()I", &[])
        .and_then(|v| v.i())
        .map_err(|e| format!("{}: {}", name, e))
}

pub fn is_vpn_prepared() -> Result<bool, String> {
    let (vm, ctx_ptr) = vm_and_ctx()?;
    let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
    let context = unsafe { JObject::from_raw(ctx_ptr as jni::sys::jobject) };
    let app_loader = env
        .call_method(&context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
        .and_then(|v| v.l())
        .map_err(|e| format!("getClassLoader: {}", e))?;
    let cls_name = env
        .new_string(PREP_CLASS.replace('/', "."))
        .map_err(|e| e.to_string())?;
    let cls = env
        .call_method(
            &app_loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&cls_name.into())],
        )
        .and_then(|v| v.l())
        .map_err(|e| format!("loadClass {}: {}", PREP_CLASS, e))?;
    let cls_class: jni::objects::JClass = cls.into();
    let result = env
        .call_static_method(&cls_class, "isPrepared", "()Z", &[])
        .and_then(|v| v.z())
        .map_err(|e| format!("isPrepared: {}", e))?;
    Ok(result)
}

/// Открывает системный диалог 'Allow VPN'. Возвращает Ok(true) если диалог
/// показан/уже approved, Err при отсутствии MainActivity.
pub fn request_vpn_permission() -> Result<i32, String> {
    let (vm, ctx_ptr) = vm_and_ctx()?;
    let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
    let context = unsafe { JObject::from_raw(ctx_ptr as jni::sys::jobject) };
    let app_loader = env
        .call_method(&context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
        .and_then(|v| v.l())
        .map_err(|e| format!("getClassLoader: {}", e))?;
    let cls_name = env
        .new_string(PREP_CLASS.replace('/', "."))
        .map_err(|e| e.to_string())?;
    let cls = env
        .call_method(
            &app_loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&cls_name.into())],
        )
        .and_then(|v| v.l())
        .map_err(|e| format!("loadClass {}: {}", PREP_CLASS, e))?;
    let cls_class: jni::objects::JClass = cls.into();
    let result = env
        .call_static_method(&cls_class, "requestPermission", "()I", &[])
        .and_then(|v| v.i())
        .map_err(|e| format!("requestPermission: {}", e))?;
    Ok(result)
}

/// Сохраняет параметры VPN в WhispVpnPrep.savePending() для авто-запуска
/// после onActivityResult (пользователь разрешил VPN).
pub fn save_pending_start(
    rules_json: &str,
    conn_key: &str,
    vpn_dns: &str,
    ipv6: bool,
    hwid: bool,
    tls_fingerprint: &str,
    mixed_port: u16,
    allow_lan: bool,
    socks_user: &str,
    socks_pass: &str,
    dns_mode: &str,
    dns_strategy: &str,
    mtu: u16,
    tls_fragment: bool,
    auto_connect: bool,
    tun_stack: &str,
    quic: bool,
    server_ipv6: &str,
) -> Result<(), String> {
    let (vm, ctx_ptr) = vm_and_ctx()?;
    let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
    let context = unsafe { JObject::from_raw(ctx_ptr as jni::sys::jobject) };
    let app_loader = env
        .call_method(&context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
        .and_then(|v| v.l())
        .map_err(|e| format!("getClassLoader: {}", e))?;
    let cls_name = env
        .new_string(PREP_CLASS.replace('/', "."))
        .map_err(|e| e.to_string())?;
    let cls = env
        .call_method(
            &app_loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&cls_name.into())],
        )
        .and_then(|v| v.l())
        .map_err(|e| format!("loadClass WhispVpnPrep: {}", e))?;
    let cls_class: jni::objects::JClass = cls.into();
    let j_rules = env.new_string(rules_json).map_err(|e| e.to_string())?;
    let j_key = env.new_string(conn_key).map_err(|e| e.to_string())?;
    let j_dns = env.new_string(vpn_dns).map_err(|e| e.to_string())?;
    let j_fp = env.new_string(tls_fingerprint).map_err(|e| e.to_string())?;
    let j_user = env.new_string(socks_user).map_err(|e| e.to_string())?;
    let j_pass = env.new_string(socks_pass).map_err(|e| e.to_string())?;
    let j_mode = env.new_string(dns_mode).map_err(|e| e.to_string())?;
    let j_strat = env.new_string(dns_strategy).map_err(|e| e.to_string())?;
    let j_stack = env.new_string(tun_stack).map_err(|e| e.to_string())?;
    let j_server_ipv6 = env.new_string(server_ipv6).map_err(|e| e.to_string())?;
    env.call_static_method(
        &cls_class,
        "savePending",
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;ZZLjava/lang/String;IZLjava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;IZZLjava/lang/String;ZLjava/lang/String;)V",
        &[
            JValue::Object(&j_rules.into()),
            JValue::Object(&j_key.into()),
            JValue::Object(&j_dns.into()),
            JValue::Bool(if ipv6 { 1 } else { 0 }),
            JValue::Bool(if hwid { 1 } else { 0 }),
            JValue::Object(&j_fp.into()),
            JValue::Int(mixed_port as i32),
            JValue::Bool(if allow_lan { 1 } else { 0 }),
            JValue::Object(&j_user.into()),
            JValue::Object(&j_pass.into()),
            JValue::Object(&j_mode.into()),
            JValue::Object(&j_strat.into()),
            JValue::Int(mtu as i32),
            JValue::Bool(if tls_fragment { 1 } else { 0 }),
            JValue::Bool(if auto_connect { 1 } else { 0 }),
            JValue::Object(&j_stack.into()),
            JValue::Bool(if quic { 1 } else { 0 }),
            JValue::Object(&j_server_ipv6.into()),
        ],
    )
    .map_err(|e| format!("savePending: {}", e))?;
    Ok(())
}

pub fn is_vpn_service_running() -> bool {
    let Ok((vm, ctx_ptr)) = vm_and_ctx() else {
        return false;
    };
    let Ok(mut env) = vm.attach_current_thread() else {
        return false;
    };
    let context = unsafe { JObject::from_raw(ctx_ptr as jni::sys::jobject) };
    let Ok(loader) = env
        .call_method(&context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
        .and_then(|v| v.l())
    else {
        return false;
    };
    let Ok(cls_name) = env.new_string(SERVICE_CLASS.replace('/', ".")) else {
        return false;
    };
    let Ok(cls) = env
        .call_method(
            &loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&cls_name.into())],
        )
        .and_then(|v| v.l())
    else {
        return false;
    };
    let cls_class: jni::objects::JClass = cls.into();
    env.call_static_method(
        &cls_class,
        "isActuallyRunning",
        "(Landroid/content/Context;)Z",
        &[JValue::Object(&context)],
    )
    .and_then(|v| v.z())
    .unwrap_or(false)
}

/// Открывает URL через Android Intent.ACTION_VIEW.
pub fn open_url_android(url: &str) -> Result<(), String> {
    let (vm, ctx_ptr) = vm_and_ctx()?;
    let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
    let context = unsafe { JObject::from_raw(ctx_ptr as jni::sys::jobject) };

    let uri_class = env
        .find_class("android/net/Uri")
        .map_err(|e| format!("find Uri: {}", e))?;
    let j_url = env.new_string(url).map_err(|e| e.to_string())?;
    let uri = env
        .call_static_method(
            &uri_class,
            "parse",
            "(Ljava/lang/String;)Landroid/net/Uri;",
            &[JValue::Object(&j_url.into())],
        )
        .and_then(|v| v.l())
        .map_err(|e| format!("Uri.parse: {}", e))?;

    let intent_class = env
        .find_class("android/content/Intent")
        .map_err(|e| format!("find Intent: {}", e))?;
    let action_view = env
        .new_string("android.intent.action.VIEW")
        .map_err(|e| e.to_string())?;
    let intent = env
        .new_object(
            &intent_class,
            "(Ljava/lang/String;Landroid/net/Uri;)V",
            &[JValue::Object(&action_view.into()), JValue::Object(&uri)],
        )
        .map_err(|e| format!("new Intent: {}", e))?;

    // FLAG_ACTIVITY_NEW_TASK required when starting Activity from non-Activity context
    env.call_method(
        &intent,
        "addFlags",
        "(I)Landroid/content/Intent;",
        &[JValue::Int(0x10000000)],
    ) // FLAG_ACTIVITY_NEW_TASK
    .map_err(|e| format!("addFlags: {}", e))?;

    env.call_method(
        &context,
        "startActivity",
        "(Landroid/content/Intent;)V",
        &[JValue::Object(&intent)],
    )
    .map_err(|e| format!("startActivity: {}", e))?;

    Ok(())
}

/// Читает текст из системного буфера обмена (Android ClipboardManager).
/// Нужно потому, что tauri-plugin-clipboard-manager readText на Android не
/// реализован ("Clipboard content reader not implemented"), а WebView
/// navigator.clipboard.readText блокируется политикой (NotAllowedError).
pub fn read_clipboard() -> Result<String, String> {
    let (vm, ctx_ptr) = vm_and_ctx()?;
    let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
    let context = unsafe { JObject::from_raw(ctx_ptr as jni::sys::jobject) };

    let svc_name = env.new_string("clipboard").map_err(|e| e.to_string())?;
    let clipboard = env
        .call_method(
            &context,
            "getSystemService",
            "(Ljava/lang/String;)Ljava/lang/Object;",
            &[JValue::Object(&svc_name.into())],
        )
        .and_then(|v| v.l())
        .map_err(|e| format!("getSystemService clipboard: {}", e))?;
    if clipboard.is_null() {
        return Ok(String::new());
    }

    let clip = env
        .call_method(
            &clipboard,
            "getPrimaryClip",
            "()Landroid/content/ClipData;",
            &[],
        )
        .and_then(|v| v.l())
        .map_err(|e| format!("getPrimaryClip: {}", e))?;
    if clip.is_null() {
        return Ok(String::new());
    }

    let count = env
        .call_method(&clip, "getItemCount", "()I", &[])
        .and_then(|v| v.i())
        .map_err(|e| format!("getItemCount: {}", e))?;
    if count <= 0 {
        return Ok(String::new());
    }

    let item = env
        .call_method(
            &clip,
            "getItemAt",
            "(I)Landroid/content/ClipData$Item;",
            &[JValue::Int(0)],
        )
        .and_then(|v| v.l())
        .map_err(|e| format!("getItemAt: {}", e))?;

    let text = env
        .call_method(
            &item,
            "coerceToText",
            "(Landroid/content/Context;)Ljava/lang/CharSequence;",
            &[JValue::Object(&context)],
        )
        .and_then(|v| v.l())
        .map_err(|e| format!("coerceToText: {}", e))?;
    if text.is_null() {
        return Ok(String::new());
    }

    let s = env
        .call_method(&text, "toString", "()Ljava/lang/String;", &[])
        .and_then(|v| v.l())
        .map_err(|e| format!("toString: {}", e))?;
    let jstr: jni::objects::JString = s.into();
    let rust: String = env.get_string(&jstr).map_err(|e| e.to_string())?.into();
    Ok(rust)
}
