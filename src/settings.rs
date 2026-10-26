//! What the user chose in Settings, kept in the app's config directory so it
//! holds across restarts. Headless captures read it but never write it.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Hide addresses, accounts and owners. Stays on until turned off.
    pub privacy: bool,
    /// While privacy is on, show device-1, device-2 in place of names.
    pub privacy_names: bool,
    pub show_ips: bool,
    /// The kernel or build next to the OS in the list; always on hover.
    pub show_kernel: bool,
    /// Health warnings whose banner was closed; they stay as a dot on the status pill.
    pub hidden_warnings: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            privacy: false,
            privacy_names: false,
            show_ips: true,
            show_kernel: false,
            hidden_warnings: Vec::new(),
        }
    }
}

/// `~/.config/tailscale-desktop-unofficial` (or under `XDG_CONFIG_HOME`) on
/// Linux, Application Support on macOS, and AppData\Roaming on Windows.
pub fn config_dir() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("tailscale-desktop-unofficial"))
}

fn path() -> Option<PathBuf> {
    Some(config_dir()?.join("settings.json"))
}

fn flag(name: &str) -> Option<bool> {
    std::env::var(name).ok().map(|v| v != "0")
}

impl Settings {
    /// The saved settings, with `TSDU_PRIVACY`, `TSDU_PRIVACY_NAMES`,
    /// `TSDU_SHOW_IPS` and `TSDU_SHOW_KERNEL` on top for tests.
    pub fn load() -> Self {
        let mut s: Settings = path()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        for (name, field) in [
            ("TSDU_PRIVACY", &mut s.privacy),
            ("TSDU_PRIVACY_NAMES", &mut s.privacy_names),
            ("TSDU_SHOW_IPS", &mut s.show_ips),
            ("TSDU_SHOW_KERNEL", &mut s.show_kernel),
        ] {
            if let Some(v) = flag(name) {
                *field = v;
            }
        }
        s
    }

    pub fn save(&self) -> Result<(), String> {
        if std::env::var("TSDU_SHOT").is_ok() {
            return Ok(());
        }
        let p = path().ok_or("no home directory")?;
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let body = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&p, body).map_err(|e| format!("{}: {e}", p.display()))
    }
}
