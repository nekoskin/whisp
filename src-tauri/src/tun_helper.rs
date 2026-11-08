use std::ffi::CString;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

pub const HELPER_FLAG: &str = "--tun-helper";

const LABEL: &str = "com.whispera.whisp.tun-helper";
const INSTALL_DIR: &str = "/Library/PrivilegedHelperTools";
const LAUNCH_DAEMONS_DIR: &str = "/Library/LaunchDaemons";
const LOG_DIR: &str = "/Library/Logs";
const SOCKET_DIR: &str = "/var/run";
const INSTALL_OWNER: &str = "root";
const INSTALL_GROUP: &str = "wheel";
const BINARY_MODE: &str = "755";
const PLIST_MODE: &str = "644";
const QUARANTINE_ATTRIBUTE: &str = "com.apple.quarantine";

const ARG_SOCKET: &str = "--socket";
const ARG_OWNER_UID: &str = "--owner-uid";
const ARG_MIHOMO: &str = "--mihomo";
const ARG_HOME: &str = "--home";
const ARG_CONFIG: &str = "--config";

const REQUEST_START: &str = "start";
const REQUEST_STOP: &str = "stop";
const REQUEST_STATUS: &str = "status";
const REPLY_OK: &str = "ok";
const REPLY_RUNNING: &str = "running";
const REPLY_STOPPED: &str = "stopped";
const REPLY_DENIED: &str = "denied";
const REPLY_ERROR: &str = "error";

const SOCKET_MODE: u32 = 0o600;
const STOP_GRACE: Duration = Duration::from_secs(3);
const STOP_POLL: Duration = Duration::from_millis(50);
const REPLY_TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(target_os = "macos")]
const INSTALL_WAIT: Duration = Duration::from_secs(15);
#[cfg(target_os = "macos")]
const INSTALL_POLL: Duration = Duration::from_millis(200);
#[cfg(target_os = "macos")]
const STATE_DIR: &str = "/Library/Application Support";
#[cfg(target_os = "macos")]
const DNS_STATE_FILE: &str = "dns-before-tun";

#[derive(Clone, Debug, PartialEq)]
pub struct HelperSettings {
    pub socket: PathBuf,
    pub owner_uid: u32,
    pub mihomo: PathBuf,
    pub home: PathBuf,
    pub config: PathBuf,
}

impl HelperSettings {
    #[cfg(target_os = "macos")]
    pub fn for_current_user(config: &Path) -> Self {
        Self {
            socket: socket_path(),
            owner_uid: current_uid(),
            mihomo: installed_mihomo(),
            home: config.parent().unwrap_or(config).to_path_buf(),
            config: config.to_path_buf(),
        }
    }

    fn to_args(&self) -> Vec<String> {
        vec![
            ARG_SOCKET.to_string(),
            self.socket.to_string_lossy().into_owned(),
            ARG_OWNER_UID.to_string(),
            self.owner_uid.to_string(),
            ARG_MIHOMO.to_string(),
            self.mihomo.to_string_lossy().into_owned(),
            ARG_HOME.to_string(),
            self.home.to_string_lossy().into_owned(),
            ARG_CONFIG.to_string(),
            self.config.to_string_lossy().into_owned(),
        ]
    }

    fn from_args(args: &[String]) -> Result<Self, String> {
        let value = |name: &str| {
            args.iter()
                .position(|arg| arg == name)
                .and_then(|i| args.get(i + 1))
                .ok_or_else(|| format!("missing {name}"))
        };
        Ok(Self {
            socket: PathBuf::from(value(ARG_SOCKET)?),
            owner_uid: value(ARG_OWNER_UID)?
                .parse()
                .map_err(|_| format!("{ARG_OWNER_UID} must be a number"))?,
            mihomo: PathBuf::from(value(ARG_MIHOMO)?),
            home: PathBuf::from(value(ARG_HOME)?),
            config: PathBuf::from(value(ARG_CONFIG)?),
        })
    }
}

pub fn socket_path() -> PathBuf {
    Path::new(SOCKET_DIR).join(format!("{LABEL}.sock"))
}

fn installed_helper() -> PathBuf {
    Path::new(INSTALL_DIR).join(LABEL)
}

fn installed_mihomo() -> PathBuf {
    Path::new(INSTALL_DIR).join(format!("{LABEL}.mihomo"))
}

fn installed_plist() -> PathBuf {
    Path::new(LAUNCH_DAEMONS_DIR).join(format!("{LABEL}.plist"))
}

fn current_uid() -> u32 {
    unsafe { libc::getuid() }
}

#[cfg(target_os = "macos")]
pub fn run_if_requested() -> Option<Result<(), String>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some(HELPER_FLAG) {
        return None;
    }
    Some(HelperSettings::from_args(&args[1..]).and_then(|settings| serve(&settings)))
}

#[derive(Default)]
struct Mihomo {
    child: Option<Child>,
    owner: u64,
}

fn lock(mihomo: &Mutex<Mihomo>) -> MutexGuard<'_, Mihomo> {
    mihomo.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(target_os = "macos")]
fn dns_state() -> PathBuf {
    Path::new(STATE_DIR).join(LABEL).join(DNS_STATE_FILE)
}

#[cfg(target_os = "macos")]
fn point_dns_at_tunnel(settings: &HelperSettings) {
    if let Err(e) = crate::system_dns::point_at_tunnel(&settings.config, &dns_state()) {
        eprintln!("dns: {e}");
    }
}

#[cfg(not(target_os = "macos"))]
fn point_dns_at_tunnel(_settings: &HelperSettings) {}

#[cfg(target_os = "macos")]
fn restore_dns() {
    if let Err(e) = crate::system_dns::restore(&dns_state()) {
        eprintln!("dns: {e}");
    }
}

#[cfg(not(target_os = "macos"))]
fn restore_dns() {}

fn stop_mihomo(state: &mut Mihomo) {
    terminate(&mut state.child);
    restore_dns();
}

fn serve(settings: &HelperSettings) -> Result<(), String> {
    let listener = bind(settings).map_err(|e| format!("{}: {e}", settings.socket.display()))?;
    restore_dns();
    let mihomo = Arc::new(Mutex::new(Mihomo::default()));
    let next_connection = AtomicU64::new(1);
    for stream in listener.incoming().flatten() {
        let connection = next_connection.fetch_add(1, Ordering::Relaxed);
        let settings = settings.clone();
        let mihomo = Arc::clone(&mihomo);
        std::thread::spawn(move || serve_connection(stream, connection, &settings, &mihomo));
    }
    Ok(())
}

fn bind(settings: &HelperSettings) -> std::io::Result<UnixListener> {
    let _ = std::fs::remove_file(&settings.socket);
    let listener = UnixListener::bind(&settings.socket)?;
    let path = CString::new(settings.socket.as_os_str().as_bytes())?;
    if unsafe { libc::chown(path.as_ptr(), settings.owner_uid, libc::gid_t::MAX) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    std::fs::set_permissions(
        &settings.socket,
        std::fs::Permissions::from_mode(SOCKET_MODE),
    )?;
    Ok(listener)
}

fn serve_connection(
    stream: UnixStream,
    connection: u64,
    settings: &HelperSettings,
    mihomo: &Mutex<Mihomo>,
) {
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    if peer_uid(&stream) != Some(settings.owner_uid) {
        let _ = writeln!(writer, "{REPLY_DENIED}");
        return;
    }
    for line in BufReader::new(stream).lines() {
        let Ok(request) = line else {
            break;
        };
        let reply = match request.trim() {
            REQUEST_START => start_mihomo(settings, connection, mihomo),
            REQUEST_STOP => {
                stop_mihomo(&mut lock(mihomo));
                REPLY_OK.to_string()
            }
            REQUEST_STATUS => mihomo_status(mihomo).to_string(),
            other => format!("{REPLY_ERROR} unknown request {other:?}"),
        };
        if writeln!(writer, "{reply}").is_err() {
            break;
        }
    }
    let mut state = lock(mihomo);
    if state.owner == connection {
        stop_mihomo(&mut state);
    }
}

fn start_mihomo(settings: &HelperSettings, connection: u64, mihomo: &Mutex<Mihomo>) -> String {
    let mut state = lock(mihomo);
    terminate(&mut state.child);
    let spawned = Command::new(&settings.mihomo)
        .arg("-d")
        .arg(&settings.home)
        .arg("-f")
        .arg(&settings.config)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match spawned {
        Ok(child) => {
            state.child = Some(child);
            state.owner = connection;
            point_dns_at_tunnel(settings);
            REPLY_OK.to_string()
        }
        Err(e) => format!("{REPLY_ERROR} {e}"),
    }
}

fn mihomo_status(mihomo: &Mutex<Mihomo>) -> &'static str {
    let mut state = lock(mihomo);
    let alive = state
        .child
        .as_mut()
        .is_some_and(|child| matches!(child.try_wait(), Ok(None)));
    if !alive {
        state.child = None;
    }
    if alive {
        REPLY_RUNNING
    } else {
        REPLY_STOPPED
    }
}

fn terminate(slot: &mut Option<Child>) {
    let Some(mut child) = slot.take() else {
        return;
    };
    if let Ok(pid) = libc::pid_t::try_from(child.id()) {
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
    }
    let deadline = Instant::now() + STOP_GRACE;
    while Instant::now() < deadline {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(STOP_POLL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(target_os = "macos")]
fn peer_uid(stream: &UnixStream) -> Option<u32> {
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    (unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } == 0).then_some(uid)
}

#[cfg(target_os = "linux")]
fn peer_uid(stream: &UnixStream) -> Option<u32> {
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    (rc == 0).then_some(cred.uid)
}

pub fn request_start(socket: &Path) -> Result<UnixStream, String> {
    let mut stream = connect(socket)?;
    expect_ok(&exchange(&mut stream, REQUEST_START)?)?;
    Ok(stream)
}

pub fn request_stop(stream: &mut UnixStream) -> Result<(), String> {
    expect_ok(&exchange(stream, REQUEST_STOP)?)
}

pub fn is_running(socket: &Path) -> Result<bool, String> {
    let mut stream = connect(socket)?;
    match exchange(&mut stream, REQUEST_STATUS)?.as_str() {
        REPLY_RUNNING => Ok(true),
        REPLY_STOPPED => Ok(false),
        other => Err(describe_failure(other)),
    }
}

fn connect(socket: &Path) -> Result<UnixStream, String> {
    let stream = UnixStream::connect(socket)
        .map_err(|e| format!("TUN helper is not reachable at {}: {e}", socket.display()))?;
    stream
        .set_read_timeout(Some(REPLY_TIMEOUT))
        .map_err(|e| e.to_string())?;
    Ok(stream)
}

fn exchange(stream: &mut UnixStream, request: &str) -> Result<String, String> {
    let sent = writeln!(stream, "{request}");
    let mut reply = String::new();
    let received = BufReader::new(&*stream).read_line(&mut reply);
    if !reply.trim().is_empty() {
        return Ok(reply.trim().to_string());
    }
    sent.and(received.map(|_| ()))
        .map_err(|e| format!("TUN helper: {e}"))?;
    Ok(String::new())
}

fn expect_ok(reply: &str) -> Result<(), String> {
    if reply == REPLY_OK {
        Ok(())
    } else {
        Err(describe_failure(reply))
    }
}

fn describe_failure(reply: &str) -> String {
    match reply {
        REPLY_DENIED => "TUN helper belongs to another user".to_string(),
        "" => "TUN helper closed the connection".to_string(),
        other => format!("TUN helper: {other}"),
    }
}

#[cfg(target_os = "macos")]
pub fn ensure_installed(settings: &HelperSettings, mihomo_source: &Path) -> Result<(), String> {
    let helper_source = std::env::current_exe().map_err(|e| e.to_string())?;
    let plist = launchd_plist(settings, &installed_helper());
    let current = std::fs::read_to_string(installed_plist())
        .is_ok_and(|installed| installed == plist)
        && same_contents(&helper_source, &installed_helper())
        && same_contents(mihomo_source, &installed_mihomo())
        && is_running(&settings.socket).is_ok();
    if current {
        return Ok(());
    }
    let staging = std::env::temp_dir().join(LABEL);
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let staged_plist = staging.join(format!("{LABEL}.plist"));
    std::fs::write(&staged_plist, plist).map_err(|e| e.to_string())?;
    let script = staging.join("install.sh");
    std::fs::write(
        &script,
        install_script(&helper_source, mihomo_source, &staged_plist),
    )
    .map_err(|e| e.to_string())?;
    run_as_administrator(&script)?;
    wait_until_reachable(&settings.socket)
}

#[cfg(target_os = "macos")]
fn same_contents(source: &Path, installed: &Path) -> bool {
    match (
        crate::sidecar_sha256(source),
        crate::sidecar_sha256(installed),
    ) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

#[cfg(target_os = "macos")]
fn run_as_administrator(script: &Path) -> Result<(), String> {
    let status = Command::new("osascript")
        .args([
            "-e",
            "on run argv",
            "-e",
            "do shell script \"/bin/sh \" & quoted form of (item 1 of argv) with administrator privileges",
            "-e",
            "end run",
        ])
        .arg(script)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("osascript: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("administrator rights were not granted".to_string())
    }
}

#[cfg(target_os = "macos")]
fn wait_until_reachable(socket: &Path) -> Result<(), String> {
    let deadline = Instant::now() + INSTALL_WAIT;
    loop {
        match is_running(socket) {
            Ok(_) => return Ok(()),
            Err(e) if Instant::now() >= deadline => return Err(e),
            Err(_) => std::thread::sleep(INSTALL_POLL),
        }
    }
}

fn install_script(helper_source: &Path, mihomo_source: &Path, plist_source: &Path) -> String {
    let install = |mode: &str, from: &Path, to: &Path| {
        format!(
            "install -o {INSTALL_OWNER} -g {INSTALL_GROUP} -m {mode} {} {}",
            shell_quote(from),
            shell_quote(to)
        )
    };
    [
        "set -e".to_string(),
        format!("mkdir -p {}", shell_quote(Path::new(INSTALL_DIR))),
        format!("launchctl bootout system/{LABEL} 2>/dev/null || true"),
        install(BINARY_MODE, helper_source, &installed_helper()),
        install(BINARY_MODE, mihomo_source, &installed_mihomo()),
        format!(
            "xattr -d {QUARANTINE_ATTRIBUTE} {} {} 2>/dev/null || true",
            shell_quote(&installed_helper()),
            shell_quote(&installed_mihomo())
        ),
        install(PLIST_MODE, plist_source, &installed_plist()),
        format!(
            "launchctl bootstrap system {}",
            shell_quote(&installed_plist())
        ),
    ]
    .join("\n")
        + "\n"
}

#[cfg(test)]
fn uninstall_script() -> String {
    [
        format!("launchctl bootout system/{LABEL} 2>/dev/null || true"),
        format!(
            "rm -f {} {} {} {}",
            shell_quote(&installed_plist()),
            shell_quote(&installed_helper()),
            shell_quote(&installed_mihomo()),
            shell_quote(&socket_path())
        ),
    ]
    .join("\n")
        + "\n"
}

fn launchd_plist(settings: &HelperSettings, helper: &Path) -> String {
    let log = Path::new(LOG_DIR).join(format!("{LABEL}.log"));
    let arguments = std::iter::once(helper.to_string_lossy().into_owned())
        .chain(std::iter::once(HELPER_FLAG.to_string()))
        .chain(settings.to_args())
        .map(|arg| format!("    <string>{}</string>", xml_escape(&arg)));
    let mut lines = vec![
        r#"<?xml version="1.0" encoding="UTF-8"?>"#.to_string(),
        r#"<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">"#.to_string(),
        r#"<plist version="1.0">"#.to_string(),
        "<dict>".to_string(),
        "  <key>Label</key>".to_string(),
        format!("  <string>{LABEL}</string>"),
        "  <key>ProgramArguments</key>".to_string(),
        "  <array>".to_string(),
    ];
    lines.extend(arguments);
    lines.extend([
        "  </array>".to_string(),
        "  <key>RunAtLoad</key>".to_string(),
        "  <true/>".to_string(),
        "  <key>KeepAlive</key>".to_string(),
        "  <true/>".to_string(),
        "  <key>StandardErrorPath</key>".to_string(),
        format!("  <string>{}</string>", xml_escape(&log.to_string_lossy())),
        "</dict>".to_string(),
        "</plist>".to_string(),
    ]);
    lines.join("\n") + "\n"
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_WAIT: Duration = Duration::from_secs(5);
    const STAND_PREFIX: &str = "whisp-tun";

    fn wait_for(mut condition: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + TEST_WAIT;
        while Instant::now() < deadline {
            if condition() {
                return true;
            }
            std::thread::sleep(STOP_POLL);
        }
        false
    }

    struct Stand {
        dir: PathBuf,
        settings: HelperSettings,
    }

    impl Stand {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("{STAND_PREFIX}-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let mihomo = dir.join("mihomo");
            std::fs::write(
                &mihomo,
                format!(
                    "#!/bin/sh\necho \"$@\" > '{}'\nexec sleep 600\n",
                    dir.join("args").display()
                ),
            )
            .unwrap();
            std::fs::set_permissions(&mihomo, std::fs::Permissions::from_mode(0o755)).unwrap();
            let settings = HelperSettings {
                socket: dir.join("helper.sock"),
                owner_uid: current_uid(),
                mihomo,
                home: dir.join("home dir"),
                config: dir.join("home dir").join("config.yaml"),
            };
            let serving = settings.clone();
            std::thread::spawn(move || serve(&serving));
            assert!(
                wait_for(|| UnixStream::connect(&settings.socket).is_ok()),
                "helper did not listen"
            );
            Self { dir, settings }
        }

        fn running(&self) -> bool {
            is_running(&self.settings.socket).unwrap()
        }
    }

    impl Drop for Stand {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn settings_survive_the_command_line() {
        let settings = HelperSettings {
            socket: PathBuf::from("/var/run/x.sock"),
            owner_uid: 501,
            mihomo: PathBuf::from("/Library/PrivilegedHelperTools/x.mihomo"),
            home: PathBuf::from("/Users/a b/Library/Application Support/whisp"),
            config: PathBuf::from("/Users/a b/Library/Application Support/whisp/config.yaml"),
        };
        assert_eq!(HelperSettings::from_args(&settings.to_args()), Ok(settings));
        assert!(HelperSettings::from_args(&[ARG_SOCKET.to_string()]).is_err());
    }

    #[test]
    fn start_runs_mihomo_and_stop_ends_it() {
        let stand = Stand::new("start-stop");
        let mut session = request_start(&stand.settings.socket).unwrap();
        assert!(stand.running());
        let args_file = stand.dir.join("args");
        assert!(wait_for(|| args_file.exists()));
        let expected = format!(
            "-d {} -f {}",
            stand.settings.home.display(),
            stand.settings.config.display()
        );
        assert_eq!(
            std::fs::read_to_string(&args_file).unwrap().trim(),
            expected
        );
        request_stop(&mut session).unwrap();
        assert!(!stand.running());
    }

    #[test]
    fn mihomo_stops_when_the_app_goes_away() {
        let stand = Stand::new("app-gone");
        let session = request_start(&stand.settings.socket).unwrap();
        assert!(stand.running());
        drop(session);
        assert!(
            wait_for(|| !stand.running()),
            "mihomo outlived the app that started it"
        );
    }

    #[test]
    fn another_user_is_refused() {
        let (mut app, helper_end) = UnixStream::pair().unwrap();
        let settings = HelperSettings {
            socket: PathBuf::new(),
            owner_uid: current_uid().wrapping_add(1),
            mihomo: PathBuf::from("/nonexistent/mihomo"),
            home: PathBuf::new(),
            config: PathBuf::new(),
        };
        let mihomo = Mutex::new(Mihomo::default());
        serve_connection(helper_end, 1, &settings, &mihomo);
        assert_eq!(exchange(&mut app, REQUEST_START).unwrap(), REPLY_DENIED);
        assert!(describe_failure(REPLY_DENIED).contains("another user"));
        assert!(lock(&mihomo).child.is_none());
    }

    #[test]
    fn socket_is_private_to_its_owner() {
        let stand = Stand::new("socket-mode");
        let mode = std::fs::metadata(&stand.settings.socket)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, SOCKET_MODE);
    }

    #[test]
    fn install_script_quotes_every_path() {
        let script = install_script(
            Path::new("/Applications/Whisp.app/Contents/MacOS/Whisp"),
            Path::new("/Users/o'neil/Whisp/mihomo"),
            Path::new("/tmp/with space/helper.plist"),
        );
        assert!(script.starts_with("set -e\n"));
        assert!(
            script.contains(&format!(
                "xattr -d {QUARANTINE_ATTRIBUTE} {} {}",
                shell_quote(&installed_helper()),
                shell_quote(&installed_mihomo())
            )),
            "{script}"
        );
        assert!(
            script.contains("'/Users/o'\\''neil/Whisp/mihomo'"),
            "{script}"
        );
        assert!(
            script.contains("'/tmp/with space/helper.plist'"),
            "{script}"
        );
        assert!(script.ends_with(&format!(
            "launchctl bootstrap system {}\n",
            shell_quote(&installed_plist())
        )));
    }

    #[test]
    fn uninstall_removes_what_install_put_in_place() {
        let script = uninstall_script();
        assert!(script.starts_with(&format!("launchctl bootout system/{LABEL}")));
        for installed in [
            installed_plist(),
            installed_helper(),
            installed_mihomo(),
            socket_path(),
        ] {
            assert!(script.contains(&shell_quote(&installed)), "{script}");
        }
    }

    #[test]
    fn plist_runs_the_helper_with_its_settings() {
        let settings = HelperSettings {
            socket: socket_path(),
            owner_uid: 501,
            mihomo: installed_mihomo(),
            home: PathBuf::from("/Users/a&b/whisp"),
            config: PathBuf::from("/Users/a&b/whisp/config.yaml"),
        };
        let plist = launchd_plist(&settings, &installed_helper());
        assert!(plist.contains(&format!("<string>{LABEL}</string>")));
        assert!(plist.contains(&format!("<string>{HELPER_FLAG}</string>")));
        assert!(plist.contains("<string>/Users/a&amp;b/whisp/config.yaml</string>"));
        assert!(plist.contains("<key>KeepAlive</key>\n  <true/>"));
        assert!(!plist.contains("a&b"));
    }

    #[cfg(target_os = "macos")]
    mod on_a_mac {
        use super::super::*;
        use crate::mihomo::{generate_config, MihomoConfig, MihomoRoutingRule};
        use std::net::{IpAddr, Ipv4Addr};
        use std::ops::Range;

        const HELPER_BIN_ENV: &str = "WHISP_HELPER_BIN";
        const MIHOMO_BIN_ENV: &str = "WHISP_MIHOMO_BIN";
        const SOCKS_ENV: &str = "WHISP_TUN_SOCKS";
        const SERVER_HOST_ENV: &str = "WHISP_TUN_SERVER_HOST";
        const SERVER_PROCESS_ENV: &str = "WHISP_TUN_SERVER_PROCESS";
        const SERVER_LOG_ENV: &str = "WHISP_TUN_SERVER_LOG";
        const BROWSER_ENV: &str = "WHISP_TUN_BROWSER";
        const SCREENSHOT_ENV: &str = "WHISP_TUN_SCREENSHOT";

        const TUNNEL_HOST: &str = "www.youtube.com";
        const RU_BYPASS_HOST: &str = "ya.ru";
        const DIRECT_DOMAIN_HOST: &str = "example.com";
        const RU_PROXIED_HOST: &str = "mail.ru";
        const APP_RULE_HOST: &str = "www.wikipedia.org";

        const PROBE_APP: &str = "Whisp Probe.app";
        const PROBE_HELPER_APP: &str = "Whisp Probe Helper.app";
        const CURL: &str = "/usr/bin/curl";

        const MIXED_PORT: u16 = 7890;
        const TUN_STACK: &str = "mixed";
        const LOG_LEVEL: &str = "info";
        const ROUTING_MODE: &str = "rule";
        const PROCESS_RULE: &str = "process";
        const DOMAIN_RULE: &str = "domain";
        const DIRECT_ACTION: &str = "DIRECT";
        const PROXY_ACTION: &str = "PROXY";
        const FAKE_IP_RANGE_KEY: &str = "fake-ip-range:";
        const CONTROLLER_KEY: &str = "external-controller:";
        const VERSION_PATH: &str = "/version";
        const CONNECTIONS_PATH: &str = "/connections";
        const ROUTE_INTERFACE_KEY: &str = "interface:";
        const PUBLIC_ROUTE_PROBE: &str = "1.1.1.1";
        const TUN_INTERFACE_PREFIX: &str = "utun";
        const HTTP_OK: &str = "200";
        const ANSWERED: Range<u16> = 200..400;
        const ROUTE_WAIT: Duration = Duration::from_secs(90);
        const PROBE_INTERVAL: Duration = Duration::from_secs(2);
        const PROBE_TIMEOUT_SECS: &str = "15";
        const CONTROLLER_TIMEOUT_SECS: &str = "5";
        const HELPER_LOG_LINES: &str = "50";
        const DIAGNOSTIC_LIMIT: usize = 4000;
        const SCREENSHOT_SIZE: &str = "1280,800";
        const SCREENSHOT_SCRIPT_TIME_MS: &str = "15000";

        fn required(name: &str) -> String {
            std::env::var(name).unwrap_or_else(|_| panic!("set {name}"))
        }

        fn url(host: &str) -> String {
            format!("https://{host}/")
        }

        fn rule(kind: &str, value: &str, action: &str) -> MihomoRoutingRule {
            MihomoRoutingRule {
                kind: kind.into(),
                value: value.into(),
                action: action.into(),
            }
        }

        fn run_as_root(script: &Path) {
            let status = Command::new("sudo")
                .arg("-n")
                .arg("/bin/sh")
                .arg(script)
                .status()
                .expect("sudo");
            assert!(status.success(), "{} failed", script.display());
        }

        fn probe_bundle(work: &Path) -> (PathBuf, PathBuf, PathBuf) {
            let bundle = work.join(PROBE_APP);
            let main = bundle.join("Contents/MacOS/probe");
            let helper = bundle
                .join("Contents/Frameworks")
                .join(PROBE_HELPER_APP)
                .join("Contents/MacOS/probe-helper");
            for program in [&main, &helper] {
                std::fs::create_dir_all(program.parent().unwrap()).unwrap();
                std::fs::copy(CURL, program).unwrap();
            }
            (
                std::fs::canonicalize(&bundle).unwrap(),
                std::fs::canonicalize(&main).unwrap(),
                std::fs::canonicalize(&helper).unwrap(),
            )
        }

        fn config_value<'a>(config: &'a str, key: &str) -> &'a str {
            config
                .lines()
                .map(str::trim)
                .find_map(|line| line.strip_prefix(key))
                .unwrap_or_else(|| panic!("the config has no {key}"))
                .trim()
        }

        fn fake_ip_range(config: &str) -> (Ipv4Addr, u32) {
            let value = config_value(config, FAKE_IP_RANGE_KEY);
            let (base, prefix) = value.split_once('/').expect("the range is base/prefix");
            (
                base.parse().expect("the range base is IPv4"),
                prefix.parse().expect("the prefix is a number"),
            )
        }

        fn is_fake(ip: IpAddr, (base, prefix): (Ipv4Addr, u32)) -> bool {
            let IpAddr::V4(ip) = ip else {
                return false;
            };
            let mask = u32::MAX.checked_shl(32 - prefix).unwrap_or(0);
            u32::from(ip) & mask == u32::from(base) & mask
        }

        fn probe(program: &Path, url: &str) -> Option<(u16, IpAddr)> {
            let out = Command::new(program)
                .args(["-sS", "-o", "/dev/null", "--max-time", PROBE_TIMEOUT_SECS])
                .args(["-w", "%{http_code} %{remote_ip}", url])
                .output()
                .ok()?;
            let text = String::from_utf8_lossy(&out.stdout);
            let (code, ip) = text.trim().split_once(' ')?;
            Some((code.parse().ok()?, ip.parse().ok()?))
        }

        fn eventually(mut condition: impl FnMut() -> bool) -> bool {
            let deadline = Instant::now() + ROUTE_WAIT;
            while Instant::now() < deadline {
                if condition() {
                    return true;
                }
                std::thread::sleep(PROBE_INTERVAL);
            }
            false
        }

        fn answers(program: &Path, url: &str) -> Option<IpAddr> {
            let mut answer = None;
            eventually(|| {
                answer = probe(program, url)
                    .filter(|(code, _)| ANSWERED.contains(code))
                    .map(|(_, ip)| ip);
                answer.is_some()
            });
            answer
        }

        fn reached_server(log: &Path, host: &str) -> bool {
            std::fs::read_to_string(log).is_ok_and(|text| text.contains(&format!("{host}:")))
        }

        fn controller_answers(controller: &str) -> bool {
            Command::new(CURL)
                .args([
                    "-s",
                    "-o",
                    "/dev/null",
                    "--max-time",
                    CONTROLLER_TIMEOUT_SECS,
                ])
                .args(["-w", "%{http_code}"])
                .arg(format!("http://{controller}{VERSION_PATH}"))
                .output()
                .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).trim() == HTTP_OK)
        }

        fn public_route_through_tun() -> bool {
            Command::new("route")
                .args(["-n", "get", PUBLIC_ROUTE_PROBE])
                .output()
                .is_ok_and(|out| {
                    String::from_utf8_lossy(&out.stdout)
                        .lines()
                        .filter_map(|line| line.trim().strip_prefix(ROUTE_INTERFACE_KEY))
                        .any(|name| name.trim().starts_with(TUN_INTERFACE_PREFIX))
                })
        }

        fn command_output(program: &str, args: &[&str]) -> String {
            match Command::new(program).args(args).output() {
                Ok(out) => {
                    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
                    text.push_str(&String::from_utf8_lossy(&out.stderr));
                    text.chars().take(DIAGNOSTIC_LIMIT).collect()
                }
                Err(e) => format!("{program}: {e}"),
            }
        }

        fn diagnostics(controller: &str, settings: &HelperSettings) -> String {
            let helper_log = Path::new(LOG_DIR).join(format!("{LABEL}.log"));
            let helper_log = helper_log.to_string_lossy();
            let mihomo = settings.mihomo.to_string_lossy();
            let home = settings.home.to_string_lossy();
            let config = settings.config.to_string_lossy();
            let version = format!("http://{controller}{VERSION_PATH}");
            let connections = format!("http://{controller}{CONNECTIONS_PATH}");
            let checks: [(&str, &str, Vec<&str>); 9] = [
                ("default route", "route", vec!["-n", "get", "default"]),
                ("routes", "netstat", vec!["-rn", "-f", "inet"]),
                ("interfaces", "ifconfig", vec!["-l"]),
                ("dns", "scutil", vec!["--dns"]),
                ("mihomo processes", "pgrep", vec!["-fl", "mihomo"]),
                (
                    "mihomo version",
                    CURL,
                    vec!["-sS", "--max-time", CONTROLLER_TIMEOUT_SECS, &version],
                ),
                (
                    "mihomo connections",
                    CURL,
                    vec!["-sS", "--max-time", CONTROLLER_TIMEOUT_SECS, &connections],
                ),
                (
                    "mihomo config test",
                    "sudo",
                    vec!["-n", &mihomo, "-t", "-d", &home, "-f", &config],
                ),
                (
                    "helper log",
                    "sudo",
                    vec!["-n", "tail", "-n", HELPER_LOG_LINES, &helper_log],
                ),
            ];
            checks
                .iter()
                .map(|(title, program, args)| {
                    format!("--- {title}\n{}\n", command_output(program, args))
                })
                .collect()
        }

        fn wait_for_route(program: &Path, url: &str, accept: impl Fn(IpAddr) -> bool) -> bool {
            eventually(|| {
                probe(program, url).is_some_and(|(code, ip)| ANSWERED.contains(&code) && accept(ip))
            })
        }

        #[test]
        #[ignore = "installs the LaunchDaemon with sudo and routes the machine through TUN; for a macOS CI runner"]
        fn split_tunnel_on_a_mac_matches_windows() {
            let work = std::env::temp_dir().join(format!("{LABEL}-e2e"));
            let _ = std::fs::remove_dir_all(&work);
            std::fs::create_dir_all(&work).unwrap();
            let work = std::fs::canonicalize(&work).unwrap();
            let (bundle, bundle_main, bundle_helper) = probe_bundle(&work);
            let server_log = PathBuf::from(required(SERVER_LOG_ENV));
            let curl = Path::new(CURL);

            let socks = required(SOCKS_ENV);
            let server_host = required(SERVER_HOST_ENV);
            let rules = [
                rule(PROCESS_RULE, &required(SERVER_PROCESS_ENV), DIRECT_ACTION),
                rule(DOMAIN_RULE, DIRECT_DOMAIN_HOST, DIRECT_ACTION),
                rule(DOMAIN_RULE, RU_PROXIED_HOST, PROXY_ACTION),
                rule(PROCESS_RULE, &bundle.to_string_lossy(), DIRECT_ACTION),
            ];
            let config_text = generate_config(&MihomoConfig {
                socks_addr: &socks,
                server_host: &server_host,
                mixed_port: MIXED_PORT,
                tun: true,
                tun_stack: TUN_STACK,
                dns_redirect: false,
                ipv6: false,
                routing_rules: &rules,
                extra_socks_addrs: &[],
                custom_dns: &[],
                socks_user: "",
                socks_pass: "",
                allow_lan: false,
                log_level: LOG_LEVEL,
                routing_mode: ROUTING_MODE,
                bypass_ru: true,
                external_link: "",
                secret: "",
                kill_switch: false,
                vpn_dns: "",
            });
            let fake_range = fake_ip_range(&config_text);
            let config = work.join("config.yaml");
            std::fs::write(&config, &config_text).unwrap();
            let settings = HelperSettings {
                socket: socket_path(),
                owner_uid: current_uid(),
                mihomo: installed_mihomo(),
                home: work.clone(),
                config,
            };

            let plist = work.join(format!("{LABEL}.plist"));
            std::fs::write(&plist, launchd_plist(&settings, &installed_helper())).unwrap();
            let install = work.join("install.sh");
            let helper_source = PathBuf::from(required(HELPER_BIN_ENV));
            let mihomo_source = PathBuf::from(required(MIHOMO_BIN_ENV));
            std::fs::write(
                &install,
                install_script(&helper_source, &mihomo_source, &plist),
            )
            .unwrap();
            run_as_root(&install);
            wait_until_reachable(&settings.socket).expect("the helper answers after install");

            let dns_before = crate::system_dns::snapshot().expect("system DNS settings");
            let mut session = request_start(&settings.socket).expect("the helper starts mihomo");
            let controller = config_value(&config_text, CONTROLLER_KEY).to_string();
            let report = || diagnostics(&controller, &settings);
            assert!(
                eventually(|| controller_answers(&controller)),
                "mihomo never answered on {controller}\n{}",
                report()
            );
            assert!(
                eventually(public_route_through_tun),
                "traffic to {PUBLIC_ROUTE_PROBE} never moved to the TUN\n{}",
                report()
            );

            assert!(
                wait_for_route(curl, &url(TUNNEL_HOST), |ip| is_fake(ip, fake_range)),
                "{TUNNEL_HOST} was not resolved by mihomo: system DNS goes around the TUN\n{}",
                report()
            );
            assert!(
                reached_server(&server_log, TUNNEL_HOST),
                "{TUNNEL_HOST} did not go through the tunnel\n{}",
                report()
            );

            for host in [RU_BYPASS_HOST, DIRECT_DOMAIN_HOST] {
                assert!(
                    wait_for_route(curl, &url(host), |_| true),
                    "{host} did not answer\n{}",
                    report()
                );
                assert!(
                    !reached_server(&server_log, host),
                    "{host} went through the tunnel\n{}",
                    report()
                );
            }
            assert!(
                wait_for_route(curl, &url(RU_PROXIED_HOST), |_| true),
                "{RU_PROXIED_HOST} did not answer\n{}",
                report()
            );
            assert!(
                reached_server(&server_log, RU_PROXIED_HOST),
                "the PROXY rule for {RU_PROXIED_HOST} lost to the .ru bypass\n{}",
                report()
            );

            for program in [&bundle_main, &bundle_helper] {
                assert!(
                    wait_for_route(program, &url(APP_RULE_HOST), |_| true),
                    "{} could not reach {APP_RULE_HOST}\n{}",
                    program.display(),
                    report()
                );
            }
            assert!(
                !reached_server(&server_log, APP_RULE_HOST),
                "the rule for {} did not keep its processes direct\n{}",
                bundle.display(),
                report()
            );
            assert!(
                wait_for_route(curl, &url(APP_RULE_HOST), |_| true),
                "{APP_RULE_HOST} did not answer\n{}",
                report()
            );
            assert!(
                reached_server(&server_log, APP_RULE_HOST),
                "outside the app {APP_RULE_HOST} should go through the tunnel\n{}",
                report()
            );

            if let (Ok(browser), Ok(screenshot)) =
                (std::env::var(BROWSER_ENV), std::env::var(SCREENSHOT_ENV))
            {
                let status = Command::new(browser)
                    .args(["--headless=new", "--disable-gpu", "--hide-scrollbars"])
                    .arg(format!("--window-size={SCREENSHOT_SIZE}"))
                    .arg(format!("--virtual-time-budget={SCREENSHOT_SCRIPT_TIME_MS}"))
                    .arg(format!("--screenshot={screenshot}"))
                    .arg(url(TUNNEL_HOST))
                    .status()
                    .expect("browser");
                assert!(
                    status.success() && Path::new(&screenshot).exists(),
                    "no screenshot of {TUNNEL_HOST}"
                );
            }

            request_stop(&mut session).expect("the helper stops mihomo");
            assert!(!is_running(&settings.socket).unwrap());
            assert_eq!(
                crate::system_dns::snapshot().expect("system DNS settings"),
                dns_before,
                "system DNS was not restored after stop"
            );
            assert!(
                eventually(|| !public_route_through_tun()),
                "traffic to {PUBLIC_ROUTE_PROBE} stayed on the TUN after stop\n{}",
                report()
            );
            assert!(
                answers(curl, &url(TUNNEL_HOST)).is_some(),
                "the network did not come back after the TUN went down\n{}",
                report()
            );

            let uninstall = work.join("uninstall.sh");
            std::fs::write(&uninstall, uninstall_script()).unwrap();
            run_as_root(&uninstall);
            assert!(!installed_plist().exists());
        }
    }
}
