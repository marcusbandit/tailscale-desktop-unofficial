//! The connection to the local tailscaled, which each OS offers differently: a
//! unix socket on Linux, a named pipe on Windows, and on macOS a localhost port
//! guarded by a token from the Tailscale app, or a socket for the open-source
//! daemon. Everything above this speaks the same HTTP over whichever it gets.

use std::io::{Read, Write};
use std::time::Duration;

pub trait Conn: Read + Write + Send {}
impl<T: Read + Write + Send> Conn for T {}

pub struct Link {
    pub conn: Box<dyn Conn>,
    /// An `Authorization` header value the LocalAPI wants on this route.
    pub auth: Option<String>,
}

fn unreachable(e: std::io::Error) -> String {
    if cfg!(target_os = "linux") {
        format!("tailscaled is not reachable ({e})")
    } else if e.kind() == std::io::ErrorKind::NotFound
        || e.kind() == std::io::ErrorKind::ConnectionRefused
    {
        "Tailscale isn't running. Open the Tailscale app and sign in.".into()
    } else {
        format!("Tailscale is not reachable ({e})")
    }
}

#[cfg(target_os = "linux")]
pub const SOCKET: &str = "/var/run/tailscale/tailscaled.sock";

#[cfg(target_os = "linux")]
pub fn connect(timeout: Duration) -> Result<Link, String> {
    let s = std::os::unix::net::UnixStream::connect(SOCKET).map_err(unreachable)?;
    s.set_read_timeout(Some(timeout)).ok();
    s.set_write_timeout(Some(timeout)).ok();
    Ok(Link {
        conn: Box::new(s),
        auth: None,
    })
}

#[cfg(windows)]
const PIPE: &str = r"\\.\pipe\ProtectedPrefix\Administrators\Tailscale\tailscaled";

/// Opens tailscaled's pipe as a file. Identification level lets tailscaled see
/// which user is asking, as the official CLI does; it refuses anonymous callers.
/// Pipes have no read timeout, so a hung tailscaled holds the worker thread.
#[cfg(windows)]
pub fn connect(_timeout: Duration) -> Result<Link, String> {
    use std::os::windows::fs::OpenOptionsExt;
    const SECURITY_SQOS_PRESENT: u32 = 0x0010_0000;
    const SECURITY_IDENTIFICATION: u32 = 0x0001_0000;
    const ERROR_PIPE_BUSY: i32 = 231;
    let mut tries = 0;
    loop {
        let r = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .security_qos_flags(SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION)
            .open(PIPE);
        match r {
            Ok(f) => {
                return Ok(Link {
                    conn: Box::new(f),
                    auth: None,
                });
            }
            // Every pipe instance is serving someone; another frees up quickly.
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && tries < 20 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(unreachable(e)),
        }
    }
}

#[cfg(target_os = "macos")]
const SOCKET: &str = "/var/run/tailscaled.socket";

/// The app's port and token, found once and kept until a connect to them fails,
/// so a poll doesn't run lsof every time.
#[cfg(target_os = "macos")]
static APP_PORT: std::sync::Mutex<Option<(u16, String)>> = std::sync::Mutex::new(None);

/// The Tailscale app serves the LocalAPI on a localhost port and proves the
/// caller is the same user with a token. The open-source tailscaled from
/// Homebrew uses a unix socket instead, so that is the fallback.
#[cfg(target_os = "macos")]
pub fn connect(timeout: Duration) -> Result<Link, String> {
    let cached = APP_PORT.lock().ok().and_then(|g| g.clone());
    let fresh = || {
        let found = app_port_and_token();
        if let Ok(mut g) = APP_PORT.lock() {
            *g = found.clone();
        }
        found
    };
    let mut app_err = None;
    // A cached port can go stale when the app restarts; look again once.
    for found in [cached.clone(), None] {
        let Some((port, token)) = found.or_else(fresh) else {
            break;
        };
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        match std::net::TcpStream::connect_timeout(&addr, timeout) {
            Ok(s) => {
                s.set_read_timeout(Some(timeout)).ok();
                s.set_write_timeout(Some(timeout)).ok();
                return Ok(Link {
                    conn: Box::new(s),
                    auth: Some(format!("Basic {}", base64(format!(":{token}").as_bytes()))),
                });
            }
            Err(e) => {
                if let Ok(mut g) = APP_PORT.lock() {
                    *g = None;
                }
                app_err = Some(e);
                if cached.is_none() {
                    break;
                }
            }
        }
    }
    match std::os::unix::net::UnixStream::connect(SOCKET) {
        Ok(s) => {
            s.set_read_timeout(Some(timeout)).ok();
            s.set_write_timeout(Some(timeout)).ok();
            Ok(Link {
                conn: Box::new(s),
                auth: None,
            })
        }
        Err(e) => Err(unreachable(app_err.unwrap_or(e))),
    }
}

/// The App Store app's sandboxed extension holds open a file named
/// `sameuserproof-<port>-<token>`, which lsof can see. The standalone app
/// names the port in a symlink and keeps the token in a file admins can read.
#[cfg(target_os = "macos")]
fn app_port_and_token() -> Option<(u16, String)> {
    if let Ok(out) = std::process::Command::new("lsof")
        .args(["-n", "-c", "IPNExtension", "-F"])
        .output()
    {
        let text = String::from_utf8_lossy(&out.stdout);
        if let Some(found) = text.lines().find_map(|l| {
            l.split_once(".tailscale.ipn.macos/sameuserproof-")
                .map(|(_, rest)| rest.to_owned())
        }) {
            if let Some(pt) = parse_port_token(&found) {
                return Some(pt);
            }
        }
    }
    let port: u16 = std::fs::read_link("/Library/Tailscale/ipnport")
        .ok()?
        .to_str()?
        .parse()
        .ok()?;
    let token = std::fs::read_to_string(format!("/Library/Tailscale/sameuserproof-{port}")).ok()?;
    let token = token.trim();
    (!token.is_empty()).then(|| (port, token.to_owned()))
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_port_token(s: &str) -> Option<(u16, String)> {
    let (port, token) = s.trim().split_once('-')?;
    let port = port.parse().ok()?;
    (!token.is_empty()).then(|| (port, token.to_owned()))
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The `tailscale` CLI. Windows and macOS installs don't always put it on PATH,
/// so look where the official installers leave it.
pub fn cli() -> std::process::Command {
    #[cfg(target_os = "macos")]
    {
        const APP: &str = "/Applications/Tailscale.app/Contents/MacOS/Tailscale";
        if std::path::Path::new(APP).exists() {
            return std::process::Command::new(APP);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let installed = std::env::var_os("ProgramFiles")
            .map(|p| {
                std::path::PathBuf::from(p)
                    .join("Tailscale")
                    .join("tailscale.exe")
            })
            .filter(|p| p.exists());
        let mut c = std::process::Command::new(installed.unwrap_or_else(|| "tailscale.exe".into()));
        // A GUI app spawning a console program would flash a console window.
        c.creation_flags(CREATE_NO_WINDOW);
        return c;
    }
    #[allow(unreachable_code)]
    std::process::Command::new("tailscale")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_known_values() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b":0a1b2c"), "OjBhMWIyYw==");
    }

    #[test]
    fn port_and_token_from_proof_name() {
        assert_eq!(
            parse_port_token("62345-0a1b2c3d4e\n"),
            Some((62345, "0a1b2c3d4e".to_owned()))
        );
        assert_eq!(parse_port_token("nope"), None);
        assert_eq!(parse_port_token("123-"), None);
    }
}
