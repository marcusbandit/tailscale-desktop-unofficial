//! Operating system detail for every device.
//!
//! tailscaled knows its own Hostinfo in full but only hears "linux" or "android"
//! about peers. The Tailscale API knows more: the device list carries the distro
//! and the client version, and the `node:osVersion` posture attribute carries the
//! kernel on Linux and the OS version everywhere else. With a read-only OAuth
//! client the app asks it. The client secret lives in the desktop keyring.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

const API: &str = "https://api.tailscale.com/api/v2";
/// Where an admin creates the OAuth client this app needs.
pub const CONSOLE_URL: &str = "https://console.tailscale.com/admin/settings/trust-credentials";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OsInfo {
    /// As tailscaled names it: linux, android, windows, macOS, iOS.
    pub os: String,
    /// Distro id or name, such as "arch" or "ubuntu". Linux only.
    pub distro: String,
    pub distro_version: String,
    /// Kernel release on Linux, the OS version elsewhere.
    pub os_version: String,
    /// CPU architecture, only known for this device.
    pub arch: String,
    pub client_version: String,
    pub update_available: bool,
}

impl OsInfo {
    /// The line a person reads first: "Arch Linux", "Ubuntu 24.04", "Android 15",
    /// "Windows 11", "macOS 15.6 Sequoia".
    pub fn title(&self) -> String {
        let v = self.os_version.as_str();
        match self.os.as_str() {
            "linux" => self.distro_title(),
            // pfSense, OPNsense and TrueNAS CORE run on FreeBSD and say so.
            "freebsd" if !self.distro.is_empty() => self.distro_title(),
            "freebsd" => "FreeBSD".into(),
            "openbsd" => "OpenBSD".into(),
            "android" if !v.is_empty() => format!("Android {}", major(v)),
            "windows" if !v.is_empty() => match windows_release(v) {
                Some(r) => format!("Windows {r}"),
                None => format!("Windows {v}"),
            },
            "macOS" if !v.is_empty() => {
                let short = trim_patch_zero(v);
                match macos_name(v) {
                    Some(n) => format!("macOS {short} {n}"),
                    None => format!("macOS {short}"),
                }
            }
            "iOS" | "tvOS" if !v.is_empty() => format!("{} {}", self.os, trim_patch_zero(v)),
            "android" => "Android".into(),
            "windows" => "Windows".into(),
            "" => "Unknown system".into(),
            other => other.to_owned(),
        }
    }

    fn distro_title(&self) -> String {
        let name = distro_name(&self.distro);
        let ver = self.distro_version.trim();
        // Rolling distros report their own name as the version.
        if ver.is_empty() || ver.eq_ignore_ascii_case(&name) {
            name
        } else {
            format!("{name} {ver}")
        }
    }

    /// The finer version under the title, labelled: the kernel on Linux, the
    /// build on Windows.
    pub fn detail_row(&self) -> Option<(&'static str, String)> {
        let v = self.os_version.as_str();
        if v.is_empty() {
            return None;
        }
        match self.os.as_str() {
            "linux" => Some(("Kernel", v.to_owned())),
            "windows" => windows_build(v).map(|b| ("Build", b.to_string())),
            "android" if v.contains('.') => Some(("Version", v.to_owned())),
            _ => None,
        }
    }

    /// `detail_row` as one phrase, for the device list.
    pub fn detail(&self) -> Option<String> {
        self.detail_row()
            .map(|(k, v)| format!("{} {v}", k.to_lowercase()))
    }

    /// Fill gaps from another source without overwriting what is known.
    fn merge(&mut self, other: &OsInfo) {
        let fill = |a: &mut String, b: &String| {
            if a.is_empty() {
                a.clone_from(b);
            }
        };
        fill(&mut self.os, &other.os);
        fill(&mut self.distro, &other.distro);
        fill(&mut self.distro_version, &other.distro_version);
        fill(&mut self.os_version, &other.os_version);
        fill(&mut self.arch, &other.arch);
        fill(&mut self.client_version, &other.client_version);
        self.update_available |= other.update_available;
    }
}

fn major(v: &str) -> &str {
    v.split('.').next().unwrap_or(v)
}

fn trim_patch_zero(v: &str) -> &str {
    v.strip_suffix(".0").unwrap_or(v)
}

fn windows_build(v: &str) -> Option<u32> {
    // "10.0.26100.2033": the third field is the build.
    v.split('.').nth(2)?.parse().ok()
}

/// The Windows a build number belongs to: "11", "10", "8.1", "8" or "7".
pub fn windows_release(v: &str) -> Option<&'static str> {
    Some(match windows_build(v)? {
        22000.. => "11",
        10240.. => "10",
        9600.. => "8.1",
        9200.. => "8",
        7600.. => "7",
        _ => return None,
    })
}

fn macos_name(v: &str) -> Option<&'static str> {
    Some(match major(v).parse::<u32>().ok()? {
        11 => "Big Sur",
        12 => "Monterey",
        13 => "Ventura",
        14 => "Sonoma",
        15 => "Sequoia",
        26 => "Tahoe",
        _ => return None,
    })
}

fn distro_name(id: &str) -> String {
    if id.is_empty() {
        return "Linux".into();
    }
    if let Some(d) = crate::osicon::distro(id) {
        return d.name.to_owned();
    }
    let mut c = id.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// This device's own Hostinfo, which tailscaled has in full.
pub fn local() -> Option<OsInfo> {
    let out = Command::new("tailscale")
        .args(["debug", "hostinfo"])
        .output()
        .ok()?;
    let v: Value = serde_json::from_slice(&out.stdout).ok()?;
    Some(from_hostinfo(&v))
}

fn from_hostinfo(v: &Value) -> OsInfo {
    let s = |k: &str| v[k].as_str().unwrap_or("").to_owned();
    OsInfo {
        os: s("OS"),
        distro: s("Distro"),
        distro_version: s("DistroVersion"),
        os_version: s("OSVersion"),
        arch: s("Machine"),
        client_version: s("IPNVersion"),
        update_available: false,
    }
}

// ---------------------------------------------------------------------------
// Keyring

const KEYRING_ATTRS: [&str; 4] = [
    "app",
    "tailscale-desktop-unofficial",
    "kind",
    "oauth-client",
];

/// The stored secret, `Ok(None)` when there is none, and an error when the
/// keyring could not be asked, such as while it is still locked after login.
pub fn load_secret() -> Result<Option<String>, String> {
    let out = Command::new("secret-tool")
        .arg("lookup")
        .args(KEYRING_ATTRS)
        .output()
        .map_err(|e| format!("Could not run secret-tool: {e}"))?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if !s.is_empty() {
        return Ok(Some(s));
    }
    // No match exits 1 quietly; anything on stderr means the keyring itself failed.
    let err = String::from_utf8_lossy(&out.stderr).trim().to_owned();
    if err.is_empty() {
        Ok(None)
    } else {
        Err(format!("The keyring is not available: {err}"))
    }
}

pub fn store_secret(secret: &str) -> Result<(), String> {
    let mut child = Command::new("secret-tool")
        .args(["store", "--label=Tailscale Desktop API access"])
        .args(KEYRING_ATTRS)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not run secret-tool: {e}. Install libsecret."))?;
    child
        .stdin
        .take()
        .ok_or("secret-tool has no stdin")?
        .write_all(secret.as_bytes())
        .map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "The keyring refused the secret: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

pub fn clear_secret() {
    Command::new("secret-tool")
        .arg("clear")
        .args(KEYRING_ATTRS)
        .status()
        .ok();
}

/// An OAuth client secret is `tskey-client-<client id>-<secret>`.
pub fn client_id(secret: &str) -> Option<&str> {
    let rest = secret.trim().strip_prefix("tskey-client-")?;
    let (id, tail) = rest.split_once('-')?;
    (!id.is_empty() && !tail.is_empty()).then_some(id)
}

// ---------------------------------------------------------------------------
// Tailscale API

pub struct Api {
    secret: String,
    token: Option<(String, Instant)>,
}

impl Api {
    pub fn new(secret: String) -> Self {
        Self {
            secret: secret.trim().to_owned(),
            token: None,
        }
    }

    fn token(&mut self) -> Result<String, String> {
        if let Some((t, at)) = &self.token
            && at.elapsed() < Duration::from_secs(50 * 60)
        {
            return Ok(t.clone());
        }
        let id = client_id(&self.secret)
            .ok_or("That is not an OAuth client secret. It starts with tskey-client-.")?;
        let mut resp = ureq::post(format!("{API}/oauth/token"))
            .config()
            .http_status_as_error(false)
            .build()
            .send_form([
                ("grant_type", "client_credentials"),
                ("client_id", id),
                ("client_secret", self.secret.as_str()),
            ])
            .map_err(|e| format!("Could not reach the Tailscale API: {e}"))?;
        let status = resp.status().as_u16();
        let body: Value = resp.body_mut().read_json().unwrap_or(Value::Null);
        if status != 200 {
            return Err(match status {
                400 | 401 | 403 => "Tailscale rejected the credential. It may have been revoked; connect again with a new one.".into(),
                _ => format!("The Tailscale API answered {status}."),
            });
        }
        let t = body["access_token"]
            .as_str()
            .ok_or("The Tailscale API sent no access token.")?
            .to_owned();
        self.token = Some((t.clone(), Instant::now()));
        Ok(t)
    }

    /// GET with the current token, and one retry with a fresh token on 401.
    fn get(&mut self, path: &str) -> Result<(u16, Value), String> {
        let r = get_with(&self.token()?, path)?;
        if r.0 != 401 {
            return Ok(r);
        }
        self.token = None;
        get_with(&self.token()?, path)
    }

    /// OS detail for every device in the tailnet, keyed by stable node id.
    pub fn fetch(&mut self) -> Result<HashMap<String, OsInfo>, String> {
        let (status, body) = self.get("/tailnet/-/devices?fields=all")?;
        if status != 200 {
            return Err(match status {
                401 | 403 => {
                    "The credential cannot read devices. Make a new one with All - Read.".into()
                }
                _ => format!("Listing devices failed with {status}."),
            });
        }
        let mut out = parse_devices(&body);
        // The kernel or OS version is a posture attribute, one request per
        // device, eight at a time. A device whose request fails keeps the rest.
        let ids: Vec<(String, String)> = body["devices"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|d| {
                Some((
                    d["id"].as_str()?.to_owned(),
                    d["nodeId"].as_str()?.to_owned(),
                ))
            })
            .collect();
        let token = self.token()?;
        for chunk in ids.chunks(8) {
            let versions: Vec<(String, Option<String>)> = std::thread::scope(|sc| {
                let hs: Vec<_> = chunk
                    .iter()
                    .map(|(device, node)| {
                        let token = &token;
                        sc.spawn(move || {
                            let v = get_with(token, &format!("/device/{device}/attributes"))
                                .ok()
                                .filter(|(status, _)| *status == 200)
                                .and_then(|(_, a)| {
                                    a["attributes"]["node:osVersion"]
                                        .as_str()
                                        .map(str::to_owned)
                                });
                            (node.clone(), v)
                        })
                    })
                    .collect();
                hs.into_iter().filter_map(|h| h.join().ok()).collect()
            });
            for (node, v) in versions {
                if let (Some(v), Some(info)) = (v, out.get_mut(&node)) {
                    info.os_version = v;
                }
            }
        }
        Ok(out)
    }
}

fn get_with(token: &str, path: &str) -> Result<(u16, Value), String> {
    let mut resp = ureq::get(format!("{API}{path}"))
        .header("Authorization", format!("Bearer {token}"))
        .config()
        .http_status_as_error(false)
        .build()
        .call()
        .map_err(|e| format!("Could not reach the Tailscale API: {e}"))?;
    let status = resp.status().as_u16();
    let body = resp.body_mut().read_json().unwrap_or(Value::Null);
    Ok((status, body))
}

fn parse_devices(body: &Value) -> HashMap<String, OsInfo> {
    let mut out = HashMap::new();
    for d in body["devices"].as_array().into_iter().flatten() {
        let Some(node) = d["nodeId"].as_str() else {
            continue;
        };
        let s = |v: &Value| v.as_str().unwrap_or("").to_owned();
        let client = s(&d["clientVersion"]);
        out.insert(
            node.to_owned(),
            OsInfo {
                os: s(&d["os"]),
                distro: s(&d["distro"]["name"]),
                distro_version: s(&d["distro"]["version"]),
                os_version: String::new(),
                arch: String::new(),
                // "1.104.1-t1a2b3c4d-g5e6f7a8b9" reads as 1.104.1.
                client_version: client.split('-').next().unwrap_or("").to_owned(),
                update_available: d["updateAvailable"].as_bool().unwrap_or(false),
            },
        );
    }
    out
}

/// What is known about every device: the API's view, with this device's own
/// Hostinfo filling the gaps for itself.
pub fn combine(
    api: &HashMap<String, OsInfo>,
    me: Option<(&str, &OsInfo)>,
) -> HashMap<String, OsInfo> {
    let mut out = api.clone();
    if let Some((id, local)) = me {
        let entry = out.entry(id.to_owned()).or_default();
        let api_side = entry.clone();
        *entry = local.clone();
        entry.merge(&api_side);
    }
    out
}

// ---------------------------------------------------------------------------
// Cache, so versions show at once on the next start and while offline.

fn cache_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("tailscale-desktop-unofficial").join("os.json"))
}

pub fn load_cache() -> HashMap<String, OsInfo> {
    cache_path()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save_cache(map: &HashMap<String, OsInfo>) {
    if let Some(p) = cache_path() {
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        if let Ok(b) = serde_json::to_vec(map) {
            std::fs::write(p, b).ok();
        }
    }
}

pub fn clear_cache() {
    if let Some(p) = cache_path() {
        std::fs::remove_file(p).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(os: &str, distro: &str, dv: &str, v: &str) -> OsInfo {
        OsInfo {
            os: os.into(),
            distro: distro.into(),
            distro_version: dv.into(),
            os_version: v.into(),
            ..Default::default()
        }
    }

    #[test]
    fn titles_read_like_a_person_would_say_them() {
        let arch = info("linux", "arch", "Arch Linux", "7.2.11-arch1-1");
        assert_eq!(arch.title(), "Arch Linux");
        assert_eq!(arch.detail().as_deref(), Some("kernel 7.2.11-arch1-1"));
        assert_eq!(
            info("linux", "ubuntu", "24.04", "6.8.0-45-generic").title(),
            "Ubuntu 24.04"
        );
        assert_eq!(info("linux", "", "", "").title(), "Linux");
        assert_eq!(info("linux", "", "", "").detail(), None);
        assert_eq!(info("android", "", "", "15").title(), "Android 15");
        let win = info("windows", "", "", "10.0.26100.2033");
        assert_eq!(win.title(), "Windows 11");
        assert_eq!(win.detail().as_deref(), Some("build 26100"));
        assert_eq!(win.detail_row(), Some(("Build", "26100".to_owned())));
        assert_eq!(info("windows", "", "", "6.1.7601").title(), "Windows 7");
        assert_eq!(info("windows", "", "", "6.3.9600").title(), "Windows 8.1");
        assert_eq!(
            info("windows", "", "", "10.0.19045.1").title(),
            "Windows 10"
        );
        assert_eq!(
            info("macOS", "", "", "15.6.1").title(),
            "macOS 15.6.1 Sequoia"
        );
        assert_eq!(info("macOS", "", "", "26.0").title(), "macOS 26 Tahoe");
        assert_eq!(info("iOS", "", "", "18.1").title(), "iOS 18.1");
        assert_eq!(info("android", "", "", "").title(), "Android");
    }

    #[test]
    fn client_id_comes_out_of_the_secret() {
        assert_eq!(
            client_id("tskey-client-kAbC123CNTRL-xyz789"),
            Some("kAbC123CNTRL")
        );
        assert_eq!(client_id("  tskey-client-k1-s  "), Some("k1"));
        assert_eq!(client_id("tskey-api-k1-s"), None);
        assert_eq!(client_id("tskey-client-k1"), None);
    }

    #[test]
    fn device_list_parses() {
        let body: Value = serde_json::from_str(
            r#"{"devices":[{"id":"123","nodeId":"nAbcCNTRL","os":"linux",
            "clientVersion":"1.104.1-t1a2b3c4d-g5e6f7a8b9","updateAvailable":true,
            "distro":{"name":"ubuntu","version":"25.04","codeName":"plucky"}},
            {"id":"456","nodeId":"nDefCNTRL","os":"android","clientVersion":"1.98.0"}]}"#,
        )
        .unwrap();
        let m = parse_devices(&body);
        let u = &m["nAbcCNTRL"];
        assert_eq!(u.title(), "Ubuntu 25.04");
        assert_eq!(u.client_version, "1.104.1");
        assert!(u.update_available);
        assert_eq!(m["nDefCNTRL"].title(), "Android");
    }

    #[test]
    fn local_hostinfo_fills_gaps_for_this_device() {
        let local = from_hostinfo(&serde_json::json!({
            "OS": "linux", "OSVersion": "7.2.11-arch1-1", "Distro": "arch",
            "DistroVersion": "Arch Linux", "Machine": "x86_64", "IPNVersion": "1.104.1"
        }));
        let mut api = HashMap::new();
        api.insert(
            "me".to_owned(),
            OsInfo {
                os: "linux".into(),
                update_available: true,
                ..Default::default()
            },
        );
        let all = combine(&api, Some(("me", &local)));
        assert_eq!(all["me"].arch, "x86_64");
        assert!(all["me"].update_available);
        assert_eq!(all["me"].detail().as_deref(), Some("kernel 7.2.11-arch1-1"));
    }
}
