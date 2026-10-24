//! Who may open connections to whom.
//!
//! A device only learns the packet filter for its own inbound traffic, so the
//! picture is assembled from three sources, weakest last:
//!
//! - exact: the filter on this device says who may reach it
//! - probed: an ICMP ping went out to a peer and came back, which only happens
//!   when the peer's filter lets this device in
//! - inferred: policies are written against users and tags, so devices with the
//!   same owner and tags get the same treatment as a device already known

use std::collections::HashMap;

use crate::ts::{FilterRule, Node, Snapshot, ip_matches};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Exact,
    Probed,
    Inferred,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Access {
    Allowed { source: Source, ports: String },
    Denied(Source),
    Unknown,
}

impl Access {
    pub fn allowed(&self) -> Option<bool> {
        match self {
            Access::Allowed { .. } => Some(true),
            Access::Denied(_) => Some(false),
            Access::Unknown => None,
        }
    }

    pub fn source(&self) -> Option<Source> {
        match self {
            Access::Allowed { source, .. } | Access::Denied(source) => Some(*source),
            Access::Unknown => None,
        }
    }

    fn with_source(&self, s: Source) -> Access {
        match self {
            Access::Allowed { ports, .. } => Access::Allowed {
                source: s,
                ports: ports.clone(),
            },
            Access::Denied(_) => Access::Denied(s),
            Access::Unknown => Access::Unknown,
        }
    }
}

/// Probe results: peer stable id to "my ICMP ping came back".
pub type Probes = HashMap<String, bool>;

/// Inbound access to this device from `src`, read straight off the packet filter.
pub fn inbound(filter: &[FilterRule], me: &Node, src: &Node) -> Access {
    let mut ranges: Vec<(u16, u16)> = Vec::new();
    for rule in filter {
        let src_hit = rule
            .srcs
            .iter()
            .any(|p| src.ips.iter().any(|ip| ip_matches(p, ip)));
        if !src_hit {
            continue;
        }
        for (dst, first, last) in &rule.dsts {
            if dst == "*" || me.ips.iter().any(|ip| ip_matches(dst, ip)) {
                ranges.push((*first, *last));
            }
        }
    }
    if ranges.is_empty() {
        return Access::Denied(Source::Exact);
    }
    Access::Allowed {
        source: Source::Exact,
        ports: describe_ports(ranges),
    }
}

fn describe_ports(mut ranges: Vec<(u16, u16)>) -> String {
    ranges.sort();
    ranges.dedup();
    if ranges.iter().any(|&(a, b)| a == 0 && b == 65535) {
        return "all ports".into();
    }
    let parts: Vec<String> = ranges
        .iter()
        .map(|&(a, b)| {
            if a == b {
                a.to_string()
            } else {
                format!("{a}-{b}")
            }
        })
        .collect();
    format!("ports {}", parts.join(", "))
}

pub struct Matrix {
    /// (src stable id, dst stable id) -> access
    pub edges: HashMap<(String, String), Access>,
}

impl Matrix {
    pub fn get(&self, src: &str, dst: &str) -> Access {
        self.edges
            .get(&(src.to_owned(), dst.to_owned()))
            .cloned()
            .unwrap_or(Access::Unknown)
    }
}

pub fn build(snap: &Snapshot, probes: &Probes) -> Matrix {
    let me = &snap.me;
    let my_class = me.class();
    let mut edges = HashMap::new();

    // Exact: X -> me.
    let to_me: HashMap<&str, Access> = snap
        .peers
        .iter()
        .map(|p| {
            let a = match &snap.filter {
                Some(f) => inbound(f, me, p),
                None => Access::Unknown,
            };
            (p.stable_id.as_str(), a)
        })
        .collect();

    // A device of my class reaching me means my class may reach my class.
    let class_to_me = |class: &str| -> Access {
        snap.peers
            .iter()
            .filter(|p| p.class() == class)
            .map(|p| to_me[p.stable_id.as_str()].clone())
            .find(|a| a.allowed().is_some())
            .unwrap_or(Access::Unknown)
    };

    // me -> T: probe if we have one, else infer from how T's class treats me.
    let from_me = |t: &Node| -> Access {
        match probes.get(&t.stable_id) {
            Some(true) => Access::Allowed {
                source: Source::Probed,
                ports: "ICMP answered".into(),
            },
            Some(false) => Access::Denied(Source::Probed),
            None if t.class() == my_class => class_to_me(&my_class).with_source(Source::Inferred),
            None => {
                // Another device of T's class that I did reach or fail to reach.
                snap.peers
                    .iter()
                    .filter(|p| p.class() == t.class())
                    .filter_map(|p| probes.get(&p.stable_id))
                    .next()
                    .map(|&ok| {
                        if ok {
                            Access::Allowed {
                                source: Source::Inferred,
                                ports: String::new(),
                            }
                        } else {
                            Access::Denied(Source::Inferred)
                        }
                    })
                    .unwrap_or(Access::Unknown)
            }
        }
    };

    for p in &snap.peers {
        edges.insert(
            (p.stable_id.clone(), me.stable_id.clone()),
            to_me[p.stable_id.as_str()].clone(),
        );
        edges.insert((me.stable_id.clone(), p.stable_id.clone()), from_me(p));
    }

    for x in &snap.peers {
        for t in &snap.peers {
            if x.stable_id == t.stable_id {
                continue;
            }
            let a = if t.class() == my_class {
                // T is treated like me, so X reaches T the way X reaches me.
                to_me[x.stable_id.as_str()].with_source(Source::Inferred)
            } else if x.class() == my_class {
                // X is treated like me, so X reaches T the way I reach T.
                from_me(t).with_source(Source::Inferred)
            } else {
                Access::Unknown
            };
            edges.insert((x.stable_id.clone(), t.stable_id.clone()), a);
        }
    }

    Matrix { edges }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, ip: &str, tags: &[&str]) -> Node {
        Node {
            stable_id: id.into(),
            name: id.into(),
            ips: vec![ip.into()],
            tags: tags.iter().map(|s| s.to_string()).collect(),
            user_login: if tags.is_empty() {
                "me@x".into()
            } else {
                "tagged-devices".into()
            },
            online: true,
            ..Default::default()
        }
    }

    #[test]
    fn one_way_tagged_device() {
        let me = Node {
            is_self: true,
            ..node("me", "100.0.0.1", &[])
        };
        let laptop = node("laptop", "100.0.0.2", &[]);
        let coach = node("coach", "100.0.0.9", &["tag:untrusted"]);
        let snap = Snapshot {
            me: me.clone(),
            peers: vec![laptop.clone(), coach.clone()],
            filter: Some(vec![FilterRule {
                srcs: vec!["100.0.0.1".into(), "100.0.0.2".into()],
                dsts: vec![("*".into(), 0, 65535)],
            }]),
            ..Default::default()
        };
        let probes = Probes::from([("coach".to_string(), true)]);
        let m = build(&snap, &probes);
        assert_eq!(m.get("laptop", "me").allowed(), Some(true));
        assert_eq!(m.get("coach", "me").allowed(), Some(false));
        assert_eq!(m.get("me", "coach").allowed(), Some(true));
        assert_eq!(m.get("me", "laptop").allowed(), Some(true));
        assert_eq!(m.get("laptop", "coach").allowed(), Some(true));
        assert_eq!(m.get("coach", "laptop").allowed(), Some(false));
        assert_eq!(m.get("coach", "laptop").source(), Some(Source::Inferred));
    }

    #[test]
    fn port_description() {
        assert_eq!(describe_ports(vec![(22, 22), (443, 443)]), "ports 22, 443");
        assert_eq!(describe_ports(vec![(0, 65535), (22, 22)]), "all ports");
    }
}
