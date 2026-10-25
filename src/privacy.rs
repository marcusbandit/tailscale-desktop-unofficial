//! Privacy mode: what the window shows when it may end up on a stream or in a
//! screenshot. Masking happens where text is drawn. The data underneath stays
//! real, so copying a hidden address still copies the address.

use std::collections::HashMap;
use std::net::IpAddr;

use crate::ts::{Node, Snapshot};

const HIDDEN: &str = "hidden";

#[derive(Clone, Default)]
pub struct Mask {
    pub on: bool,
    names: bool,
    aliases: HashMap<String, String>,
    /// Known personal strings and what free text shows instead, longest first,
    /// so an error from the CLI can't spell out what the rest of the window hides.
    swaps: Vec<(String, String)>,
}

impl Mask {
    pub fn new(on: bool, names: bool, snap: Option<&Snapshot>) -> Self {
        let names = on && names;
        let mut aliases = HashMap::new();
        if let Some(s) = snap.filter(|_| names) {
            // Numbered in the order the devices were added, so a device keeps its
            // number while others come and go online.
            let mut nodes: Vec<&Node> = s.all().collect();
            nodes.sort_by(|a, b| (a.created, &a.stable_id).cmp(&(b.created, &b.stable_id)));
            for (i, n) in nodes.iter().enumerate() {
                aliases.insert(n.stable_id.clone(), format!("device-{}", i + 1));
            }
        }
        let mut m = Self {
            on,
            names,
            aliases,
            swaps: Vec::new(),
        };
        if on {
            m.swaps = m.swaps_for(snap);
        }
        m
    }

    fn swaps_for(&self, snap: Option<&Snapshot>) -> Vec<(String, String)> {
        let mut swaps = Vec::new();
        if let Some(home) = std::env::var("HOME").ok().filter(|h| h.len() > 1) {
            swaps.push((home, "~".to_owned()));
        }
        if let Some(s) = snap {
            for n in s.all() {
                swaps.push((n.dns_name.clone(), self.dns(n)));
                swaps.push((n.user_login.clone(), self.who(&n.user_login)));
                swaps.push((n.user_display.clone(), self.who(&n.user_display)));
                if self.names {
                    swaps.push((n.name.clone(), self.name(n)));
                    swaps.push((n.hostname.clone(), self.name(n)));
                }
            }
            swaps.push((s.tailnet.clone(), self.who(&s.tailnet)));
            let op = &s.prefs.operator_user;
            swaps.push((op.clone(), self.who(op)));
        }
        // Short strings would eat into ordinary words.
        swaps.retain(|(from, to)| from.chars().count() >= 3 && from != to);
        swaps.sort_by_key(|(from, _)| std::cmp::Reverse(from.len()));
        swaps.dedup_by(|a, b| a.0 == b.0);
        swaps
    }

    pub fn name(&self, n: &Node) -> String {
        if !self.names {
            return n.name.clone();
        }
        self.aliases
            .get(&n.stable_id)
            .cloned()
            .unwrap_or_else(|| "device".into())
    }

    /// A machine's own hostname reads like its name, so it hides with the names.
    pub fn hostname(&self, n: &Node) -> String {
        if self.names {
            self.name(n)
        } else {
            n.hostname.clone()
        }
    }

    /// `100.94.40.21` becomes `100.••.••.21`. IPv6 keeps Tailscale's shared
    /// prefix and hides the part that belongs to the device.
    pub fn ip(&self, ip: &str) -> String {
        if !self.on {
            return ip.to_owned();
        }
        mask_ip(ip)
    }

    /// The MagicDNS name without the tailnet part, and without the device's
    /// name when names are hidden too.
    pub fn dns(&self, n: &Node) -> String {
        if !self.on {
            return n.dns_name.clone();
        }
        let mut parts: Vec<String> = n.dns_name.split('.').map(str::to_owned).collect();
        if parts.len() >= 3 {
            if self.names {
                parts[0] = self.name(n);
            }
            let i = parts.len() - 3;
            parts[i] = if parts[i].starts_with("tail") {
                "tail••••".into()
            } else {
                "••••".into()
            };
        }
        parts.join(".")
    }

    /// An account, a person's name or a local user: the first letter stays so
    /// two of them can still be told apart.
    pub fn who(&self, s: &str) -> String {
        if !self.on || s.is_empty() {
            return s.to_owned();
        }
        let first: String = s.chars().take(1).collect();
        if s.contains('@') {
            format!("{first}•••@•••")
        } else {
            format!("{first}•••")
        }
    }

    /// Something with nothing worth keeping, such as a public endpoint.
    pub fn secret(&self, s: &str) -> String {
        if !self.on || s.is_empty() {
            s.to_owned()
        } else {
            HIDDEN.into()
        }
    }

    /// Free text from elsewhere, a health warning or an error, with every IP
    /// address and every known name, account and home path in it masked.
    pub fn text(&self, s: &str) -> String {
        if !self.on {
            return s.to_owned();
        }
        let mut s = s.to_owned();
        for (from, to) in &self.swaps {
            if s.contains(from.as_str()) {
                s = s.replace(from.as_str(), to);
            }
        }
        let mut out = String::with_capacity(s.len());
        let mut run = String::new();
        let flush = |run: &mut String, out: &mut String| {
            out.push_str(&mask_run(run));
            run.clear();
        };
        for c in s.chars() {
            if c.is_ascii_hexdigit() || c == '.' || c == ':' {
                run.push(c);
            } else {
                flush(&mut run, &mut out);
                out.push(c);
            }
        }
        flush(&mut run, &mut out);
        out
    }
}

fn mask_ip(ip: &str) -> String {
    if let Some((a, rest)) = ip.split_once('.')
        && let Some((_, d)) = rest.rsplit_once('.')
    {
        return format!("{a}.••.••.{d}");
    }
    let groups: Vec<&str> = ip.split(':').collect();
    if groups.len() > 3 {
        return format!("{}::••••", groups[..3].join(":"));
    }
    "••••".into()
}

/// One stretch of hex digits, dots and colons: an address, an address with a
/// port, or ordinary text that only looks a bit like one.
fn mask_run(run: &str) -> String {
    let trimmed = run.trim_end_matches(['.', ':']);
    let tail = &run[trimmed.len()..];
    if trimmed.parse::<IpAddr>().is_ok() {
        return format!("{}{tail}", mask_ip(trimmed));
    }
    if let Some((host, port)) = trimmed.rsplit_once(':')
        && host.parse::<std::net::Ipv4Addr>().is_ok()
        && port.chars().all(|c| c.is_ascii_digit())
    {
        return format!("{}:{port}{tail}", mask_ip(host));
    }
    run.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, name: &str, created: i64) -> Node {
        Node {
            stable_id: id.into(),
            name: name.into(),
            hostname: format!("{name}-host"),
            dns_name: format!("{name}.tail24c6a9.ts.net"),
            created: chrono::DateTime::from_timestamp(created, 0),
            ..Default::default()
        }
    }

    #[test]
    fn off_changes_nothing() {
        let m = Mask::new(false, true, None);
        assert_eq!(m.ip("100.94.40.21"), "100.94.40.21");
        assert_eq!(m.who("a@b.com"), "a@b.com");
        assert_eq!(m.text("from 100.1.2.3"), "from 100.1.2.3");
    }

    #[test]
    fn addresses_keep_their_shape() {
        let m = Mask::new(true, false, None);
        assert_eq!(m.ip("100.94.40.21"), "100.••.••.21");
        assert_eq!(
            m.ip("fd7a:115c:a1e0::5d01:2815"),
            "fd7a:115c:a1e0::••••"
        );
        assert_eq!(m.who("marcus@example.com"), "m•••@•••");
        assert_eq!(m.who("bandit"), "b•••");
        assert_eq!(m.secret("84.2.3.4:41641"), "hidden");
    }

    #[test]
    fn free_text_loses_its_addresses() {
        let m = Mask::new(true, false, None);
        assert_eq!(
            m.text("dial 84.238.12.3:41641 failed."),
            "dial 84.••.••.3:41641 failed."
        );
        assert_eq!(
            m.text("via [2a02:aa1:1022::1]:41641"),
            "via [2a02:aa1:1022::••••]:41641"
        );
        // Versions, words and times stay as they were.
        assert_eq!(
            m.text("tailscaled 1.104.1 added at 12:30, IP forwarding is disabled."),
            "tailscaled 1.104.1 added at 12:30, IP forwarding is disabled."
        );
    }

    #[test]
    fn names_follow_the_order_devices_were_added() {
        let snap = Snapshot {
            me: node("b", "banditbox", 200),
            peers: vec![node("a", "nemurenai", 100), node("c", "diablo-pi", 300)],
            ..Default::default()
        };
        let m = Mask::new(true, true, Some(&snap));
        assert_eq!(m.name(&snap.peers[0]), "device-1");
        assert_eq!(m.name(&snap.me), "device-2");
        assert_eq!(m.hostname(&snap.peers[1]), "device-3");
        assert_eq!(m.dns(&snap.me), "device-2.tail••••.ts.net");
        // Names only hide while privacy mode is on.
        let off = Mask::new(false, true, Some(&snap));
        assert_eq!(off.name(&snap.me), "banditbox");
        let addresses_only = Mask::new(true, false, Some(&snap));
        assert_eq!(addresses_only.dns(&snap.me), "banditbox.tail••••.ts.net");
    }

    #[test]
    fn free_text_loses_known_names_and_accounts() {
        let mut me = node("b", "banditbox", 200);
        me.user_login = "marcus@example.com".into();
        let snap = Snapshot {
            tailnet: "marcus@example.com".into(),
            me,
            peers: vec![node("a", "nemurenai", 100)],
            ..Default::default()
        };
        let m = Mask::new(true, true, Some(&snap));
        assert_eq!(
            m.text("can't send to nemurenai (nemurenai.tail24c6a9.ts.net) as marcus@example.com"),
            "can't send to device-1 (device-1.tail••••.ts.net) as m•••@•••"
        );
        // Names stay when only addresses are hidden.
        let m = Mask::new(true, false, Some(&snap));
        assert_eq!(m.text("ping nemurenai"), "ping nemurenai");
    }
}
