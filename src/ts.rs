//! Talks to the local tailscaled: the LocalAPI over its unix socket for reads and
//! preference changes, and the `tailscale` CLI where the CLI is the simpler route.

use std::io::{Read, Write};
use std::net::IpAddr;
use std::os::unix::net::UnixStream;
use std::process::Command;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};

const SOCKET: &str = "/var/run/tailscale/tailscaled.sock";

// ---------------------------------------------------------------------------
// LocalAPI transport

pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

/// One HTTP/1.0 request over the tailscaled socket. HTTP/1.0 keeps the reply
/// unchunked and closes the connection, so reading to EOF is the whole body.
pub fn local_api(
    method: &str,
    path: &str,
    body: Option<&[u8]>,
    timeout: Duration,
) -> Result<Response, String> {
    let mut sock =
        UnixStream::connect(SOCKET).map_err(|e| format!("tailscaled is not reachable ({e})"))?;
    sock.set_read_timeout(Some(timeout)).ok();
    sock.set_write_timeout(Some(timeout)).ok();
    let body = body.unwrap_or_default();
    let head = format!(
        "{method} {path} HTTP/1.0\r\nHost: local-tailscaled.sock\r\nSec-Tailscale: localapi\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    sock.write_all(head.as_bytes()).map_err(|e| e.to_string())?;
    sock.write_all(body).map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    sock.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("malformed reply from tailscaled")?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Ok(Response {
        status,
        body: raw[split + 4..].to_vec(),
    })
}

fn get_json(path: &str) -> Result<Value, String> {
    let r = local_api("GET", path, None, Duration::from_secs(5))?;
    if r.status != 200 {
        return Err(format!(
            "{path}: HTTP {} {}",
            r.status,
            String::from_utf8_lossy(&r.body).trim()
        ));
    }
    serde_json::from_slice(&r.body).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Snapshot of everything the UI shows

#[derive(Clone, Debug, Default)]
pub struct Node {
    pub stable_id: String,
    pub name: String,
    pub hostname: String,
    pub dns_name: String,
    pub os: String,
    pub ips: Vec<String>,
    pub online: bool,
    pub last_seen: Option<DateTime<Utc>>,
    pub tags: Vec<String>,
    pub user_login: String,
    pub user_display: String,
    pub is_self: bool,
    pub exit_node_active: bool,
    pub exit_node_option: bool,
    pub active: bool,
    pub cur_addr: String,
    pub relay: String,
    pub rx: u64,
    pub tx: u64,
    pub created: Option<DateTime<Utc>>,
    pub key_expiry: Option<DateTime<Utc>>,
    pub taildrop_target: i64,
    pub primary_routes: Vec<String>,
}

impl Node {
    pub fn ipv4(&self) -> &str {
        self.ips
            .iter()
            .find(|ip| !ip.contains(':'))
            .or(self.ips.first())
            .map(String::as_str)
            .unwrap_or("")
    }

    /// Devices that the policy treats the same way: same tags, or same owner when untagged.
    pub fn class(&self) -> String {
        if self.tags.is_empty() {
            format!("user:{}", self.user_login)
        } else {
            self.tags.join(",")
        }
    }

    pub fn taildrop_ok(&self) -> bool {
        self.taildrop_target == 1
    }

    pub fn taildrop_reason(&self) -> &'static str {
        match self.taildrop_target {
            1 => "Ready to receive files",
            2 => "No network map yet",
            3 => "Tailscale is not running",
            4 => "Your account lacks file sharing",
            5 => "Device is offline",
            6 => "No peer info",
            7 => "Device OS does not support Taildrop",
            8 => "Device does not expose Taildrop",
            9 => "Owned by another user or tagged",
            _ => "Unknown",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Prefs {
    #[serde(default)]
    pub route_all: bool,
    #[serde(rename = "ExitNodeID", default)]
    pub exit_node_id: String,
    #[serde(rename = "ExitNodeAllowLANAccess", default)]
    pub exit_node_allow_lan_access: bool,
    #[serde(default)]
    pub want_running: bool,
    #[serde(default)]
    pub shields_up: bool,
    #[serde(default)]
    pub advertise_routes: Option<Vec<String>>,
    #[serde(default)]
    pub operator_user: String,
}

impl Prefs {
    pub fn advertises_exit(&self) -> bool {
        self.advertise_routes
            .as_ref()
            .is_some_and(|r| r.iter().any(|x| x == "0.0.0.0/0" || x == "::/0"))
    }
}

/// One packet filter rule as tailscaled enforces it for traffic coming in to this device.
#[derive(Clone, Debug, Default)]
pub struct FilterRule {
    pub srcs: Vec<String>,
    /// (destination ip or "*", first port, last port)
    pub dsts: Vec<(String, u16, u16)>,
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub version: String,
    pub backend_state: String,
    pub tailnet: String,
    pub health: Vec<String>,
    pub me: Node,
    pub peers: Vec<Node>,
    pub prefs: Prefs,
    pub filter: Option<Vec<FilterRule>>,
    pub waiting_files: Vec<WaitingFile>,
}

impl Snapshot {
    pub fn running(&self) -> bool {
        self.backend_state == "Running"
    }

    pub fn all(&self) -> impl Iterator<Item = &Node> {
        std::iter::once(&self.me).chain(self.peers.iter())
    }

    pub fn node(&self, id: &str) -> Option<&Node> {
        self.all().find(|n| n.stable_id == id)
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct WaitingFile {
    pub name: String,
    pub size: u64,
}

fn parse_time(v: &Value) -> Option<DateTime<Utc>> {
    let t = DateTime::parse_from_rfc3339(v.as_str()?)
        .ok()?
        .with_timezone(&Utc);
    (t.timestamp() > 0).then_some(t)
}

fn strs(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_node(v: &Value, users: &Value, is_self: bool) -> Node {
    let dns_name = v["DNSName"]
        .as_str()
        .unwrap_or("")
        .trim_end_matches('.')
        .to_owned();
    let hostname = v["HostName"].as_str().unwrap_or("").to_owned();
    let name = dns_name
        .split('.')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(&hostname)
        .to_owned();
    let user = &users[v["UserID"].as_u64().unwrap_or(0).to_string()];
    Node {
        stable_id: v["ID"].as_str().unwrap_or("").to_owned(),
        name,
        hostname,
        dns_name,
        os: v["OS"].as_str().unwrap_or("").to_owned(),
        ips: strs(&v["TailscaleIPs"]),
        online: v["Online"].as_bool().unwrap_or(false),
        last_seen: parse_time(&v["LastSeen"]),
        tags: strs(&v["Tags"]),
        user_login: user["LoginName"].as_str().unwrap_or("").to_owned(),
        user_display: user["DisplayName"].as_str().unwrap_or("").to_owned(),
        is_self,
        exit_node_active: v["ExitNode"].as_bool().unwrap_or(false),
        exit_node_option: v["ExitNodeOption"].as_bool().unwrap_or(false),
        active: v["Active"].as_bool().unwrap_or(false),
        cur_addr: v["CurAddr"].as_str().unwrap_or("").to_owned(),
        relay: v["Relay"].as_str().unwrap_or("").to_owned(),
        rx: v["RxBytes"].as_u64().unwrap_or(0),
        tx: v["TxBytes"].as_u64().unwrap_or(0),
        created: parse_time(&v["Created"]),
        key_expiry: parse_time(&v["KeyExpiry"]),
        taildrop_target: v["TaildropTarget"].as_i64().unwrap_or(0),
        primary_routes: strs(&v["PrimaryRoutes"]),
    }
}

pub fn fetch_snapshot() -> Result<Snapshot, String> {
    let st = get_json("/localapi/v0/status")?;
    let prefs: Prefs =
        serde_json::from_value(get_json("/localapi/v0/prefs")?).map_err(|e| e.to_string())?;
    let users = &st["User"];
    let mut peers: Vec<Node> = st["Peer"]
        .as_object()
        .map(|m| m.values().map(|p| parse_node(p, users, false)).collect())
        .unwrap_or_default();
    peers.sort_by(|a, b| a.name.cmp(&b.name));
    let me = if st["Self"].is_object() {
        parse_node(&st["Self"], users, true)
    } else {
        Node::default()
    };
    let waiting_files = get_json("/localapi/v0/files/")
        .ok()
        .and_then(|v| {
            serde_json::from_value::<Option<Vec<WaitingFile>>>(v)
                .ok()
                .flatten()
        })
        .unwrap_or_default();
    Ok(Snapshot {
        version: st["Version"]
            .as_str()
            .unwrap_or("")
            .split('-')
            .next()
            .unwrap_or("")
            .to_owned(),
        backend_state: st["BackendState"].as_str().unwrap_or("").to_owned(),
        tailnet: st["CurrentTailnet"]["Name"]
            .as_str()
            .unwrap_or("")
            .to_owned(),
        health: strs(&st["Health"]),
        me,
        peers,
        prefs,
        filter: None,
        waiting_files,
    })
}

/// The packet filter this device enforces on incoming traffic, read from the netmap.
pub fn fetch_filter() -> Result<Vec<FilterRule>, String> {
    let out = Command::new("tailscale")
        .args(["debug", "netmap"])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
    }
    let v: Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let rules = v["PacketFilterRules"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    Ok(rules
        .iter()
        .map(|r| FilterRule {
            srcs: strs(&r["SrcIPs"]),
            dsts: r["DstPorts"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|d| {
                            (
                                d["IP"].as_str().unwrap_or("").to_owned(),
                                d["Ports"]["First"].as_u64().unwrap_or(0) as u16,
                                d["Ports"]["Last"].as_u64().unwrap_or(65535) as u16,
                            )
                        })
                        .collect()
                })
                .unwrap_or_default(),
        })
        .collect())
}

// ---------------------------------------------------------------------------
// IP matching for filter rules

pub fn ip_matches(pattern: &str, ip: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let Ok(addr) = ip.parse::<IpAddr>() else {
        return false;
    };
    let (net, bits) = match pattern.split_once('/') {
        Some((n, b)) => (n, b.parse::<u32>().ok()),
        None => (pattern, None),
    };
    let Ok(net) = net.parse::<IpAddr>() else {
        return false;
    };
    match (net, addr) {
        (IpAddr::V4(n), IpAddr::V4(a)) => {
            let bits = bits.unwrap_or(32).min(32);
            let mask = if bits == 0 {
                0
            } else {
                u32::MAX << (32 - bits)
            };
            u32::from(n) & mask == u32::from(a) & mask
        }
        (IpAddr::V6(n), IpAddr::V6(a)) => {
            let bits = bits.unwrap_or(128).min(128);
            let mask = if bits == 0 {
                0
            } else {
                u128::MAX << (128 - bits)
            };
            u128::from(n) & mask == u128::from(a) & mask
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Actions

pub fn patch_prefs(body: Value) -> Result<(), String> {
    let data = body.to_string();
    let r = local_api(
        "PATCH",
        "/localapi/v0/prefs",
        Some(data.as_bytes()),
        Duration::from_secs(10),
    )?;
    match r.status {
        200 => Ok(()),
        403 => Err("tailscaled refused: your user is not the Tailscale operator".into()),
        s => Err(format!(
            "HTTP {s}: {}",
            String::from_utf8_lossy(&r.body).trim()
        )),
    }
}

pub fn set_running(on: bool) -> Result<(), String> {
    let prefs = json!({ "WantRunning": on, "WantRunningSet": true });
    let body = prefs.to_string();
    match patch_prefs(prefs) {
        Ok(()) => Ok(()),
        // Not the operator: send the same prefs change as root. `tailscale up`
        // would refuse whenever a non-default pref is set.
        Err(e) if e.contains("operator") => run_as_root(&[
            "curl",
            "-sf",
            "--unix-socket",
            SOCKET,
            "-X",
            "PATCH",
            "-H",
            "Sec-Tailscale: localapi",
            "-d",
            &body,
            "http://local-tailscaled.sock/localapi/v0/prefs",
            "-o",
            "/dev/null",
        ]),
        Err(e) => Err(e),
    }
}

pub fn set_exit_node(stable_id: Option<&str>) -> Result<(), String> {
    patch_prefs(json!({
        "ExitNodeID": stable_id.unwrap_or(""),
        "ExitNodeIDSet": true,
        "ExitNodeIP": "",
        "ExitNodeIPSet": true,
    }))
}

pub fn set_advertise_exit(prefs: &Prefs, on: bool) -> Result<(), String> {
    let mut routes: Vec<String> = prefs
        .advertise_routes
        .clone()
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r != "0.0.0.0/0" && r != "::/0")
        .collect();
    if on {
        routes.push("0.0.0.0/0".into());
        routes.push("::/0".into());
    }
    patch_prefs(json!({ "AdvertiseRoutes": routes, "AdvertiseRoutesSet": true }))
}

pub fn start_daemon() -> Result<(), String> {
    run_as_root(&["systemctl", "start", "tailscaled"])
}

/// Tailscale's own page on IP forwarding, the one its CLI links to.
pub const FORWARDING_URL: &str = "https://tailscale.com/s/ip-forwarding";

/// The sysctl file this app owns, so turning forwarding on can replace it
/// without touching anyone else's settings.
const FORWARDING_FILE: &str = "/etc/sysctl.d/99-tailscale-desktop.conf";

/// Whether a tailscaled health warning is the one about IP forwarding.
pub fn is_forwarding_warning(text: &str) -> bool {
    text.contains("IP forwarding is disabled")
}

/// Turns IP forwarding on now and at every boot, as Tailscale's guide does, so
/// this device can act as an exit node or subnet router.
pub fn enable_ip_forwarding() -> Result<(), String> {
    let script = format!(
        "printf 'net.ipv4.ip_forward = 1\\nnet.ipv6.conf.all.forwarding = 1\\n' > {FORWARDING_FILE} && sysctl -p {FORWARDING_FILE}"
    );
    run_as_root(&["sh", "-c", &script])
}

/// Runs a command as root. pkexec when a polkit agent is around to ask, otherwise
/// a terminal window running sudo so the password or face prompt has somewhere to go.
pub fn run_as_root(cmd: &[&str]) -> Result<(), String> {
    let has_agent = Command::new("pgrep")
        .args([
            "-f",
            "polkit.*agent|polkit-gnome|polkit-kde|hyprpolkitagent|lxpolkit|mate-polkit",
        ])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if has_agent {
        let st = Command::new("pkexec")
            .args(cmd)
            .status()
            .map_err(|e| e.to_string())?;
        return if st.success() {
            Ok(())
        } else {
            Err("pkexec was cancelled or failed".into())
        };
    }
    let script = format!(
        "echo 'Tailscale Desktop needs root to run {1}.'; sudo {0}; s=$?; [ $s -ne 0 ] && {{ echo; read -r -p 'Failed. Press Enter to close.'; }}; exit $s",
        cmd.iter()
            .map(|a| shell_quote(a))
            .collect::<Vec<_>>()
            .join(" "),
        cmd[0].replace('\'', "")
    );
    // How each terminal wants the command handed over.
    let prefix = |bin: &str| -> Vec<String> {
        let base = bin.rsplit('/').next().unwrap_or(bin);
        match base {
            "kitty" => vec!["--class".into(), "tailscale-desktop-sudo".into()],
            "foot" | "wezterm" => vec![],
            "gnome-terminal" => vec!["--wait".into(), "--".into()],
            "ghostty" => vec!["-e".into()],
            _ => vec!["-e".into()],
        }
    };
    let candidates: Vec<(String, Vec<String>)> = std::env::var("TERMINAL")
        .ok()
        .into_iter()
        .chain(
            [
                "kitty",
                "foot",
                "alacritty",
                "gnome-terminal",
                "konsole",
                "xterm",
            ]
            .map(String::from),
        )
        .map(|t| {
            let p = prefix(&t);
            (t, p)
        })
        .collect();
    for (bin, pre) in candidates {
        let mut c = Command::new(&bin);
        c.args(&pre).args(["sh", "-c", &script]);
        if let Ok(st) = c.status() {
            return if st.success() {
                Ok(())
            } else {
                Err(format!("{} as root failed or was cancelled", cmd.join(" ")))
            };
        }
    }
    Err("No polkit agent and no terminal found to ask for the sudo password".into())
}

fn shell_quote(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_./:=".contains(c))
    {
        s.to_owned()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

#[derive(Clone, Debug)]
pub struct Pong {
    pub latency_ms: f64,
    pub via: String,
    pub direct: bool,
}

/// Disco ping: shows the path the tunnel takes. ICMP ping: goes through the
/// peer's packet filter, so it also answers "may I reach this device".
pub fn ping(ip: &str, icmp: bool) -> Result<Pong, String> {
    let kind = if icmp { "ICMP" } else { "disco" };
    let r = local_api(
        "POST",
        &format!("/localapi/v0/ping?ip={ip}&type={kind}"),
        None,
        Duration::from_secs(6),
    )?;
    let v: Value = serde_json::from_slice(&r.body).map_err(|_| format!("HTTP {}", r.status))?;
    if let Some(err) = v["Err"].as_str().filter(|e| !e.is_empty()) {
        return Err(err.to_owned());
    }
    if r.status != 200 {
        return Err(format!("HTTP {}", r.status));
    }
    let endpoint = v["Endpoint"].as_str().unwrap_or("");
    let derp = v["DERPRegionCode"].as_str().unwrap_or("");
    let relay = v["PeerRelay"].as_str().unwrap_or("");
    let (via, direct) = if !endpoint.is_empty() {
        (endpoint.to_owned(), true)
    } else if !relay.is_empty() {
        (format!("peer relay {relay}"), false)
    } else if !derp.is_empty() {
        (format!("DERP relay {derp}"), false)
    } else {
        (
            if icmp {
                "tunnel".into()
            } else {
                "unknown".into()
            },
            false,
        )
    };
    Ok(Pong {
        latency_ms: v["LatencySeconds"].as_f64().unwrap_or(0.0) * 1000.0,
        via,
        direct,
    })
}

pub fn send_file(path: &std::path::Path, target_ip: &str) -> Result<(), String> {
    let out = Command::new("tailscale")
        .args(["file", "cp"])
        .arg(path)
        .arg(format!("{target_ip}:"))
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

pub fn downloads_dir() -> std::path::PathBuf {
    let home = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()));
    Command::new("xdg-user-dir")
        .arg("DOWNLOAD")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .filter(|s| !s.is_empty() && *s != home.to_string_lossy())
        .map(Into::into)
        .unwrap_or_else(|| home.join("Downloads"))
}

/// Moves every waiting Taildrop file into the downloads folder.
pub fn receive_files() -> Result<std::path::PathBuf, String> {
    let dir = downloads_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let out = Command::new("tailscale")
        .args(["file", "get", "--conflict=rename"])
        .arg(&dir)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(dir)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

pub fn delete_waiting_file(name: &str) -> Result<(), String> {
    let path = format!("/localapi/v0/files/{}", url_escape(name));
    let r = local_api("DELETE", &path, None, Duration::from_secs(5))?;
    if r.status / 100 == 2 {
        Ok(())
    } else {
        Err(format!("HTTP {}", r.status))
    }
}

fn url_escape(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cidr_matching() {
        assert!(ip_matches("*", "100.1.2.3"));
        assert!(ip_matches("100.64.0.0/10", "100.94.40.21"));
        assert!(!ip_matches("100.64.0.0/10", "192.168.1.1"));
        assert!(ip_matches("100.94.40.21", "100.94.40.21"));
        assert!(ip_matches(
            "fd7a:115c:a1e0::/48",
            "fd7a:115c:a1e0::6d01:2897"
        ));
        assert!(!ip_matches("100.94.40.21", "fd7a:115c:a1e0::6d01:2897"));
    }

    #[test]
    fn shell_quoting_round_trips() {
        let body = r#"{"WantRunning":true}"#;
        let q = shell_quote(body);
        let out = Command::new("sh")
            .args(["-c", &format!("printf %s {q}")])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), body);
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn url_escaping() {
        assert_eq!(url_escape("a b/c.txt"), "a%20b%2Fc.txt");
    }
}
