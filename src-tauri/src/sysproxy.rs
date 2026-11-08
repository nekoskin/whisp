use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProxySettings {
    pub flags: u32,
    pub server: Option<String>,
    pub bypass: Option<String>,
    pub pac_url: Option<String>,
    pub http_proxy: Option<String>,
    pub https_proxy: Option<String>,
    pub no_proxy: Option<String>,
}

pub trait ProxyStore {
    fn read(&self) -> Result<ProxySettings, String>;
    fn write(&self, settings: &ProxySettings) -> Result<(), String>;
}

pub fn apply(store: &impl ProxyStore, backup: &Path, ours: &ProxySettings) -> Result<(), String> {
    if !backup.exists() {
        let previous = serde_json::to_string(&store.read()?).map_err(|e| e.to_string())?;
        std::fs::write(backup, previous).map_err(|e| format!("system proxy backup: {e}"))?;
    }
    store.write(ours)
}

pub fn restore(store: &impl ProxyStore, backup: &Path) -> Result<(), String> {
    let Ok(raw) = std::fs::read_to_string(backup) else {
        return Ok(());
    };
    let previous = serde_json::from_str::<ProxySettings>(&raw).unwrap_or_default();
    store.write(&previous)?;
    std::fs::remove_file(backup).map_err(|e| format!("system proxy backup: {e}"))
}

#[cfg(windows)]
pub use windows_store::{manual, InternetOptions};

#[cfg(windows)]
mod windows_store {
    use super::{ProxySettings, ProxyStore};
    use std::ffi::c_void;
    use windows_sys::Win32::Foundation::{GlobalFree, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::Networking::WinInet::{
        InternetQueryOptionW, InternetSetOptionW, INTERNET_OPTION_PER_CONNECTION_OPTION,
        INTERNET_OPTION_REFRESH, INTERNET_OPTION_SETTINGS_CHANGED,
        INTERNET_PER_CONN_AUTOCONFIG_URL, INTERNET_PER_CONN_FLAGS, INTERNET_PER_CONN_OPTIONW,
        INTERNET_PER_CONN_OPTION_LISTW, INTERNET_PER_CONN_PROXY_BYPASS,
        INTERNET_PER_CONN_PROXY_SERVER, PROXY_TYPE_DIRECT, PROXY_TYPE_PROXY,
    };
    use windows_sys::Win32::System::Registry::{
        RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_NOEXPAND,
        RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };

    const LOCAL_BYPASS: &str = "localhost;127.*;10.*;172.16.*;172.17.*;172.18.*;172.19.*;172.20.*;172.21.*;172.22.*;172.23.*;172.24.*;172.25.*;172.26.*;172.27.*;172.28.*;172.29.*;172.30.*;172.31.*;192.168.*;<local>";
    const LOCAL_NO_PROXY: &str = "localhost,127.0.0.1,::1";
    const ENVIRONMENT: &str = "Environment";
    const HTTP_PROXY: &str = "HTTP_PROXY";
    const HTTPS_PROXY: &str = "HTTPS_PROXY";
    const NO_PROXY: &str = "NO_PROXY";
    const ANNOUNCE_TIMEOUT_MS: u32 = 5000;

    pub fn manual(server: &str) -> ProxySettings {
        let url = format!("http://{server}");
        ProxySettings {
            flags: PROXY_TYPE_DIRECT | PROXY_TYPE_PROXY,
            server: Some(server.to_string()),
            bypass: Some(LOCAL_BYPASS.to_string()),
            pac_url: None,
            http_proxy: Some(url.clone()),
            https_proxy: Some(url),
            no_proxy: Some(LOCAL_NO_PROXY.to_string()),
        }
    }

    fn terminated(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn read_env(name: &str) -> Option<String> {
        let subkey = terminated(ENVIRONMENT);
        let value = terminated(name);
        let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND;
        let mut size = 0u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                value.as_ptr(),
                flags,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let mut buf = vec![0u16; (size as usize).div_ceil(2)];
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                value.as_ptr(),
                flags,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }

    fn write_env(name: &str, value: &Option<String>) -> Result<(), String> {
        let subkey = terminated(ENVIRONMENT);
        let name_w = terminated(name);
        let status = match value {
            Some(v) => {
                let data = terminated(v);
                unsafe {
                    RegSetKeyValueW(
                        HKEY_CURRENT_USER,
                        subkey.as_ptr(),
                        name_w.as_ptr(),
                        REG_SZ,
                        data.as_ptr().cast(),
                        (data.len() * std::mem::size_of::<u16>()) as u32,
                    )
                }
            }
            None => match unsafe {
                RegDeleteKeyValueW(HKEY_CURRENT_USER, subkey.as_ptr(), name_w.as_ptr())
            } {
                ERROR_FILE_NOT_FOUND => ERROR_SUCCESS,
                other => other,
            },
        };
        if status == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(format!(
                "environment variable {name}: {}",
                std::io::Error::from_raw_os_error(status as i32)
            ))
        }
    }

    fn announce_environment() {
        let what = terminated(ENVIRONMENT);
        unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                0,
                what.as_ptr() as isize,
                SMTO_ABORTIFHUNG,
                ANNOUNCE_TIMEOUT_MS,
                std::ptr::null_mut(),
            );
        }
    }

    pub struct InternetOptions;

    fn option(kind: u32) -> INTERNET_PER_CONN_OPTIONW {
        INTERNET_PER_CONN_OPTIONW {
            dwOption: kind,
            ..Default::default()
        }
    }

    fn option_list(options: &mut [INTERNET_PER_CONN_OPTIONW]) -> INTERNET_PER_CONN_OPTION_LISTW {
        INTERNET_PER_CONN_OPTION_LISTW {
            dwSize: std::mem::size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32,
            pszConnection: std::ptr::null_mut(),
            dwOptionCount: options.len() as u32,
            dwOptionError: 0,
            pOptions: options.as_mut_ptr(),
        }
    }

    fn wide(value: &Option<String>) -> Option<Vec<u16>> {
        value
            .as_deref()
            .map(|s| s.encode_utf16().chain(std::iter::once(0)).collect())
    }

    unsafe fn take_string(ptr: *mut u16) -> Option<String> {
        if ptr.is_null() {
            return None;
        }
        let len = (0..).take_while(|&i| *ptr.add(i) != 0).count();
        let value = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
        GlobalFree(ptr.cast());
        Some(value)
    }

    fn last_error(what: &str) -> String {
        format!("{what}: {}", std::io::Error::last_os_error())
    }

    impl ProxyStore for InternetOptions {
        fn read(&self) -> Result<ProxySettings, String> {
            let mut options = [
                option(INTERNET_PER_CONN_FLAGS),
                option(INTERNET_PER_CONN_PROXY_SERVER),
                option(INTERNET_PER_CONN_PROXY_BYPASS),
                option(INTERNET_PER_CONN_AUTOCONFIG_URL),
            ];
            let mut list = option_list(&mut options);
            let mut size = list.dwSize;
            let ok = unsafe {
                InternetQueryOptionW(
                    std::ptr::null(),
                    INTERNET_OPTION_PER_CONNECTION_OPTION,
                    (&mut list as *mut INTERNET_PER_CONN_OPTION_LISTW).cast::<c_void>(),
                    &mut size,
                )
            };
            if ok == 0 {
                return Err(last_error("read system proxy"));
            }
            unsafe {
                Ok(ProxySettings {
                    flags: options[0].Value.dwValue,
                    server: take_string(options[1].Value.pszValue),
                    bypass: take_string(options[2].Value.pszValue),
                    pac_url: take_string(options[3].Value.pszValue),
                    http_proxy: read_env(HTTP_PROXY),
                    https_proxy: read_env(HTTPS_PROXY),
                    no_proxy: read_env(NO_PROXY),
                })
            }
        }

        fn write(&self, settings: &ProxySettings) -> Result<(), String> {
            write_env(HTTP_PROXY, &settings.http_proxy)?;
            write_env(HTTPS_PROXY, &settings.https_proxy)?;
            write_env(NO_PROXY, &settings.no_proxy)?;
            announce_environment();
            let mut server = wide(&settings.server);
            let mut bypass = wide(&settings.bypass);
            let mut pac_url = wide(&settings.pac_url);
            let text = |value: &mut Option<Vec<u16>>| {
                value
                    .as_mut()
                    .map_or(std::ptr::null_mut(), |v| v.as_mut_ptr())
            };
            let mut options = [
                option(INTERNET_PER_CONN_FLAGS),
                option(INTERNET_PER_CONN_PROXY_SERVER),
                option(INTERNET_PER_CONN_PROXY_BYPASS),
                option(INTERNET_PER_CONN_AUTOCONFIG_URL),
            ];
            options[0].Value.dwValue = settings.flags;
            options[1].Value.pszValue = text(&mut server);
            options[2].Value.pszValue = text(&mut bypass);
            options[3].Value.pszValue = text(&mut pac_url);
            let list = option_list(&mut options);
            let ok = unsafe {
                InternetSetOptionW(
                    std::ptr::null(),
                    INTERNET_OPTION_PER_CONNECTION_OPTION,
                    (&list as *const INTERNET_PER_CONN_OPTION_LISTW).cast::<c_void>(),
                    list.dwSize,
                )
            };
            if ok == 0 {
                return Err(last_error("set system proxy"));
            }
            unsafe {
                InternetSetOptionW(
                    std::ptr::null(),
                    INTERNET_OPTION_SETTINGS_CHANGED,
                    std::ptr::null(),
                    0,
                );
                InternetSetOptionW(
                    std::ptr::null(),
                    INTERNET_OPTION_REFRESH,
                    std::ptr::null(),
                    0,
                );
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{apply, restore, ProxySettings, ProxyStore};
    use std::cell::RefCell;

    struct MemoryStore(RefCell<ProxySettings>);

    impl ProxyStore for MemoryStore {
        fn read(&self) -> Result<ProxySettings, String> {
            Ok(self.0.borrow().clone())
        }
        fn write(&self, settings: &ProxySettings) -> Result<(), String> {
            *self.0.borrow_mut() = settings.clone();
            Ok(())
        }
    }

    fn proxy(flags: u32, server: &str) -> ProxySettings {
        ProxySettings {
            flags,
            server: Some(server.to_string()),
            bypass: Some("<local>".to_string()),
            http_proxy: Some(format!("http://{server}")),
            https_proxy: Some(format!("http://{server}")),
            no_proxy: Some("localhost".to_string()),
            ..Default::default()
        }
    }

    fn backup_path(name: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("whisp-sysproxy-{name}-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn restore_brings_back_what_was_there_before() {
        let backup = backup_path("roundtrip");
        let theirs = ProxySettings {
            flags: 5,
            pac_url: Some("http://wpad/proxy.pac".to_string()),
            https_proxy: Some("http://corp:3128".to_string()),
            ..Default::default()
        };
        let store = MemoryStore(RefCell::new(theirs.clone()));
        apply(&store, &backup, &proxy(3, "127.0.0.1:9887")).expect("apply");
        assert_eq!(store.read().unwrap(), proxy(3, "127.0.0.1:9887"));
        restore(&store, &backup).expect("restore");
        assert_eq!(store.read().unwrap(), theirs);
        assert!(!backup.exists());
    }

    #[test]
    fn a_second_apply_keeps_the_original_backup() {
        let backup = backup_path("twice");
        let theirs = proxy(1, "corp:3128");
        let store = MemoryStore(RefCell::new(theirs.clone()));
        apply(&store, &backup, &proxy(3, "127.0.0.1:9887")).expect("first apply");
        apply(&store, &backup, &proxy(3, "127.0.0.1:7890")).expect("second apply");
        restore(&store, &backup).expect("restore");
        assert_eq!(store.read().unwrap(), theirs);
    }

    #[test]
    fn restore_without_a_backup_leaves_the_proxy_alone() {
        let backup = backup_path("none");
        let theirs = proxy(3, "corp:3128");
        let store = MemoryStore(RefCell::new(theirs.clone()));
        restore(&store, &backup).expect("restore");
        assert_eq!(store.read().unwrap(), theirs);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "changes this machine's system proxy for a moment"]
    fn the_windows_proxy_is_set_and_given_back() {
        use super::{manual, InternetOptions};
        let backup = backup_path("windows");
        let store = InternetOptions;
        let before = store.read().expect("read");
        let ours = manual("127.0.0.1:9887");
        apply(&store, &backup, &ours).expect("apply");
        let during = store.read().expect("read while applied");
        restore(&store, &backup).expect("restore");
        let after = store.read().expect("read after restore");
        assert_eq!(during, ours);
        assert_eq!(after, before);
    }

    #[test]
    fn a_backup_from_before_environment_variables_still_restores() {
        let backup = backup_path("older");
        let store = MemoryStore(RefCell::new(proxy(3, "127.0.0.1:9887")));
        std::fs::write(
            &backup,
            r#"{"flags":1,"server":null,"bypass":null,"pac_url":null}"#,
        )
        .expect("write");
        restore(&store, &backup).expect("restore");
        assert_eq!(
            store.read().unwrap(),
            ProxySettings {
                flags: 1,
                ..Default::default()
            }
        );
    }

    #[test]
    fn a_damaged_backup_still_switches_our_proxy_off() {
        let backup = backup_path("damaged");
        let store = MemoryStore(RefCell::new(proxy(3, "127.0.0.1:9887")));
        std::fs::write(&backup, "{not json").expect("write");
        restore(&store, &backup).expect("restore");
        assert_eq!(store.read().unwrap(), ProxySettings::default());
        assert!(!backup.exists());
    }
}
