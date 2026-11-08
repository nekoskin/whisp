use std::net::{IpAddr, SocketAddr};
#[cfg(target_os = "macos")]
use std::path::Path;
#[cfg(target_os = "macos")]
use std::process::Command;

const NAMESERVER_KEY: &str = "nameserver:";
const LIST_ITEM: &str = "- ";
const SCHEME_SEPARATOR: &str = "://";
const URL_TAIL_STARTS: [char; 3] = ['/', '#', '?'];
const SERVICE_LIST_HEADER_LINES: usize = 1;
const DISABLED_MARK: char = '*';
const FIELD_SEPARATOR: char = '\t';
const SERVER_SEPARATOR: &str = " ";
#[cfg(target_os = "macos")]
const NO_SERVERS: &str = "Empty";
#[cfg(target_os = "macos")]
const NETWORKSETUP: &str = "networksetup";

pub(crate) type Snapshot = Vec<(String, Vec<String>)>;

pub(crate) fn tunnelled_servers(config: &str) -> Vec<IpAddr> {
    config
        .lines()
        .map(str::trim)
        .skip_while(|line| *line != NAMESERVER_KEY)
        .skip(1)
        .map_while(|line| line.strip_prefix(LIST_ITEM))
        .filter_map(nameserver_ip)
        .collect()
}

fn nameserver_ip(item: &str) -> Option<IpAddr> {
    let item = item.trim().trim_matches('"');
    let addr = item
        .split_once(SCHEME_SEPARATOR)
        .map_or(item, |(_, rest)| rest);
    let addr = addr.split(URL_TAIL_STARTS).next().unwrap_or(addr);
    addr.parse::<IpAddr>()
        .ok()
        .or_else(|| addr.parse::<SocketAddr>().ok().map(|addr| addr.ip()))
}

pub(crate) fn services(listing: &str) -> Vec<String> {
    listing
        .lines()
        .skip(SERVICE_LIST_HEADER_LINES)
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with(DISABLED_MARK))
        .map(String::from)
        .collect()
}

pub(crate) fn servers_of(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| line.parse::<IpAddr>().is_ok())
        .map(String::from)
        .collect()
}

fn encode(snapshot: &Snapshot) -> String {
    snapshot
        .iter()
        .map(|(service, servers)| {
            format!(
                "{service}{FIELD_SEPARATOR}{}\n",
                servers.join(SERVER_SEPARATOR)
            )
        })
        .collect()
}

fn decode(text: &str) -> Snapshot {
    text.lines()
        .filter_map(|line| {
            let (service, servers) = line.split_once(FIELD_SEPARATOR)?;
            let servers = servers
                .split(SERVER_SEPARATOR)
                .filter(|server| !server.is_empty())
                .map(String::from)
                .collect();
            Some((service.to_string(), servers))
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn networksetup(args: &[&str]) -> Result<String, String> {
    let out = Command::new(NETWORKSETUP)
        .args(args)
        .output()
        .map_err(|e| format!("{NETWORKSETUP}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!(
            "{NETWORKSETUP} {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn snapshot() -> Result<Snapshot, String> {
    services(&networksetup(&["-listallnetworkservices"])?)
        .into_iter()
        .map(|service| {
            let servers = servers_of(&networksetup(&["-getdnsservers", &service])?);
            Ok((service, servers))
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn set_servers(service: &str, servers: &[String]) -> Result<(), String> {
    let mut args = vec!["-setdnsservers", service];
    if servers.is_empty() {
        args.push(NO_SERVERS);
    } else {
        args.extend(servers.iter().map(String::as_str));
    }
    networksetup(&args).map(|_| ())
}

#[cfg(target_os = "macos")]
fn flush_cache() {
    let _ = Command::new("dscacheutil").arg("-flushcache").status();
    let _ = Command::new("killall")
        .args(["-HUP", "mDNSResponder"])
        .status();
}

#[cfg(target_os = "macos")]
pub(crate) fn point_at_tunnel(config: &Path, state: &Path) -> Result<(), String> {
    if state.exists() {
        return Ok(());
    }
    let config =
        std::fs::read_to_string(config).map_err(|e| format!("{}: {e}", config.display()))?;
    let servers: Vec<String> = tunnelled_servers(&config)
        .iter()
        .map(IpAddr::to_string)
        .collect();
    if servers.is_empty() {
        return Ok(());
    }
    let before = snapshot()?;
    if let Some(dir) = state.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(state, encode(&before)).map_err(|e| format!("{}: {e}", state.display()))?;
    for (service, _) in &before {
        set_servers(service, &servers)?;
    }
    flush_cache();
    Ok(())
}

#[cfg(target_os = "macos")]
pub(crate) fn restore(state: &Path) -> Result<(), String> {
    let Ok(text) = std::fs::read_to_string(state) else {
        return Ok(());
    };
    for (service, servers) in decode(&text) {
        set_servers(&service, &servers)?;
    }
    std::fs::remove_file(state).map_err(|e| format!("{}: {e}", state.display()))?;
    flush_cache();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mihomo::{generate_config, MihomoConfig};

    #[test]
    fn tunnel_servers_come_from_the_generated_config() {
        let custom = [
            "77.88.8.8".to_string(),
            "8.8.4.4:53".to_string(),
            "https://dns.example/dns-query".to_string(),
        ];
        let config = generate_config(&config_with(&custom, false, ""));
        let expected: Vec<IpAddr> = vec!["77.88.8.8".parse().unwrap(), "8.8.4.4".parse().unwrap()];
        assert_eq!(tunnelled_servers(&config), expected, "{config}");
    }

    #[test]
    fn redirected_dns_points_the_system_at_the_tunnelled_resolver() {
        for (vpn_dns, expected) in [
            ("", "1.1.1.1"),
            ("8.8.8.8", "8.8.8.8"),
            ("77.88.8.8:53", "77.88.8.8"),
            ("https://9.9.9.9/dns-query", "9.9.9.9"),
        ] {
            let config = generate_config(&config_with(&[], true, vpn_dns));
            let expected: Vec<IpAddr> = vec![expected.parse().unwrap()];
            assert_eq!(tunnelled_servers(&config), expected, "{config}");
        }
    }

    fn config_with<'a>(
        custom_dns: &'a [String],
        dns_redirect: bool,
        vpn_dns: &'a str,
    ) -> MihomoConfig<'a> {
        MihomoConfig {
            socks_addr: "127.0.0.1:1081",
            server_host: "example.com",
            mixed_port: 7890,
            tun: true,
            tun_stack: "mixed",
            dns_redirect,
            ipv6: false,
            routing_rules: &[],
            extra_socks_addrs: &[],
            custom_dns,
            socks_user: "",
            socks_pass: "",
            allow_lan: false,
            log_level: "info",
            routing_mode: "rule",
            bypass_ru: true,
            external_link: "",
            secret: "",
            kill_switch: false,
            vpn_dns,
        }
    }

    #[test]
    fn only_enabled_services_are_listed() {
        let listing = "An asterisk (*) denotes that a network service is disabled.\n\
USB 10/100/1000 LAN\n\
Wi-Fi\n\
*Thunderbolt Bridge\n\n";
        assert_eq!(services(listing), ["USB 10/100/1000 LAN", "Wi-Fi"]);
    }

    #[test]
    fn dhcp_dns_reads_as_no_servers() {
        assert!(servers_of("There aren't any DNS Servers set on Wi-Fi.\n").is_empty());
        assert_eq!(
            servers_of("192.168.1.1\n2001:db8::1\n"),
            ["192.168.1.1", "2001:db8::1"]
        );
    }

    #[test]
    fn saved_state_survives_a_round_trip() {
        let saved: Snapshot = vec![
            ("USB 10/100/1000 LAN".to_string(), vec![]),
            (
                "Wi-Fi".to_string(),
                vec!["192.168.1.1".to_string(), "1.1.1.1".to_string()],
            ),
        ];
        assert_eq!(decode(&encode(&saved)), saved);
    }
}
