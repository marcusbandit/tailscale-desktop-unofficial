//! Privacy mode: what the window shows when it may end up on a stream or in a
//! screenshot. Masking happens where text is drawn. The data underneath stays
//! real, so copying a hidden address still copies the address.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};

use crate::ts::{Node, Snapshot};

const HIDDEN: &str = "hidden";

#[derive(Clone, Default)]
pub struct Mask {
    pub on: bool,
    names: bool,
    /// Device id to its number and alias, when names are hidden.
    aliases: HashMap<String, (usize, String)>,
    /// Known personal strings and what text shows instead, longest first:
    /// accounts, MagicDNS names, the home path.
    swaps: Vec<(String, String)>,
    /// `swaps` plus the device names, for text that came from outside the app,
    /// where a name can't be told apart from the app's own words otherwise.
    external_swaps: Vec<(String, String)>,
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
                aliases.insert(n.stable_id.clone(), (i, format!("device-{}", i + 1)));
            }
        }
        let mut m = Self {
            on,
            names,
            aliases,
            ..Default::default()
        };
        if on {
            m.swaps = m.swaps_for(snap, false);
            m.external_swaps = m.swaps_for(snap, true);
        }
        m
    }

    fn swaps_for(&self, snap: Option<&Snapshot>, names: bool) -> Vec<(String, String)> {
        let mut swaps = Vec::new();
        if let Some(home) = dirs::home_dir()
            .map(|h| h.to_string_lossy().into_owned())
            .filter(|h| h.len() > 1)
        {
            swaps.push((home, "~".to_owned()));
        }
        if let Some(s) = snap {
            for n in s.all() {
                swaps.push((n.dns_name.clone(), self.dns(n)));
                swaps.push((n.user_login.clone(), self.who(&n.user_login)));
                swaps.push((n.user_display.clone(), self.who(&n.user_display)));
                if names && self.names {
                    swaps.push((n.name.clone(), self.name(n)));
                    swaps.push((n.hostname.clone(), self.name(n)));
                }
            }
            swaps.push((s.tailnet.clone(), self.who(&s.tailnet)));
            let op = &s.prefs.operator_user;
            swaps.push((op.clone(), self.who(op)));
        }
        swaps.retain(|(from, to)| from.chars().count() >= 2 && from != to);
        swaps.sort_by_key(|(from, _)| std::cmp::Reverse(from.len()));
        swaps.dedup_by(|a, b| a.0 == b.0);
        swaps
    }

    pub fn hides_names(&self) -> bool {
        self.names
    }

    /// Sorts the way the names read: by number while names are hidden, so the
    /// order can't hint at the real names.
    pub fn order(&self, n: &Node) -> String {
        match self.aliases.get(&n.stable_id) {
            Some((i, _)) if self.names => format!("{i:08}"),
            _ => n.name.to_lowercase(),
        }
    }

    pub fn name(&self, n: &Node) -> String {
        if !self.names {
            return n.name.clone();
        }
        self.aliases
            .get(&n.stable_id)
            .map(|(_, a)| a.clone())
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

    /// Text the app put together, with every IP address and every known
    /// account, MagicDNS name and home path in it masked. Device names in it
    /// were masked when the text was made.
    pub fn text(&self, s: &str) -> String {
        if !self.on {
            return s.to_owned();
        }
        scrub_ips(&swap(s, &self.swaps))
    }

    /// Text from outside the app, a health warning or a CLI error: `text`, and
    /// device names too while they are hidden.
    pub fn external(&self, s: &str) -> String {
        if !self.on {
            return s.to_owned();
        }
        scrub_ips(&swap(s, &self.external_swaps))
    }
}

/// Replaces whole words only, in one pass, so a replacement is never matched
/// again and a short name can't eat into a longer word.
fn swap(s: &str, swaps: &[(String, String)]) -> String {
    let word = |c: char| c.is_alphanumeric() || c == '-' || c == '_';
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    'next: while i < s.len() {
        if s[..i].chars().next_back().is_none_or(|c| !word(c)) {
            for (from, to) in swaps {
                if s[i..].starts_with(from.as_str())
                    && s[i + from.len()..].chars().next().is_none_or(|c| !word(c))
                {
                    out.push_str(to);
                    i += from.len();
                    continue 'next;
                }
            }
        }
        let c = s[i..].chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
    }
    out
}

fn scrub_ips(s: &str) -> String {
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
    // Any IPv4 address inside: after a colon, before a port, glued to hex.
    let mut out = String::with_capacity(run.len());
    let mut digits = String::new();
    let flush = |digits: &mut String, out: &mut String| {
        let ip = digits.trim_matches('.');
        if ip.parse::<Ipv4Addr>().is_ok() {
            let lead = digits.len() - digits.trim_start_matches('.').len();
            out.push_str(&digits[..lead]);
            out.push_str(&mask_ip(ip));
            out.push_str(&digits[lead + ip.len()..]);
        } else {
            out.push_str(digits);
        }
        digits.clear();
    };
    for c in run.chars() {
        if c.is_ascii_digit() || c == '.' {
            digits.push(c);
        } else {
            flush(&mut digits, &mut out);
            out.push(c);
        }
    }
    flush(&mut digits, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, name: &str, created: i64) -> Node {
        Node {
            stable_id: id.into(),
            name: name.into(),
            hostname: format!("{name}-host"),
            dns_name: format!("{name}.tailfe12ab.ts.net"),
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
        assert_eq!(m.ip("fd7a:115c:a1e0::5d01:2815"), "fd7a:115c:a1e0::••••");
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
        // Glued to a colon or to hex letters, an address still goes.
        assert_eq!(m.text("addr:100.64.0.1"), "addr:100.••.••.1");
        assert_eq!(m.text("dead:100.1.2.3:41641."), "dead:100.••.••.3:41641.");
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
            m.external(
                "can't send to nemurenai (nemurenai.tailfe12ab.ts.net) as marcus@example.com"
            ),
            "can't send to device-1 (device-1.tail••••.ts.net) as m•••@•••"
        );
        // The app's own text already carries masked names; it keeps its words.
        assert_eq!(m.text("ping nemurenai"), "ping nemurenai");
        // Names stay when only addresses are hidden.
        let m = Mask::new(true, false, Some(&snap));
        assert_eq!(m.external("ping nemurenai"), "ping nemurenai");
    }

    #[test]
    fn swaps_take_whole_words_once() {
        let mut dev = node("a", "dev", 100);
        dev.hostname = "dev".into();
        let mut box_ = node("b", "node", 200);
        box_.hostname = "node".into();
        let snap = Snapshot {
            me: box_,
            peers: vec![dev],
            prefs: crate::ts::Prefs {
                operator_user: "bandit".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let m = Mask::new(true, true, Some(&snap));
        // An alias is never matched again, and a name inside a word stays.
        assert_eq!(m.external("dev and node"), "device-1 and device-2");
        assert_eq!(m.external("device-2 devices"), "device-2 devices");
        assert_eq!(m.text("ping banditbox as bandit"), "ping banditbox as b•••");
        assert_eq!(m.order(&snap.peers[0]), "00000000");
    }
}
