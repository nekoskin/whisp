use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const CREATE_NO_WINDOW: u32 = 0x08000000;
#[cfg(windows)]
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x00004000;

const SERVICE_NAME: &str = "WhisperaGW";
const SERVICE_DISPLAY: &str = "Whispera Gateway";
const KEEP_FULL_DIAL_TRACE: &str = "-dial-trace-mb=512";
const POLICY_TRAINER_FLAGS: [(&str, &str); 3] = [
    ("-policy-trainer", "WHISPERA_POLICY_TRAINER"),
    ("-policy-python", "WHISPERA_POLICY_PYTHON"),
    ("-pattern-dumps", "WHISPERA_PATTERN_DUMPS"),
];

fn policy_trainer_args() -> String {
    POLICY_TRAINER_FLAGS
        .iter()
        .filter_map(|(flag, var)| {
            std::env::var(var)
                .ok()
                .filter(|v| !v.is_empty())
                .map(|v| format!(" {} \"{}\"", flag, v.replace('"', "\\\"")))
        })
        .collect()
}

pub struct GoClientManager {
    binary_path: PathBuf,
    process: Option<Child>,
    use_service: bool,
}

pub struct GoClientConfig<'a> {
    pub conn_key: &'a str,
    pub server_addr: &'a str,
    pub socks_addr: &'a str,
    pub transport: &'a str,
    pub vpn_dns: &'a str,
    pub hwid: bool,
    pub tls_fingerprint: &'a str,
    pub split_rules: &'a str,
    pub tls_fragment: bool,
    pub ipv6_fallback: bool,
    pub ipv6_only: bool,
}

fn is_forceable_fingerprint(v: &str) -> bool {
    !v.is_empty() && v != "random"
}

pub(crate) fn go_client_log_file() -> std::path::PathBuf {
    std::env::temp_dir().join("whispera-go-client.log")
}

pub(crate) fn log_len() -> u64 {
    std::fs::metadata(go_client_log_file())
        .map(|m| m.len())
        .unwrap_or(0)
}

pub(crate) fn startup_failure(since: u64) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let mut tail = Vec::new();
    if let Ok(mut f) = std::fs::File::open(go_client_log_file()) {
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        let from = since.max(len.saturating_sub(64 * 1024));
        if f.seek(SeekFrom::Start(from)).is_ok() {
            let _ = f.read_to_end(&mut tail);
        }
    }
    let text = String::from_utf8_lossy(&tail);
    match failure_line(&text) {
        Some(line) => format!("go-client stopped at startup: {}", line),
        None => "go-client stopped at startup".to_string(),
    }
}

fn failure_line(tail: &str) -> Option<&str> {
    let line = tail
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("[whisp]"))?;
    Some(without_log_stamp(line))
}

fn without_log_stamp(line: &str) -> &str {
    let b = line.as_bytes();
    if b.len() > 20 && b[4] == b'/' && b[7] == b'/' && b[13] == b':' && b[19] == b' ' {
        &line[20..]
    } else {
        line
    }
}

// The UI tails this file, so a sidecar dying has to say so here or the user
// sees a connection drop with nothing to explain it.
pub(crate) fn note_exit(who: &str, status: std::process::ExitStatus) {
    use std::io::Write;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let line = match status.code() {
        Some(code) => format!("[whisp] {} exited with code {} (t={})\n", who, code, stamp),
        None => format!("[whisp] {} was killed by a signal (t={})\n", who, stamp),
    };
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(go_client_log_file())
    {
        let _ = f.write_all(line.as_bytes());
    }
}

impl GoClientManager {
    pub fn new(binary_path: PathBuf) -> Self {
        Self {
            binary_path,
            process: None,
            use_service: false,
        }
    }

    pub fn install_service(&self, cfg: &GoClientConfig) -> Result<(), String> {
        let bin = self.binary_path.to_string_lossy().to_string();

        let key_part = if !cfg.conn_key.is_empty() {
            format!("-key \"{}\"", cfg.conn_key)
        } else {
            format!("-server \"{}\"", cfg.server_addr)
        };
        let mut args = format!(
            "{} -socks \"{}\" -no-tun {}",
            key_part, cfg.socks_addr, KEEP_FULL_DIAL_TRACE
        );
        if !cfg.split_rules.is_empty() {
            args.push_str(&format!(
                " -split-rules \"{}\"",
                cfg.split_rules.replace('"', "\\\"")
            ));
        }
        if !cfg.transport.is_empty() {
            args.push_str(&format!(" -transport {}", cfg.transport));
        }
        if !cfg.vpn_dns.is_empty() {
            args.push_str(&format!(" -dns \"{}\"", cfg.vpn_dns));
        }
        if !cfg.hwid {
            args.push_str(" -hwid=false");
        }
        if is_forceable_fingerprint(cfg.tls_fingerprint) {
            args.push_str(&format!(" -force-fingerprint {}", cfg.tls_fingerprint));
        }
        // The toggle only ever reached Android; on the desktop the switch sat
        // in the UI doing nothing while fragmentation stayed on.
        if !cfg.tls_fragment {
            args.push_str(" -hello-frag=false");
        }
        if !cfg.ipv6_fallback {
            args.push_str(" -ipv6-fallback=false");
        }
        if cfg.ipv6_only {
            args.push_str(" -ipv6-only");
        }
        args.push_str(&policy_trainer_args());

        args.push_str(&format!(
            " -log-file \"{}\"",
            go_client_log_file().display()
        ));

        let bin_path = format!("\"{}\" {}", bin.replace('"', "\\\""), args);

        let _ = Command::new("sc")
            .args(["delete", SERVICE_NAME])
            .creation_flags_win(CREATE_NO_WINDOW)
            .output();

        let status = Command::new("sc")
            .args([
                "create",
                SERVICE_NAME,
                "binPath=",
                &bin_path,
                "type=",
                "own",
                "start=",
                "demand",
                "DisplayName=",
                SERVICE_DISPLAY,
                "error=",
                "ignore",
            ])
            .creation_flags_win(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("sc create failed: {}", e))?;

        if !status.status.success() {
            let err = String::from_utf8_lossy(&status.stderr).to_string();
            return Err(format!("Service install failed: {}", err));
        }

        let _ = Command::new("sc")
            .args([
                "description",
                SERVICE_NAME,
                "Whispera VPN gateway tunnel process",
            ])
            .creation_flags_win(CREATE_NO_WINDOW)
            .output();

        Ok(())
    }

    fn start_service(&mut self) -> Result<(), String> {
        let out = Command::new("sc")
            .args(["start", SERVICE_NAME])
            .creation_flags_win(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| e.to_string())?;

        if out.status.success()
            || String::from_utf8_lossy(&out.stdout).contains("RUNNING")
            || String::from_utf8_lossy(&out.stderr).contains("1056")
        {
            self.use_service = true;
            return Ok(());
        }

        Err(format!(
            "sc start failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ))
    }

    fn stop_service(&mut self) -> Result<(), String> {
        Command::new("sc")
            .args(["stop", SERVICE_NAME])
            .creation_flags_win(CREATE_NO_WINDOW)
            .output()
            .ok();
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(2000);
        while std::time::Instant::now() < deadline {
            if !service_running(SERVICE_NAME) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        Command::new("taskkill")
            .args(["/F", "/IM", "whispera-go-client.exe"])
            .creation_flags_win(CREATE_NO_WINDOW)
            .output()
            .ok();
        self.use_service = false;
        Ok(())
    }

    pub fn start(&mut self, cfg: &GoClientConfig) -> Result<(), String> {
        eprintln!(
            "[go_client] start() called, binary={}",
            self.binary_path.display()
        );

        if self.owns_live_tunnel() {
            eprintln!("[go_client] already running — keeping existing tunnel alive");
            return Ok(());
        }

        self.stop()?;
        self.wait_ports_released();

        if service_exists(SERVICE_NAME) {
            eprintln!("[go_client] service exists, trying service mode");
            if self.install_service(cfg).is_ok() && self.start_service().is_ok() {
                eprintln!("[go_client] started as service OK");
                return Ok(());
            }
            eprintln!("[go_client] service mode failed, falling back to direct");
        }

        self.start_direct(cfg)
    }

    fn start_direct(&mut self, cfg: &GoClientConfig) -> Result<(), String> {
        eprintln!(
            "[go_client] start_direct: binary={}, exists={}",
            self.binary_path.display(),
            self.binary_path.exists()
        );
        let mut cmd = Command::new(&self.binary_path);

        if !cfg.conn_key.is_empty() {
            cmd.arg("-key").arg(cfg.conn_key);
        } else if !cfg.server_addr.is_empty() {
            cmd.arg("-server").arg(cfg.server_addr);
        } else {
            return Err("No connection key or server address provided".to_string());
        }

        cmd.arg("-socks").arg(cfg.socks_addr);
        cmd.arg("-no-tun");
        cmd.arg(KEEP_FULL_DIAL_TRACE);
        cmd.arg("-log-file").arg(go_client_log_file());

        if !cfg.split_rules.is_empty() {
            cmd.arg("-split-rules").arg(cfg.split_rules);
        }

        if !cfg.transport.is_empty() {
            cmd.arg("-transport").arg(cfg.transport);
        }

        if !cfg.vpn_dns.is_empty() {
            cmd.arg("-dns").arg(cfg.vpn_dns);
        }

        if !cfg.hwid {
            cmd.arg("-hwid=false");
        }

        if is_forceable_fingerprint(cfg.tls_fingerprint) {
            cmd.arg("-force-fingerprint").arg(cfg.tls_fingerprint);
        }
        if !cfg.tls_fragment {
            cmd.arg("-hello-frag=false");
        }
        if !cfg.ipv6_fallback {
            cmd.arg("-ipv6-fallback=false");
        }
        if cfg.ipv6_only {
            cmd.arg("-ipv6-only");
        }

        let log_path = std::env::temp_dir().join("whispera-go-client.log");
        cmd.arg("-log-file").arg(&log_path);
        let log_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map(Stdio::from)
            .unwrap_or(Stdio::null());
        let log_file2 = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map(Stdio::from)
            .unwrap_or(Stdio::null());

        cmd.stdout(log_file).stderr(log_file2);

        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);

        let child = cmd
            .spawn()
            .map_err(|e| format!("Failed to start go-client: {}", e))?;

        self.process = Some(child);
        self.use_service = false;
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), String> {
        if self.use_service {
            return self.stop_service();
        }
        if let Some(ref mut child) = self.process {
            child.kill().ok();
            child.wait().ok();
        }
        self.process = None;
        self.kill_all_by_name();
        Ok(())
    }

    pub fn kill_all_by_name(&self) {
        #[cfg(windows)]
        {
            Command::new("taskkill")
                .args(["/F", "/IM", "whispera-go-client.exe"])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .ok();
        }
        #[cfg(unix)]
        {
            Command::new("pkill")
                .args(["-9", "-f", "whispera-go-client"])
                .output()
                .ok();
        }
    }

    pub fn is_running(&mut self) -> bool {
        if self.use_service {
            return service_running(SERVICE_NAME);
        }
        match &mut self.process {
            Some(child) => match child.try_wait() {
                Ok(Some(status)) => {
                    note_exit("whispera-go-client", status);
                    self.process = None;
                    false
                }
                Ok(None) => true,
                Err(_) => false,
            },
            None => control_reachable(),
        }
    }

    fn owns_live_tunnel(&mut self) -> bool {
        if self.use_service {
            return service_running(SERVICE_NAME);
        }
        match &mut self.process {
            Some(child) => match child.try_wait() {
                Ok(Some(status)) => {
                    note_exit("whispera-go-client", status);
                    self.process = None;
                    false
                }
                Ok(None) => true,
                Err(_) => false,
            },
            None => false,
        }
    }

    fn wait_ports_released(&self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(3000);
        while std::time::Instant::now() < deadline {
            if !control_reachable() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

impl Drop for GoClientManager {
    fn drop(&mut self) {
        self.stop().ok();
        self.kill_all_by_name();
    }
}

pub(crate) fn control_reachable() -> bool {
    crate::mihomo::http_ok("127.0.0.1:10801", "/connections")
}

// Whether any transport is actually carrying traffic, and why not when it is
// not. A live process says nothing about a live tunnel: mihomo without TUN and
// a client that never reached the server both look healthy from outside.
// None means the client could not be asked at all.
pub(crate) struct TunnelState {
    pub up: bool,
    pub error: Option<String>,
    pub failure: Option<serde_json::Value>,
    pub connected_at: Option<String>,
}

pub(crate) fn tunnel_state() -> Option<TunnelState> {
    let (200, body) = crate::mihomo::http_get("127.0.0.1:10801", "/connections")? else {
        return None;
    };
    let entries: Vec<serde_json::Value> = serde_json::from_str(&body).ok()?;
    let mut reason = None;
    let failure = entries
        .iter()
        .find_map(|e| e.get("failure").filter(|v| v.is_object()).cloned());
    for e in &entries {
        if e.get("status").and_then(|v| v.as_str()) == Some("connected") {
            return Some(TunnelState {
                up: true,
                error: None,
                failure,
                connected_at: e
                    .get("connected_at")
                    .and_then(|v| v.as_str())
                    .map(millisecond_timestamp),
            });
        }
        if reason.is_none() {
            reason = e
                .get("error")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string);
        }
    }
    Some(TunnelState {
        up: false,
        error: reason,
        failure,
        connected_at: None,
    })
}

fn millisecond_timestamp(stamp: &str) -> String {
    let Some(dot) = stamp.find('.') else {
        return stamp.to_string();
    };
    let fraction = &stamp[dot + 1..];
    let digits = fraction
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(fraction.len());
    format!(
        "{}.{:0<3}{}",
        &stamp[..dot],
        &fraction[..digits.min(3)],
        &fraction[digits..]
    )
}

fn service_exists(name: &str) -> bool {
    Command::new("sc")
        .args(["query", name])
        .creation_flags_win(CREATE_NO_WINDOW)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn service_running(name: &str) -> bool {
    Command::new("sc")
        .args(["query", name])
        .creation_flags_win(CREATE_NO_WINDOW)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("RUNNING"))
        .unwrap_or(false)
}

trait CommandExtWin {
    fn creation_flags_win(&mut self, flags: u32) -> &mut Self;
}

impl CommandExtWin for Command {
    fn creation_flags_win(&mut self, _flags: u32) -> &mut Self {
        #[cfg(windows)]
        self.creation_flags(_flags);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{failure_line, millisecond_timestamp};

    #[test]
    fn connected_at_is_cut_to_milliseconds_and_keeps_its_offset() {
        assert_eq!(
            millisecond_timestamp("2026-10-17T16:38:54.053883208+03:00"),
            "2026-10-17T16:38:54.053+03:00"
        );
        assert_eq!(
            millisecond_timestamp("2026-10-17T13:38:54.5Z"),
            "2026-10-17T13:38:54.500Z"
        );
        assert_eq!(
            millisecond_timestamp("2026-10-17T13:38:54Z"),
            "2026-10-17T13:38:54Z"
        );
    }

    #[test]
    fn startup_failure_names_the_fatal_line_not_our_exit_note() {
        let tail = "2026/09/23 09:35:12 Whispera Client starting...
                    2026/09/23 09:35:12 Failed to parse connection key: invalid key format
                    [whisp] whispera-go-client exited with code 1 (t=1)
";
        assert_eq!(
            failure_line(tail),
            Some("Failed to parse connection key: invalid key format")
        );
    }

    #[test]
    fn startup_failure_is_empty_when_the_client_wrote_nothing() {
        assert_eq!(
            failure_line(
                "[whisp] whispera-go-client exited with code 1 (t=1)
"
            ),
            None
        );
    }
}
