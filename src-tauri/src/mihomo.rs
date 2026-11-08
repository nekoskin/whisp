use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const CREATE_NO_WINDOW: u32 = 0x08000000;
#[cfg(windows)]
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x00004000;

const SERVICE_NAME: &str = "WhisperaNH";
#[cfg(windows)]
const SERVICE_DISPLAY: &str = "Whispera Network Helper";

const CONTROLLER_ADDR: &str = "127.0.0.1:9090";
const CONTROLLER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const TUN_OFF: &str = r#"{"tun":{"enable":false}}"#;
const TUN_DEVICE: &str = "Meta";
#[cfg(any(windows, test))]
const TUN_TASK: &str = "WhispTUN";
#[cfg(windows)]
const OWNER_PID_FILE: &str = "whisp-owner.pid";
#[cfg(windows)]
const TUN_TASK_STAMP_FILE: &str = "tun-task.sha256";
#[cfg(windows)]
const TUN_STOP_WAIT: std::time::Duration = std::time::Duration::from_secs(3);

fn controller_secret(config: &str) -> Option<&str> {
    config
        .lines()
        .find_map(|line| line.strip_prefix("secret:"))
        .map(|value| value.trim().trim_matches('"'))
}

#[cfg(any(windows, test))]
fn remove_stale_tun_script() -> String {
    format!(
        "$id = [guid]::new([Security.Cryptography.MD5]::Create().ComputeHash([Text.Encoding]::UTF8.GetBytes('wintun{TUN_DEVICE}')))\n\
         & pnputil.exe /remove-device \"SWD\\Wintun\\{{$id}}\" | Out-Null"
    )
}

#[cfg(any(windows, test))]
fn tun_task_script(bin: &str, home_dir: &str, config: &str, pid_file: &str, owner: &str) -> String {
    let quoted = |s: &str| s.replace('\'', "''");
    format!(
        "$pidFile = '{pid_file}'\n\
         $ownerId = 0\n\
         if (-not [int]::TryParse(\"$(Get-Content -Raw -LiteralPath $pidFile -ErrorAction SilentlyContinue)\".Trim(), [ref]$ownerId)) {{ exit 1 }}\n\
         $owner = Get-Process -Id $ownerId -ErrorAction SilentlyContinue\n\
         if (-not $owner -or $owner.ProcessName -ne '{owner}') {{ exit 1 }}\n\
         $null = $owner.Handle\n\
         {cleanup}\n\
         $mihomo = Start-Process -FilePath '{bin}' -ArgumentList '-d','{home_dir}','-f','{config}' -WindowStyle Hidden -PassThru\n\
         while (-not $mihomo.WaitForExit(500)) {{\n\
         \x20   if ($owner.HasExited -or -not (Test-Path -LiteralPath $pidFile)) {{ Stop-Process -Id $mihomo.Id -Force; break }}\n\
         }}",
        pid_file = quoted(pid_file),
        owner = quoted(owner),
        cleanup = remove_stale_tun_script(),
        bin = quoted(bin),
        home_dir = quoted(home_dir),
        config = quoted(config),
    )
}

#[cfg(any(windows, test))]
fn register_tun_task_script(task_args: &str, user_sid: &str) -> String {
    format!(
        "$action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument '{task_args}'\n\
         $principal = New-ScheduledTaskPrincipal -UserId '{user_sid}' -LogonType Interactive -RunLevel Highest\n\
         $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances Parallel\n\
         Register-ScheduledTask -TaskName '{TUN_TASK}' -Action $action -Principal $principal -Settings $settings -Force -ErrorAction Stop | Out-Null"
    )
}

#[cfg(any(windows, test))]
fn encoded_powershell(script: &str) -> String {
    use base64::Engine;
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(utf16)
}

pub struct MihomoManager {
    binary_path: PathBuf,
    config_path: Option<PathBuf>,
    process: Option<Child>,
    elevated: bool,
    idle: bool,
    use_service: bool,
    #[cfg(target_os = "macos")]
    tun_session: Option<std::os::unix::net::UnixStream>,
}

/// How many mihomo processes the system currently has, ours included.
/// A leftover from a crashed run keeps the ports, so a second one would come
/// up useless and route nothing.
fn running_instances() -> usize {
    #[cfg(windows)]
    {
        let out = Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq mihomo.exe", "/NH"])
            .creation_flags_win(CREATE_NO_WINDOW)
            .output();
        return match out {
            Ok(o) => String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| l.to_lowercase().contains("mihomo.exe"))
                .count(),
            Err(_) => 0,
        };
    }
    #[cfg(unix)]
    {
        let out = Command::new("pgrep").args(["-x", "mihomo"]).output();
        return match out {
            Ok(o) => String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.trim().is_empty())
                .count(),
            Err(_) => 0,
        };
    }
    #[allow(unreachable_code)]
    0
}

impl MihomoManager {
    pub fn new(binary_path: PathBuf) -> Self {
        Self {
            binary_path,
            config_path: None,
            process: None,
            elevated: false,
            idle: false,
            use_service: false,
            #[cfg(target_os = "macos")]
            tun_session: None,
        }
    }

    #[cfg(target_os = "macos")]
    pub fn install_tun_helper(&self, config_path: &Path) -> Result<(), String> {
        let settings = crate::tun_helper::HelperSettings::for_current_user(config_path);
        crate::tun_helper::ensure_installed(&settings, &self.binary_path)
    }

    #[cfg(windows)]
    pub fn install_persistent_service(&self, config_path: &Path) -> Result<(), String> {
        let bin = self.binary_path.to_string_lossy().to_string();
        let home = config_path
            .parent()
            .unwrap_or(config_path)
            .to_string_lossy()
            .to_string();
        let cfg = config_path.to_string_lossy().to_string();
        let bin_path = format!("\\\"{}\\\" -d \\\"{}\\\" -f \\\"{}\\\"", bin, home, cfg);

        let sid = current_user_sid()?;
        let sddl = format!(
            "D:(A;;CCLCSWRPWPDTLOCRRC;;;SY)(A;;CCDCLCSWRPWPDTLOCRSDRCWDWO;;;BA)(A;;CCLCSWLOCRRC;;;IU)(A;;CCLCSWLOCRRC;;;SU)(A;;RPWPCR;;;{})",
            sid
        );

        let script = format!(
            "sc.exe stop {name} | Out-Null;              sc.exe delete {name} | Out-Null;              Start-Sleep -Milliseconds 300;              sc.exe create {name} binPath= '{bin}' type= own start= demand DisplayName= '{disp}' error= ignore | Out-Null;              sc.exe sdset {name} '{sddl}' | Out-Null",
            name = SERVICE_NAME,
            bin = bin_path,
            disp = SERVICE_DISPLAY,
            sddl = sddl,
        );

        let status = Command::new("powershell")
            .args([
                "-WindowStyle",
                "Hidden",
                "-NonInteractive",
                "-Command",
                &format!(
                    "Start-Process powershell -ArgumentList '-WindowStyle','Hidden','-NonInteractive','-Command',\"{}\" -Verb RunAs -WindowStyle Hidden -Wait",
                    script.replace('"', "`\"")
                ),
            ])
            .creation_flags_win(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| format!("elevation failed: {}", e))?;

        if !status.success() {
            return Err("UAC elevation was denied".to_string());
        }
        if !service_exists(SERVICE_NAME) {
            return Err("service was not created".to_string());
        }
        Ok(())
    }

    #[cfg(windows)]
    pub fn ensure_persistent_service(&self, config_path: &Path) -> Result<(), String> {
        if service_exists(SERVICE_NAME) {
            return Ok(());
        }
        self.install_persistent_service(config_path)
    }

    #[cfg(windows)]
    fn start_service(&mut self) -> Result<(), String> {
        let out = Command::new("sc")
            .args(["start", SERVICE_NAME])
            .creation_flags_win(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| e.to_string())?;

        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();

        if out.status.success()
            || stdout.contains("RUNNING")
            || stderr.contains("1056")
            || stdout.contains("1056")
        {
            self.use_service = true;
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1500);
            while std::time::Instant::now() < deadline {
                if service_running(SERVICE_NAME) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            return Ok(());
        }

        Err(format!("sc start failed: {} {}", stdout, stderr))
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
            .args(["/F", "/IM", "mihomo.exe"])
            .creation_flags_win(CREATE_NO_WINDOW)
            .output()
            .ok();
        self.use_service = false;
        Ok(())
    }

    pub fn start(&mut self, config_path: &Path, tun: bool) -> Result<(), String> {
        self.config_path = Some(config_path.to_path_buf());
        #[cfg(windows)]
        if self.elevated && self.reload_config(config_path) {
            self.idle = false;
            return Ok(());
        }
        self.idle = false;
        if self.is_running() {
            self.stop()?;
        }

        // A leftover instance holds the ports, so starting a second one would
        // produce a process that routes nothing while the old config stays in
        // charge. Better to fail loudly than to run two.
        if running_instances() > 0 {
            self.stop()?;
            if running_instances() > 0 {
                return Err(
                    "another mihomo is already running and could not be stopped; \
                     close it and try again"
                        .to_string(),
                );
            }
        }

        #[cfg(not(windows))]
        let _ = tun;

        #[cfg(windows)]
        {
            if !tun {
                return self.start_direct(config_path);
            }
            if service_exists(SERVICE_NAME) && self.start_service().is_ok() {
                return Ok(());
            }
            return if is_admin() {
                remove_stale_tun();
                self.start_direct(config_path)
            } else {
                self.start_via_task(config_path)
            };
        }

        #[cfg(target_os = "macos")]
        {
            self.install_tun_helper(config_path)?;
            let socket = crate::tun_helper::socket_path();
            self.tun_session = Some(crate::tun_helper::request_start(&socket)?);
            return Ok(());
        }

        #[cfg(all(unix, not(target_os = "macos")))]
        {
            // Best-effort: raise TUN capabilities on the binary (via pkexec if a
            // polkit agent is present) so mihomo can build the tun device without
            // root. If that is not possible (no pkexec/agent) we still start it
            // directly — mihomo comes up unprivileged and never hard-fails on
            // elevation; the mixed-port proxy works and TUN is best-effort.
            self.ensure_writable_binary();
            if !is_admin() && !mihomo_has_caps(&self.binary_path) {
                let _ = self.grant_caps();
            }
            return self.start_direct(config_path);
        }

        #[allow(unreachable_code)]
        Err("unsupported platform".to_string())
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    fn ensure_writable_binary(&mut self) {
        use std::os::unix::fs::PermissionsExt;

        let Some(dir) = self.binary_path.parent().map(|d| d.to_path_buf()) else {
            return;
        };
        let probe = dir.join(".whisp-write-probe");
        if std::fs::write(&probe, b"1").is_ok() {
            let _ = std::fs::remove_file(&probe);
            return;
        }

        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        let dst_dir = base.join("com.whispera.whisp").join("bin");
        if std::fs::create_dir_all(&dst_dir).is_err() {
            return;
        }
        let Some(name) = self.binary_path.file_name() else {
            return;
        };
        let dst = dst_dir.join(name);

        let same = match (
            std::fs::metadata(&dst),
            std::fs::metadata(&self.binary_path),
        ) {
            (Ok(a), Ok(b)) => a.len() == b.len(),
            _ => false,
        };
        if !same && std::fs::copy(&self.binary_path, &dst).is_err() {
            return;
        }
        let _ = std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o755));
        self.binary_path = dst;
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    fn grant_caps(&self) -> Result<(), String> {
        let status = Command::new("pkexec")
            .arg(find_tool("setcap"))
            .arg("cap_net_admin,cap_net_bind_service=+ep")
            .arg(&self.binary_path)
            .status()
            .map_err(|e| format!("pkexec setcap: {}", e))?;
        if status.success() {
            Ok(())
        } else {
            Err("setcap denied".to_string())
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn start_direct(&mut self, config_path: &Path) -> Result<(), String> {
        let home_dir = config_path.parent().unwrap_or(config_path);
        let mut cmd = Command::new(&self.binary_path);
        cmd.arg("-d").arg(home_dir).arg("-f").arg(config_path);
        cmd.stdout(Stdio::null()).stderr(Stdio::null());

        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);

        let child = cmd
            .spawn()
            .map_err(|e| format!("Failed to start mihomo: {}", e))?;

        self.process = Some(child);
        self.elevated = false;
        self.use_service = false;
        Ok(())
    }

    #[cfg(windows)]
    fn start_via_task(&mut self, config_path: &Path) -> Result<(), String> {
        use sha2::{Digest, Sha256};

        let home_dir = config_path.parent().unwrap_or(config_path);
        let pid_file = home_dir.join(OWNER_PID_FILE);
        let owner = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.file_stem().map(|s| s.to_string_lossy().to_string()))
            .ok_or("cannot resolve Whisp's own executable")?;
        let script = tun_task_script(
            &self.binary_path.to_string_lossy(),
            &home_dir.to_string_lossy(),
            &config_path.to_string_lossy(),
            &pid_file.to_string_lossy(),
            &owner,
        );
        let task_args = format!(
            "-NoProfile -NonInteractive -WindowStyle Hidden -EncodedCommand {}",
            encoded_powershell(&script)
        );
        let stamp = format!("{:x}", Sha256::digest(task_args.as_bytes()));
        let stamp_path = home_dir.join(TUN_TASK_STAMP_FILE);

        std::fs::write(&pid_file, std::process::id().to_string())
            .map_err(|e| format!("owner pid file: {e}"))?;
        let registered = std::fs::read_to_string(&stamp_path).is_ok_and(|s| s == stamp);
        if !(registered && run_tun_task()) {
            register_tun_task(&task_args)?;
            let _ = std::fs::write(&stamp_path, &stamp);
            if !run_tun_task() {
                return Err(format!("scheduled task {TUN_TASK} did not start"));
            }
        }

        self.process = None;
        self.elevated = true;
        self.use_service = false;
        Ok(())
    }

    fn controller_call(&self, method: &str, path: &str, body: &str) -> bool {
        let Some(config) = self
            .config_path
            .as_deref()
            .and_then(|path| std::fs::read_to_string(path).ok())
        else {
            return false;
        };
        matches!(
            http_request(
                CONTROLLER_ADDR,
                method,
                path,
                controller_secret(&config),
                body,
                CONTROLLER_TIMEOUT,
            ),
            Some((200..=299, _))
        )
    }

    fn close_tun(&self) -> bool {
        self.controller_call("PATCH", "/configs", TUN_OFF)
    }

    #[cfg(windows)]
    fn reload_config(&self, config_path: &Path) -> bool {
        let path = config_path.to_string_lossy().replace('\\', "/");
        let body = serde_json::json!({ "path": path }).to_string();
        self.controller_call("PUT", "/configs?force=true", &body)
    }

    #[cfg(windows)]
    fn release_tun_task(&self) {
        let Some(home_dir) = self.config_path.as_deref().and_then(Path::parent) else {
            return;
        };
        let _ = std::fs::remove_file(home_dir.join(OWNER_PID_FILE));
        let deadline = std::time::Instant::now() + TUN_STOP_WAIT;
        while running_instances() > 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    pub fn is_idle(&self) -> bool {
        self.idle
    }

    pub fn stop_tun(&mut self) -> Result<(), String> {
        #[cfg(windows)]
        if self.elevated && self.close_tun() {
            self.idle = true;
            return Ok(());
        }
        self.stop()
    }

    pub fn stop(&mut self) -> Result<(), String> {
        self.close_tun();
        self.idle = false;
        #[cfg(windows)]
        if self.elevated {
            self.release_tun_task();
        }
        if self.use_service {
            return self.stop_service();
        }

        #[cfg(target_os = "macos")]
        if let Some(mut session) = self.tun_session.take() {
            let _ = crate::tun_helper::request_stop(&mut session);
        }

        if let Some(ref mut child) = self.process {
            child.kill().ok();
            child.wait().ok();
        }

        #[cfg(windows)]
        {
            Command::new("taskkill")
                .args(["/F", "/IM", "mihomo.exe"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags_win(CREATE_NO_WINDOW)
                .spawn()
                .ok()
                .and_then(|mut c| c.wait().ok());

            if (self.elevated || self.process.is_none()) && running_instances() > 0 {
                Command::new("powershell")
                    .args([
                        "-WindowStyle", "Hidden", "-NonInteractive", "-Command",
                        "Start-Process -FilePath 'taskkill' -ArgumentList '/F','/IM','mihomo.exe' -Verb RunAs -WindowStyle Hidden -Wait",
                    ])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .creation_flags_win(CREATE_NO_WINDOW)
                    .spawn()
                    .ok()
                    .and_then(|mut c| c.wait().ok());
            }
        }

        #[cfg(unix)]
        {
            Command::new("pkill")
                .args(["-9", "-x", "mihomo"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .ok();
        }

        self.process = None;
        self.elevated = false;
        Ok(())
    }

    pub fn kill_all_by_name(&self) {
        #[cfg(windows)]
        {
            Command::new("taskkill")
                .args(["/F", "/IM", "mihomo.exe"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags_win(CREATE_NO_WINDOW)
                .output()
                .ok();
        }
        #[cfg(unix)]
        {
            Command::new("pkill")
                .args(["-9", "-x", "mihomo"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .ok();
        }
    }

    pub fn is_running(&mut self) -> bool {
        if self.idle {
            return false;
        }
        if self.use_service {
            return api_reachable();
        }

        #[cfg(target_os = "macos")]
        if self.tun_session.is_some() {
            return crate::tun_helper::is_running(&crate::tun_helper::socket_path())
                .unwrap_or(false);
        }

        if let Some(ref mut child) = self.process {
            match child.try_wait() {
                Ok(Some(status)) => {
                    crate::go_client::note_exit("mihomo", status);
                    self.process = None;
                    false
                }
                Ok(None) => true,
                Err(_) => false,
            }
        } else {
            api_reachable()
        }
    }
}

impl Drop for MihomoManager {
    fn drop(&mut self) {
        self.stop_tun().ok();
    }
}

#[cfg(windows)]
fn is_admin() -> bool {
    Command::new("net")
        .arg("session")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags_win(CREATE_NO_WINDOW)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn is_admin() -> bool {
    Command::new("id")
        .arg("-u")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
        .unwrap_or(false)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn find_tool(name: &str) -> String {
    for dir in ["/usr/sbin", "/sbin", "/usr/bin", "/bin"] {
        let p = format!("{}/{}", dir, name);
        if Path::new(&p).exists() {
            return p;
        }
    }
    name.to_string()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn mihomo_has_caps(bin: &Path) -> bool {
    Command::new(find_tool("getcap"))
        .arg(bin)
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .to_lowercase()
                .contains("cap_net_admin")
        })
        .unwrap_or(false)
}

#[cfg(windows)]
fn current_user_sid() -> Result<String, String> {
    let out = Command::new("powershell")
        .args([
            "-NonInteractive",
            "-Command",
            "[Security.Principal.WindowsIdentity]::GetCurrent().User.Value",
        ])
        .creation_flags_win(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("sid lookup: {}", e))?;
    let sid = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if sid.is_empty() {
        return Err("could not read the current user SID".to_string());
    }
    Ok(sid)
}

#[cfg(windows)]
fn remove_stale_tun() {
    let _ = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-EncodedCommand",
            &encoded_powershell(&remove_stale_tun_script()),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags_win(CREATE_NO_WINDOW)
        .status();
}

#[cfg(windows)]
fn register_tun_task(task_args: &str) -> Result<(), String> {
    let script = register_tun_task_script(task_args, &current_user_sid()?);
    let ps_cmd = format!(
        "$p = Start-Process -FilePath 'powershell' -ArgumentList '-NoProfile','-NonInteractive','-WindowStyle','Hidden','-EncodedCommand','{}' -Verb RunAs -WindowStyle Hidden -Wait -PassThru; exit $p.ExitCode",
        encoded_powershell(&script)
    );
    let status = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps_cmd])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags_win(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| format!("Failed to elevate: {e}"))?;
    if !status.success() {
        return Err(format!(
            "UAC elevation was denied or scheduled task {TUN_TASK} could not be registered"
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn run_tun_task() -> bool {
    Command::new("schtasks")
        .args(["/run", "/tn", TUN_TASK])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags_win(CREATE_NO_WINDOW)
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(windows)]
fn service_exists(name: &str) -> bool {
    Command::new("sc")
        .args(["query", name])
        .creation_flags_win(CREATE_NO_WINDOW)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// A listening port only proves something holds it. Asking mihomo's own API for
// its version proves mihomo is up and serving, which is what the caller means.
// A listening port only proves something holds it. Asking the process's own
// control API proves it is up and serving, which is what the caller means.
// A listening port only proves something holds it. Asking the process's own
// control API proves it is up and serving, which is what the caller means.
pub(crate) fn http_get(addr: &str, path: &str) -> Option<(u16, String)> {
    http_request(
        addr,
        "GET",
        path,
        None,
        "",
        std::time::Duration::from_millis(400),
    )
}

fn http_request(
    addr: &str,
    method: &str,
    path: &str,
    secret: Option<&str>,
    body: &str,
    timeout: std::time::Duration,
) -> Option<(u16, String)> {
    use std::io::{Read, Write};

    let addr = addr.parse::<std::net::SocketAddr>().ok()?;
    let mut conn = std::net::TcpStream::connect_timeout(&addr, timeout).ok()?;
    let _ = conn.set_read_timeout(Some(timeout));
    let _ = conn.set_write_timeout(Some(timeout));
    let auth = secret
        .filter(|s| !s.is_empty())
        .map(|s| format!("Authorization: Bearer {s}\r\n"))
        .unwrap_or_default();
    let content = if body.is_empty() {
        String::new()
    } else {
        format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        )
    };
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{auth}{content}Connection: close\r\n\r\n{body}"
    );
    conn.write_all(req.as_bytes()).ok()?;

    let mut raw = Vec::new();
    conn.take(64 * 1024).read_to_end(&mut raw).ok()?;
    let text = String::from_utf8_lossy(&raw);
    let code = text
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse::<u16>().ok())?;
    let body = text.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    Some((code, body))
}

pub(crate) fn http_ok(addr: &str, path: &str) -> bool {
    matches!(http_get(addr, path), Some((200, _)) | Some((401, _)))
}

fn api_reachable() -> bool {
    http_ok(CONTROLLER_ADDR, "/version")
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

pub fn external_proxy_yaml(link: &str, name: &str) -> Option<String> {
    let link = link.trim();
    let (scheme, rest) = link.split_once("://")?;
    let (rest, _tag) = match rest.split_once('#') {
        Some((r, t)) => (r, t),
        None => (rest, ""),
    };
    let (body, query) = match rest.split_once('?') {
        Some((b, q)) => (b, q),
        None => (rest, ""),
    };
    let opts = query_map(query);

    match scheme.to_ascii_lowercase().as_str() {
        "vless" => {
            let (uuid, hostport) = body.split_once('@')?;
            let (host, port) = split_host_port(hostport)?;
            let mut y = format!(
                "  - name: {name}\n    type: vless\n    server: {host}\n    port: {port}\n    uuid: {uuid}\n    udp: true\n"
            );
            let network = opts.get("type").map(String::as_str).unwrap_or("tcp");
            y.push_str(&format!(
                "    network: {}\n",
                if network.is_empty() { "tcp" } else { network }
            ));
            let security = opts.get("security").map(String::as_str).unwrap_or("");
            if security == "tls" || security == "reality" {
                y.push_str("    tls: true\n");
            }
            if let Some(sni) = opts.get("sni").filter(|v| !v.is_empty()) {
                y.push_str(&format!("    servername: {sni}\n"));
            }
            if let Some(fp) = opts.get("fp").filter(|v| !v.is_empty()) {
                y.push_str(&format!("    client-fingerprint: {fp}\n"));
            }
            if let Some(flow) = opts.get("flow").filter(|v| !v.is_empty()) {
                y.push_str(&format!("    flow: {flow}\n"));
            }
            if security == "reality" {
                let pbk = opts.get("pbk").filter(|v| !v.is_empty())?;
                y.push_str("    reality-opts:\n");
                y.push_str(&format!("      public-key: {pbk}\n"));
                if let Some(sid) = opts.get("sid").filter(|v| !v.is_empty()) {
                    y.push_str(&format!("      short-id: \"{sid}\"\n"));
                }
            }
            Some(y)
        }
        "hysteria2" | "hy2" => {
            let (password, hostport) = body.split_once('@')?;
            let (host, port) = split_host_port(hostport)?;
            let mut y = format!(
                "  - name: {name}\n    type: hysteria2\n    server: {host}\n    port: {port}\n    password: \"{password}\"\n"
            );
            if let Some(sni) = opts.get("sni").filter(|v| !v.is_empty()) {
                y.push_str(&format!("    sni: {sni}\n"));
            }
            if opts.get("insecure").map(String::as_str) == Some("1") {
                y.push_str("    skip-cert-verify: true\n");
            }
            if let Some(obfs) = opts.get("obfs").filter(|v| !v.is_empty()) {
                y.push_str(&format!("    obfs: {obfs}\n"));
                if let Some(pw) = opts.get("obfs-password").filter(|v| !v.is_empty()) {
                    y.push_str(&format!("    obfs-password: {pw}\n"));
                }
            }
            Some(y)
        }
        "trojan" => {
            let (password, hostport) = body.split_once('@')?;
            let (host, port) = split_host_port(hostport)?;
            let mut y = format!(
                "  - name: {name}\n    type: trojan\n    server: {host}\n    port: {port}\n    password: \"{password}\"\n    udp: true\n"
            );
            if let Some(sni) = opts.get("sni").filter(|v| !v.is_empty()) {
                y.push_str(&format!("    sni: {sni}\n"));
            }
            if let Some(fp) = opts.get("fp").filter(|v| !v.is_empty()) {
                y.push_str(&format!("    client-fingerprint: {fp}\n"));
            }
            Some(y)
        }
        "ss" => {
            let (userinfo, hostport) = body.split_once('@')?;
            let decoded = decode_b64(userinfo).unwrap_or_else(|| userinfo.to_string());
            let (cipher, password) = decoded.split_once(':')?;
            let (host, port) = split_host_port(hostport)?;
            Some(format!(
                "  - name: {name}\n    type: ss\n    server: {host}\n    port: {port}\n    cipher: {cipher}\n    password: \"{password}\"\n    udp: true\n"
            ))
        }
        _ => None,
    }
}

fn query_map(q: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for pair in q.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        out.insert(k.to_ascii_lowercase(), percent_decode(v));
    }
    out
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn split_host_port(s: &str) -> Option<(String, u16)> {
    let (h, p) = s.rsplit_once(':')?;
    let host = h.trim_matches(|c| c == '[' || c == ']').to_string();
    if host.is_empty() {
        return None;
    }
    Some((host, p.parse().ok()?))
}

fn decode_b64(s: &str) -> Option<String> {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let clean: Vec<u8> = s
        .bytes()
        .map(|c| match c {
            b'-' => b'+',
            b'_' => b'/',
            other => other,
        })
        .filter(|c| *c != b'=' && !c.is_ascii_whitespace())
        .collect();
    let mut bits = 0u32;
    let mut n = 0;
    let mut out = Vec::new();
    for c in clean {
        let v = T.iter().position(|t| *t == c)? as u32;
        bits = (bits << 6) | v;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((bits >> n) as u8);
        }
    }
    String::from_utf8(out).ok()
}

pub struct MihomoRoutingRule {
    pub kind: String,
    pub value: String,
    pub action: String,
}

pub struct MihomoConfig<'a> {
    pub socks_addr: &'a str,
    pub server_host: &'a str,
    pub mixed_port: u16,
    pub tun: bool,
    pub tun_stack: &'a str,
    pub dns_redirect: bool,
    pub ipv6: bool,
    pub routing_rules: &'a [MihomoRoutingRule],
    pub extra_socks_addrs: &'a [String],
    pub custom_dns: &'a [String],
    pub socks_user: &'a str,
    pub socks_pass: &'a str,
    pub allow_lan: bool,
    pub log_level: &'a str,
    pub routing_mode: &'a str,
    pub bypass_ru: bool,
    pub external_link: &'a str,
    pub secret: &'a str,
    pub kill_switch: bool,
    pub vpn_dns: &'a str,
}

const SYSTEM_RESOLVER: &str = "system";
const DEFAULT_VPN_DNS: &str = "1.1.1.1";
const THROUGH_TUNNEL: &str = "#PROXY";

fn tunnelled_nameserver(vpn_dns: &str) -> Option<String> {
    let dns = match vpn_dns.trim() {
        "" => DEFAULT_VPN_DNS,
        set => set,
    };
    if dns.eq_ignore_ascii_case(SYSTEM_RESOLVER) || !valid_nameserver(dns) {
        return None;
    }
    if dns.contains("://") {
        Some(format!("{dns}{THROUGH_TUNNEL}"))
    } else {
        Some(format!("tcp://{dns}{THROUGH_TUNNEL}"))
    }
}

fn valid_nameserver(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    if s == "system" || s.contains("://") {
        return true;
    }
    if s.parse::<std::net::IpAddr>().is_ok() || s.parse::<std::net::SocketAddr>().is_ok() {
        return true;
    }
    let host = match s.rsplit_once(':') {
        Some((h, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => h,
        _ => s,
    };
    host.len() > 3 && host.contains('.') && !host.starts_with('.') && !host.ends_with('.')
}

const GO_CLIENT_PROCESS: &str = if cfg!(windows) {
    "whispera-go-client.exe"
} else {
    "whispera-go-client"
};

const DNS_LISTEN_ADDR: &str = "127.0.0.1:1053";

pub(crate) const APP_BUNDLE_SUFFIX: &str = ".app";
const BUNDLE_CONTENTS_WILDCARD: &str = "/*";

pub fn generate_config(cfg: &MihomoConfig) -> String {
    let parts: Vec<&str> = cfg.socks_addr.splitn(2, ':').collect();
    let server = parts.first().copied().unwrap_or("127.0.0.1");
    let server_port: u16 = parts
        .get(1)
        .copied()
        .unwrap_or("1080")
        .parse()
        .unwrap_or(1080);

    let port = cfg.mixed_port;
    let tun_stack = cfg.tun_stack.to_lowercase();
    let ipv6 = cfg.ipv6;
    let picked: Vec<&String> = cfg
        .custom_dns
        .iter()
        .filter(|s| valid_nameserver(s))
        .collect();
    let nameservers: String = if picked.is_empty() {
        "    - 77.88.8.8\n    - 77.88.8.1\n    - 8.8.8.8\n    - 1.1.1.1".to_string()
    } else {
        picked
            .iter()
            .map(|s| format!("    - {}", s.trim()))
            .collect::<Vec<_>>()
            .join("\n")
    };

    // Build extra proxy entries and the proxy-group YAML.
    let mut extra_proxies = String::new();
    let mut all_proxy_names = vec!["whisp-server".to_string()];
    for (i, addr) in cfg.extra_socks_addrs.iter().enumerate() {
        let parts: Vec<&str> = addr.splitn(2, ':').collect();
        let h = parts.first().copied().unwrap_or("127.0.0.1");
        let p: u16 = parts
            .get(1)
            .copied()
            .unwrap_or("10900")
            .parse()
            .unwrap_or(10900);
        let name = format!("whisp-extra-{}", i);
        extra_proxies.push_str(&format!(
            "  - name: {name}\n    type: socks5\n    server: {h}\n    port: {p}\n    udp: true\n"
        ));
        all_proxy_names.push(name);
    }

    let proxy_group = if all_proxy_names.len() > 1 {
        let names_yaml: String = all_proxy_names
            .iter()
            .map(|n| format!("      - {}\n", n))
            .collect();
        format!(
            "  - name: PROXY\n    type: load-balance\n    strategy: round-robin\n    url: http://www.gstatic.com/generate_204\n    interval: 30\n    proxies:\n{}",
            names_yaml
        )
    } else {
        "  - name: PROXY\n    type: select\n    proxies:\n      - whisp-server\n".to_string()
    };

    let mut custom_rules = String::new();
    for rule in cfg.routing_rules {
        let action = &rule.action;
        match rule.kind.as_str() {
            "domain" => {
                custom_rules.push_str(&format!("  - DOMAIN-SUFFIX,{},{}\n", rule.value, action));
            }
            "domain-keyword" => {
                custom_rules.push_str(&format!("  - DOMAIN-KEYWORD,{},{}\n", rule.value, action));
            }
            "domain-full" => {
                custom_rules.push_str(&format!("  - DOMAIN,{},{}\n", rule.value, action));
            }
            "process" if rule.value.ends_with(APP_BUNDLE_SUFFIX) => {
                custom_rules.push_str(&format!(
                    "  - PROCESS-PATH-WILDCARD,{}{},{}\n",
                    rule.value, BUNDLE_CONTENTS_WILDCARD, action
                ));
            }
            "process" => {
                let exe_name = std::path::Path::new(&rule.value)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&rule.value);
                custom_rules.push_str(&format!("  - PROCESS-NAME,{},{}\n", exe_name, action));
            }
            "ip" => {
                if rule.value.contains('/') {
                    custom_rules.push_str(&format!(
                        "  - IP-CIDR,{},{},no-resolve\n",
                        rule.value, action
                    ));
                } else {
                    custom_rules.push_str(&format!(
                        "  - IP-CIDR,{}/32,{},no-resolve\n",
                        rule.value, action
                    ));
                }
            }
            _ => {}
        }
    }

    let mut fakeip_extra = String::new();
    for rule in cfg.routing_rules {
        if !rule.action.eq_ignore_ascii_case("DIRECT") {
            continue;
        }
        match rule.kind.as_str() {
            "domain" => fakeip_extra.push_str(&format!("    - \"+.{}\"\n", rule.value)),
            "domain-full" => fakeip_extra.push_str(&format!("    - \"{}\"\n", rule.value)),
            _ => {}
        }
    }

    let primary_proxy = external_proxy_yaml(cfg.external_link, "whisp-server").unwrap_or_else(|| {
        // Reached only when there is no external profile at all: an unparsable one
        // is refused earlier, so a broken link can never quietly become our tunnel.
        format!(
            "  - name: whisp-server\n    type: socks5\n    server: {}\n    port: {}\n    udp: true\n",
            server, server_port
        )
    });

    let ru_rules = if cfg.bypass_ru {
        "  - DOMAIN-SUFFIX,ru,DIRECT\n  - DOMAIN-SUFFIX,su,DIRECT\n  - DOMAIN-SUFFIX,рф,DIRECT\n  - GEOIP,RU,DIRECT,no-resolve\n"
    } else {
        ""
    };

    // DNS redirect: если включён — принудительно резолвим все домены через прокси
    // и отключаем fake-ip (переходим на redir-host). Так DNS-запросы уходят в тоннель,
    // а не к системному резолверу, и клиент видит реальные IP.
    // Без флага — старое поведение (fake-ip, быстрее, но DNS виден провайдеру).
    let dns_enhanced_mode = if cfg.dns_redirect {
        "redir-host"
    } else {
        "fake-ip"
    };
    let dns_proxy_policy = if cfg.dns_redirect {
        format!("  proxy-server-nameserver:\n{}\n", nameservers)
    } else {
        String::new()
    };
    let resolvers = match tunnelled_nameserver(cfg.vpn_dns) {
        Some(ns) if cfg.dns_redirect => format!("    - {ns}"),
        _ => nameservers,
    };
    let controller_secret = if cfg.secret.is_empty() {
        String::new()
    } else {
        format!("secret: \"{}\"\n", cfg.secret)
    };
    let strict_route = cfg.kill_switch;
    let tun = cfg.tun;

    let auth_block = if !cfg.socks_user.is_empty() && !cfg.socks_pass.is_empty() {
        format!(
            "authentication:\n  - \"{}:{}\"\n",
            cfg.socks_user, cfg.socks_pass
        )
    } else {
        String::new()
    };

    let server_ip_valid = cfg.server_host.parse::<std::net::Ipv4Addr>().is_ok();
    let tun_exclude = if server_ip_valid {
        format!("  route-exclude-address:\n    - {}/32\n", cfg.server_host)
    } else {
        String::new()
    };
    let server_direct_rule = if server_ip_valid {
        format!("  - IP-CIDR,{}/32,DIRECT,no-resolve\n", cfg.server_host)
    } else {
        String::new()
    };
    let self_direct_rule = format!("  - PROCESS-NAME,{GO_CLIENT_PROCESS},DIRECT\n");

    let allow_lan = cfg.allow_lan;
    let _ = cfg.log_level;
    let log_level = "info";
    let routing_mode = match cfg.routing_mode {
        "global" | "direct" => cfg.routing_mode,
        _ => "rule",
    };

    format!(
        r#"mixed-port: {port}
allow-lan: {allow_lan}
{auth_block}
ipv6: {ipv6}
mode: {routing_mode}
log-level: {log_level}
external-controller: {CONTROLLER_ADDR}
{controller_secret}find-process-mode: strict

sniffer:
  enable: true
  sniff:
    HTTP:
      ports: [80, 8080-8090]
    TLS:
      ports: [443, 8443]
    QUIC:
      ports: [443, 8443]
  override-destination: true

dns:
  enable: true
  listen: {DNS_LISTEN_ADDR}
{dns_proxy_policy}  enhanced-mode: {dns_enhanced_mode}
  fake-ip-range: 198.18.0.1/16
  fake-ip-filter:
    - "*.ru"
    - "*.su"
    - "*.рф"
    - "*.lan"
    - "*.local"
    - "localhost"
    - "*.localhost"
    - "time.windows.com"
    - "time.nist.gov"
    - "time.apple.com"
    - "+.pool.ntp.org"
    - "+.stun.*.*"
    - "+.stun.*.*.*"
{fakeip_extra}  nameserver:
{resolvers}

tun:
  enable: {tun}
  stack: {tun_stack}
  device: {TUN_DEVICE}
  dns-hijack:
    - any:53
  auto-route: true
  auto-detect-interface: true
  strict-route: {strict_route}
{tun_exclude}
proxies:
{primary_proxy}{extra_proxies}
proxy-groups:
{proxy_group}

rules:
{server_direct_rule}{self_direct_rule}{custom_rules}{ru_rules}  - IP-CIDR,10.0.0.0/8,DIRECT,no-resolve
  - IP-CIDR,172.16.0.0/12,DIRECT,no-resolve
  - IP-CIDR,192.168.0.0/16,DIRECT,no-resolve
  - IP-CIDR,127.0.0.0/8,DIRECT,no-resolve
  - IP-CIDR,100.64.0.0/10,DIRECT,no-resolve
  - MATCH,PROXY
"#
    )
}

#[cfg(test)]
mod tests {
    use super::external_proxy_yaml;

    #[test]
    fn vless_reality_link_becomes_a_proxy() {
        let link = "vless://11111111-2222-3333-4444-555555555555@example.com:443\
?type=tcp&security=reality&pbk=abcPUBKEY&sid=01ab&fp=chrome&sni=www.example.org\
&flow=xtls-rprx-vision#my%20node";
        let y = external_proxy_yaml(link, "whisp-server").expect("must parse");

        for needle in [
            "name: whisp-server",
            "type: vless",
            "server: example.com",
            "port: 443",
            "uuid: 11111111-2222-3333-4444-555555555555",
            "tls: true",
            "servername: www.example.org",
            "client-fingerprint: chrome",
            "flow: xtls-rprx-vision",
            "public-key: abcPUBKEY",
            "short-id: \"01ab\"",
        ] {
            assert!(y.contains(needle), "missing {needle} in:\n{y}");
        }
    }

    #[test]
    fn trojan_link_becomes_a_proxy() {
        let y = external_proxy_yaml("trojan://pass@example.com:8443?sni=cdn.example.org", "p")
            .expect("must parse");
        assert!(y.contains("type: trojan"), "{y}");
        assert!(y.contains("password: \"pass\""), "{y}");
        assert!(y.contains("sni: cdn.example.org"), "{y}");
    }

    #[test]
    fn shadowsocks_link_decodes_userinfo() {
        // base64 of "aes-256-gcm:secret"
        let y = external_proxy_yaml("ss://YWVzLTI1Ni1nY206c2VjcmV0@example.com:8388", "p")
            .expect("must parse");
        assert!(y.contains("cipher: aes-256-gcm"), "{y}");
        assert!(y.contains("password: \"secret\""), "{y}");
    }

    #[test]
    fn unknown_scheme_is_rejected() {
        assert!(external_proxy_yaml("https://example.com", "p").is_none());
        assert!(external_proxy_yaml("", "p").is_none());
    }

    #[test]
    fn direct_domains_leave_the_fake_ip_pool() {
        use super::{generate_config, MihomoRoutingRule};
        let rules = vec![
            MihomoRoutingRule {
                kind: "domain".into(),
                value: "direct.example".into(),
                action: "DIRECT".into(),
            },
            MihomoRoutingRule {
                kind: "domain-full".into(),
                value: "cdn.example.net".into(),
                action: "direct".into(),
            },
            MihomoRoutingRule {
                kind: "domain".into(),
                value: "blocked.example".into(),
                action: "PROXY".into(),
            },
        ];
        let out = generate_config(&test_config(&rules));
        let filter = out
            .split("fake-ip-filter:")
            .nth(1)
            .and_then(|s| s.split("nameserver:").next())
            .expect("config must have a fake-ip-filter section");
        assert!(filter.contains("+.direct.example"), "{filter}");
        assert!(filter.contains("cdn.example.net"), "{filter}");
        assert!(!filter.contains("blocked.example"), "{filter}");
    }

    #[test]
    fn app_rule_covers_every_process_in_the_bundle() {
        use super::{generate_config, MihomoRoutingRule};
        let rules = vec![
            MihomoRoutingRule {
                kind: "process".into(),
                value: "/Applications/Google Chrome.app".into(),
                action: "DIRECT".into(),
            },
            MihomoRoutingRule {
                kind: "process".into(),
                value: "/usr/bin/telegram-desktop".into(),
                action: "PROXY".into(),
            },
        ];
        let out = generate_config(&test_config(&rules));
        assert!(
            out.contains("  - PROCESS-PATH-WILDCARD,/Applications/Google Chrome.app/*,DIRECT\n"),
            "{out}"
        );
        assert!(
            out.contains("  - PROCESS-NAME,telegram-desktop,PROXY\n"),
            "{out}"
        );
    }

    #[test]
    fn sidecar_traffic_is_never_sent_back_to_itself() {
        use super::{generate_config, MihomoRoutingRule, GO_CLIENT_PROCESS};
        let rules = vec![MihomoRoutingRule {
            kind: "domain".into(),
            value: "example.org".into(),
            action: "PROXY".into(),
        }];
        let out = generate_config(&test_config(&rules));
        let own = out
            .find(&format!("PROCESS-NAME,{GO_CLIENT_PROCESS},DIRECT"))
            .expect(
                "without a DIRECT rule the sidecar's bypass dials come back to it through the TUN",
            );
        let custom = out
            .find("example.org,PROXY")
            .expect("custom rule must be in the config");
        assert!(
            own < custom,
            "the sidecar rule must precede every routing rule:\n{out}"
        );
    }

    #[test]
    fn dns_listener_only_serves_this_machine() {
        use super::{generate_config, MihomoConfig};
        for allow_lan in [false, true] {
            let out = generate_config(&MihomoConfig {
                allow_lan,
                ..test_config(&[])
            });
            let listen: std::net::SocketAddr = config_value(&out, "listen")
                .parse()
                .expect("dns listen must be ip:port");
            assert!(
                listen.ip().is_loopback(),
                "allow_lan={allow_lan}: DNS listens on {listen}"
            );
        }
    }

    #[test]
    fn kill_switch_turns_on_strict_route() {
        use super::{generate_config, MihomoConfig};
        for kill_switch in [false, true] {
            let out = generate_config(&MihomoConfig {
                kill_switch,
                ..test_config(&[])
            });
            assert_eq!(
                config_value(&out, "strict-route"),
                kill_switch.to_string(),
                "{out}"
            );
        }
    }

    #[test]
    fn controller_secret_is_written_only_when_set() {
        use super::{generate_config, MihomoConfig};
        let open = generate_config(&test_config(&[]));
        assert!(!open.contains("\nsecret:"), "{open}");
        let locked = generate_config(&MihomoConfig {
            secret: "abc123",
            ..test_config(&[])
        });
        assert_eq!(config_value(&locked, "secret"), "\"abc123\"", "{locked}");
    }

    #[test]
    fn controller_secret_is_read_back_from_the_generated_config() {
        use super::{controller_secret, generate_config, MihomoConfig};
        assert_eq!(controller_secret(&generate_config(&test_config(&[]))), None);
        let locked = generate_config(&MihomoConfig {
            secret: "abc123",
            ..test_config(&[])
        });
        assert_eq!(controller_secret(&locked), Some("abc123"));
    }

    #[test]
    fn stale_tun_is_removed_by_the_id_wintun_gives_the_device() {
        use super::{generate_config, remove_stale_tun_script, TUN_DEVICE};
        let script = remove_stale_tun_script();
        assert!(
            script.contains(&format!("GetBytes('wintun{TUN_DEVICE}')")),
            "{script}"
        );
        assert!(
            script.contains(r#"pnputil.exe /remove-device "SWD\Wintun\{$id}""#),
            "{script}"
        );
        let config = generate_config(&test_config(&[]));
        assert_eq!(config_value(&config, "device"), TUN_DEVICE, "{config}");
    }

    #[test]
    fn tun_task_runs_mihomo_only_for_a_live_whisp() {
        use super::{remove_stale_tun_script, tun_task_script};
        let home = r"C:\Users\O'Neil\AppData\Roaming\com.whispera.whisp";
        let script = tun_task_script(
            r"C:\Program Files\Whisp\mihomo.exe",
            home,
            &format!(r"{home}\mihomo_config.yaml"),
            &format!(r"{home}\whisp-owner.pid"),
            "Whisp",
        );
        let owner_check = script
            .find("$owner.ProcessName -ne 'Whisp'")
            .expect("owner check");
        let cleanup = script
            .find(&remove_stale_tun_script())
            .expect("stale adapter cleanup");
        let launch = script
            .find(r"-FilePath 'C:\Program Files\Whisp\mihomo.exe'")
            .expect("mihomo launch");
        assert!(owner_check < cleanup && cleanup < launch, "{script}");
        assert!(
            script.starts_with(r"$pidFile = 'C:\Users\O''Neil\"),
            "{script}"
        );
        assert!(
            script.contains(
                "if ($owner.HasExited -or -not (Test-Path -LiteralPath $pidFile)) { Stop-Process -Id $mihomo.Id -Force; break }"
            ),
            "{script}"
        );
    }

    #[test]
    fn tun_task_is_registered_elevated_for_the_current_user() {
        use super::{register_tun_task_script, TUN_TASK};
        let script = register_tun_task_script("-EncodedCommand AAAA", "S-1-5-21-1-2-3-1001");
        assert!(
            script.contains("-Execute 'powershell.exe' -Argument '-EncodedCommand AAAA'"),
            "{script}"
        );
        assert!(
            script
                .contains("-UserId 'S-1-5-21-1-2-3-1001' -LogonType Interactive -RunLevel Highest"),
            "{script}"
        );
        assert!(
            script.contains(&format!("-TaskName '{TUN_TASK}'")),
            "{script}"
        );
        assert!(
            script.contains("-ExecutionTimeLimit ([TimeSpan]::Zero)"),
            "{script}"
        );
    }

    #[test]
    fn encoded_powershell_is_utf16_base64_of_the_script() {
        use super::encoded_powershell;
        use base64::Engine;
        let script = "Write-Output 'Мета'";
        let raw = base64::engine::general_purpose::STANDARD
            .decode(encoded_powershell(script))
            .expect("base64");
        let units: Vec<u16> = raw
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        assert_eq!(String::from_utf16(&units).expect("utf16"), script);
    }

    #[test]
    fn tun_is_closed_with_an_authorized_patch() {
        use super::{http_request, TUN_OFF};
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr").to_string();
        let server = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().expect("accept");
            let mut raw = Vec::new();
            let mut buf = [0u8; 1024];
            while !String::from_utf8_lossy(&raw).ends_with(TUN_OFF) {
                let n = conn.read(&mut buf).expect("read");
                assert!(
                    n > 0,
                    "request ended early: {}",
                    String::from_utf8_lossy(&raw)
                );
                raw.extend_from_slice(&buf[..n]);
            }
            conn.write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                .expect("write");
            String::from_utf8_lossy(&raw).to_string()
        });

        let reply = http_request(
            &addr,
            "PATCH",
            "/configs",
            Some("abc123"),
            TUN_OFF,
            std::time::Duration::from_secs(5),
        );
        let request = server.join().expect("server");
        assert_eq!(reply.map(|(code, _)| code), Some(204));
        assert!(
            request.starts_with("PATCH /configs HTTP/1.1\r\n"),
            "{request}"
        );
        assert!(
            request.contains("\r\nAuthorization: Bearer abc123\r\n"),
            "{request}"
        );
        assert!(
            request.contains(&format!("\r\nContent-Length: {}\r\n", TUN_OFF.len())),
            "{request}"
        );
    }

    #[test]
    fn redirected_dns_resolves_through_the_tunnel() {
        use super::{generate_config, MihomoConfig, THROUGH_TUNNEL};
        for (vpn_dns, expected) in [
            ("", "tcp://1.1.1.1#PROXY"),
            ("8.8.8.8", "tcp://8.8.8.8#PROXY"),
            ("77.88.8.8:53", "tcp://77.88.8.8:53#PROXY"),
            (
                "https://1.1.1.1/dns-query",
                "https://1.1.1.1/dns-query#PROXY",
            ),
        ] {
            let out = generate_config(&MihomoConfig {
                dns_redirect: true,
                vpn_dns,
                ..test_config(&[])
            });
            assert_eq!(nameservers(&out), [expected], "{out}");
        }
        for (dns_redirect, vpn_dns) in [(true, "system"), (false, "8.8.8.8")] {
            let out = generate_config(&MihomoConfig {
                dns_redirect,
                vpn_dns,
                ..test_config(&[])
            });
            let listed = nameservers(&out);
            assert!(!listed.is_empty(), "{out}");
            assert!(
                listed.iter().all(|ns| !ns.ends_with(THROUGH_TUNNEL)),
                "{out}"
            );
        }
    }

    fn nameservers(config: &str) -> Vec<&str> {
        config
            .lines()
            .map(str::trim)
            .skip_while(|line| *line != "nameserver:")
            .skip(1)
            .map_while(|line| line.strip_prefix("- "))
            .collect()
    }

    fn config_value<'a>(config: &'a str, key: &str) -> &'a str {
        config
            .lines()
            .map(str::trim)
            .find_map(|line| line.strip_prefix(key)?.strip_prefix(": "))
            .unwrap_or_else(|| panic!("config has no {key}:\n{config}"))
    }

    mod with_sidecar {
        use super::{config_value, test_config};
        use crate::mihomo::{generate_config, http_ok, MihomoConfig};
        use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
        use std::path::PathBuf;
        use std::process::{Child, Command};
        use std::time::{Duration, Instant};

        const MIHOMO_BIN_ENV: &str = "WHISP_MIHOMO_BIN";
        const STARTUP_TIMEOUT: Duration = Duration::from_secs(20);
        const RETRY_INTERVAL: Duration = Duration::from_millis(200);
        const QUERY_TIMEOUT: Duration = Duration::from_secs(1);

        const UNROUTED_ADDR: Ipv4Addr = Ipv4Addr::new(192, 0, 2, 1);
        const STANDARD_DNS_PORT: u16 = 53;
        const PROBE_DOMAIN: &str = "example.com";

        const DNS_RECURSION_DESIRED: u16 = 0x0100;
        const DNS_TYPE_A: u16 = 1;
        const DNS_CLASS_IN: u16 = 1;
        const DNS_NAME_POINTER: u8 = 0xC0;
        const DNS_HEADER_LEN: usize = 12;
        const DNS_QUESTION_FIXED_LEN: usize = 4;
        const DNS_RECORD_FIXED_LEN: usize = 10;
        const DNS_MAX_UDP_MESSAGE: usize = 512;

        const CONTROLLER_SECRET: &str = "0123456789abcdef";
        const HTTP_OK: u16 = 200;
        const HTTP_UNAUTHORIZED: u16 = 401;

        static ONE_SIDECAR_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

        #[test]
        #[ignore = "needs the mihomo binary and a TUN device; run docker/run-checks.sh"]
        fn controller_answers_only_with_the_secret() {
            let _turn = ONE_SIDECAR_AT_A_TIME
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let config = generate_config(&MihomoConfig {
                secret: CONTROLLER_SECRET,
                kill_switch: true,
                dns_redirect: true,
                vpn_dns: "1.1.1.1",
                ..test_config(&[])
            });
            let mihomo = Sidecar::start(&config);
            let controller = config_value(&config, "external-controller");
            assert!(
                retry(|| http_ok(controller, "/version").then_some(())).is_some(),
                "mihomo did not come up:\n{}",
                mihomo.log()
            );

            let (anonymous, _) = controller_get(controller, "/version", None)
                .unwrap_or_else(|| panic!("no answer from {controller}:\n{}", mihomo.log()));
            assert_eq!(anonymous, HTTP_UNAUTHORIZED);

            let (status, body) = controller_get(controller, "/configs", Some(CONTROLLER_SECRET))
                .unwrap_or_else(|| panic!("no answer from {controller}:\n{}", mihomo.log()));
            assert_eq!(status, HTTP_OK, "{body}");
            assert!(body.contains("\"strict-route\":true"), "{body}");
        }

        const TUNNEL_RESOLVER: SocketAddr =
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), STANDARD_DNS_PORT);
        const SOCKS_VERSION: u8 = 5;
        const SOCKS_NO_AUTH: u8 = 0;
        const SOCKS_ATYP_IPV4: u8 = 1;

        #[test]
        #[ignore = "needs the mihomo binary and a TUN device; run docker/run-checks.sh"]
        fn redirected_dns_leaves_through_the_tunnel() {
            let _turn = ONE_SIDECAR_AT_A_TIME
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let resolver = TUNNEL_RESOLVER.ip().to_string();
            let config = generate_config(&MihomoConfig {
                dns_redirect: true,
                vpn_dns: &resolver,
                ..test_config(&[])
            });
            let listen: SocketAddr = config_value(&config, "listen")
                .parse()
                .expect("dns listen must be ip:port");
            let tunnel = std::net::TcpListener::bind(test_config(&[]).socks_addr)
                .expect("stand in for the tunnel's SOCKS port");
            tunnel.set_nonblocking(true).expect("non-blocking listener");

            let mihomo = Sidecar::start(&config);
            let target = retry(|| {
                let _ = resolve_a(listen, PROBE_DOMAIN);
                let (conn, _) = tunnel.accept().ok()?;
                socks_connect_target(conn)
            })
            .unwrap_or_else(|| panic!("no DNS query reached the tunnel:\n{}", mihomo.log()));
            assert_eq!(target, TUNNEL_RESOLVER);
        }

        fn socks_connect_target(mut conn: std::net::TcpStream) -> Option<SocketAddr> {
            use std::io::{Read, Write};

            conn.set_nonblocking(false).ok()?;
            conn.set_read_timeout(Some(QUERY_TIMEOUT)).ok()?;
            let mut greeting = [0u8; 2];
            conn.read_exact(&mut greeting).ok()?;
            let mut methods = vec![0u8; greeting[1] as usize];
            conn.read_exact(&mut methods).ok()?;
            conn.write_all(&[SOCKS_VERSION, SOCKS_NO_AUTH]).ok()?;
            let mut request = [0u8; 4];
            conn.read_exact(&mut request).ok()?;
            if request[3] != SOCKS_ATYP_IPV4 {
                return None;
            }
            let mut addr = [0u8; 6];
            conn.read_exact(&mut addr).ok()?;
            let ip = Ipv4Addr::new(addr[0], addr[1], addr[2], addr[3]);
            Some(SocketAddr::from((
                ip,
                u16::from_be_bytes([addr[4], addr[5]]),
            )))
        }

        fn controller_get(addr: &str, path: &str, secret: Option<&str>) -> Option<(u16, String)> {
            use std::io::{Read, Write};
            use std::net::TcpStream;

            let mut conn = TcpStream::connect_timeout(&addr.parse().ok()?, QUERY_TIMEOUT).ok()?;
            conn.set_read_timeout(Some(QUERY_TIMEOUT)).ok()?;
            let auth = secret
                .map(|s| format!("Authorization: Bearer {s}\r\n"))
                .unwrap_or_default();
            let request = format!(
                "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{auth}Connection: close\r\n\r\n"
            );
            conn.write_all(request.as_bytes()).ok()?;
            let mut raw = String::new();
            conn.read_to_string(&mut raw).ok()?;
            let status = raw.split_whitespace().nth(1)?.parse().ok()?;
            let body = raw
                .split_once("\r\n\r\n")
                .map_or("", |(_, b)| b)
                .to_string();
            Some((status, body))
        }

        #[test]
        #[ignore = "needs the mihomo binary and a TUN device; run docker/run-checks.sh"]
        fn dns_answers_through_tun_but_not_from_the_network() {
            let _turn = ONE_SIDECAR_AT_A_TIME
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let config = generate_config(&MihomoConfig {
                bypass_ru: false,
                ..test_config(&[])
            });
            let listen: SocketAddr = config_value(&config, "listen")
                .parse()
                .expect("dns listen must be ip:port");
            let fake_ip_range = config_value(&config, "fake-ip-range");
            let lan_ip = outward_ip();

            let mihomo = Sidecar::start(&config);
            let controller = config_value(&config, "external-controller");
            assert!(
                retry(|| http_ok(controller, "/version").then_some(())).is_some(),
                "mihomo did not come up:\n{}",
                mihomo.log()
            );

            let through_tun = retry(|| {
                resolve_a(
                    SocketAddr::from((UNROUTED_ADDR, STANDARD_DNS_PORT)),
                    PROBE_DOMAIN,
                )
            })
            .unwrap_or_else(|| panic!("no DNS answer through the TUN:\n{}", mihomo.log()));
            assert!(
                in_cidr(through_tun, fake_ip_range),
                "{through_tun} is outside the fake-ip range {fake_ip_range}"
            );

            assert!(
                resolve_a(listen, PROBE_DOMAIN).is_some(),
                "the DNS listener does not answer on {listen} itself"
            );
            let from_network = resolve_a(SocketAddr::new(lan_ip, listen.port()), PROBE_DOMAIN);
            assert_eq!(
                from_network, None,
                "the DNS listener answered on the network address {lan_ip}"
            );
        }

        struct Sidecar {
            process: Child,
            home: PathBuf,
        }

        impl Sidecar {
            fn start(config: &str) -> Self {
                let bin = std::env::var_os(MIHOMO_BIN_ENV)
                    .unwrap_or_else(|| panic!("set {MIHOMO_BIN_ENV} to a mihomo binary"));
                let home =
                    std::env::temp_dir().join(format!("whisp-mihomo-test-{}", std::process::id()));
                std::fs::create_dir_all(&home).expect("create mihomo home");
                let config_path = home.join("config.yaml");
                std::fs::write(&config_path, config).expect("write mihomo config");
                let log = std::fs::File::create(home.join("mihomo.log")).expect("create log");
                let process = Command::new(bin)
                    .arg("-d")
                    .arg(&home)
                    .arg("-f")
                    .arg(&config_path)
                    .stdout(log.try_clone().expect("clone log handle"))
                    .stderr(log)
                    .spawn()
                    .expect("start mihomo");
                Self { process, home }
            }

            fn log(&self) -> String {
                std::fs::read_to_string(self.home.join("mihomo.log")).unwrap_or_default()
            }
        }

        impl Drop for Sidecar {
            fn drop(&mut self) {
                let _ = self.process.kill();
                let _ = self.process.wait();
                let _ = std::fs::remove_dir_all(&self.home);
            }
        }

        fn retry<T>(mut attempt: impl FnMut() -> Option<T>) -> Option<T> {
            let deadline = Instant::now() + STARTUP_TIMEOUT;
            loop {
                if let Some(value) = attempt() {
                    return Some(value);
                }
                if Instant::now() >= deadline {
                    return None;
                }
                std::thread::sleep(RETRY_INTERVAL);
            }
        }

        fn outward_ip() -> IpAddr {
            let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).expect("bind probe socket");
            socket
                .connect((UNROUTED_ADDR, STANDARD_DNS_PORT))
                .expect("route to the probe address");
            socket.local_addr().expect("probe socket address").ip()
        }

        fn in_cidr(ip: Ipv4Addr, cidr: &str) -> bool {
            let (base, prefix) = cidr.split_once('/').expect("cidr must be base/prefix");
            let base: Ipv4Addr = base.parse().expect("cidr base must be IPv4");
            let prefix: u32 = prefix.parse().expect("cidr prefix must be a number");
            let mask = u32::MAX.checked_shl(32 - prefix).unwrap_or(0);
            u32::from(ip) & mask == u32::from(base) & mask
        }

        fn resolve_a(server: SocketAddr, name: &str) -> Option<Ipv4Addr> {
            let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
            socket.set_read_timeout(Some(QUERY_TIMEOUT)).ok()?;
            let id = std::process::id() as u16;
            socket.send_to(&a_query(id, name), server).ok()?;
            let mut reply = [0u8; DNS_MAX_UDP_MESSAGE];
            let (len, _) = socket.recv_from(&mut reply).ok()?;
            first_a_record(&reply[..len], id)
        }

        fn a_query(id: u16, name: &str) -> Vec<u8> {
            let mut message = Vec::new();
            message.extend_from_slice(&id.to_be_bytes());
            message.extend_from_slice(&DNS_RECURSION_DESIRED.to_be_bytes());
            for count in [1u16, 0, 0, 0] {
                message.extend_from_slice(&count.to_be_bytes());
            }
            for label in name.split('.') {
                message.push(label.len() as u8);
                message.extend_from_slice(label.as_bytes());
            }
            message.push(0);
            message.extend_from_slice(&DNS_TYPE_A.to_be_bytes());
            message.extend_from_slice(&DNS_CLASS_IN.to_be_bytes());
            message
        }

        fn first_a_record(reply: &[u8], id: u16) -> Option<Ipv4Addr> {
            let header = reply.get(..DNS_HEADER_LEN)?;
            if u16::from_be_bytes([header[0], header[1]]) != id {
                return None;
            }
            let questions = u16::from_be_bytes([header[4], header[5]]);
            let answers = u16::from_be_bytes([header[6], header[7]]);
            let mut pos = DNS_HEADER_LEN;
            for _ in 0..questions {
                pos = skip_name(reply, pos)? + DNS_QUESTION_FIXED_LEN;
            }
            for _ in 0..answers {
                pos = skip_name(reply, pos)?;
                let fixed = reply.get(pos..pos + DNS_RECORD_FIXED_LEN)?;
                let kind = u16::from_be_bytes([fixed[0], fixed[1]]);
                let data_len = usize::from(u16::from_be_bytes([fixed[8], fixed[9]]));
                pos += DNS_RECORD_FIXED_LEN;
                let data = reply.get(pos..pos + data_len)?;
                if kind == DNS_TYPE_A {
                    if let [a, b, c, d] = *data {
                        return Some(Ipv4Addr::new(a, b, c, d));
                    }
                }
                pos += data_len;
            }
            None
        }

        fn skip_name(message: &[u8], mut pos: usize) -> Option<usize> {
            loop {
                let len = *message.get(pos)?;
                if len & DNS_NAME_POINTER == DNS_NAME_POINTER {
                    return Some(pos + 2);
                }
                pos += 1 + usize::from(len);
                if len == 0 {
                    return Some(pos);
                }
            }
        }
    }

    fn test_config(rules: &[super::MihomoRoutingRule]) -> super::MihomoConfig<'_> {
        super::MihomoConfig {
            socks_addr: "127.0.0.1:1081",
            server_host: "example.com",
            mixed_port: 7890,
            tun: true,
            tun_stack: "gvisor",
            dns_redirect: false,
            ipv6: false,
            routing_rules: rules,
            extra_socks_addrs: &[],
            custom_dns: &[],
            socks_user: "",
            socks_pass: "",
            allow_lan: false,
            log_level: "info",
            routing_mode: "rule",
            bypass_ru: true,
            external_link: "",
            secret: "",
            kill_switch: false,
            vpn_dns: "",
        }
    }
}

#[cfg(test)]
mod hysteria_tests {
    use super::external_proxy_yaml;

    #[test]
    fn hysteria2_link_becomes_a_proxy() {
        let y = external_proxy_yaml(
            "hysteria2://secret@example.com:8443?sni=cdn.example.org&insecure=1",
            "whisp-server",
        )
        .expect("must parse");
        assert!(y.contains("type: hysteria2"), "{y}");
        assert!(y.contains("password: \"secret\""), "{y}");
        assert!(y.contains("sni: cdn.example.org"), "{y}");
        assert!(y.contains("skip-cert-verify: true"), "{y}");
    }
}
