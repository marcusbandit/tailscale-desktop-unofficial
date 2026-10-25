//! The icon a device's badge shows and what hovering it says: the distro's
//! logo, the mark of its Windows, Apple's mark, or for phones, tablets and TVs
//! the kind of device its name suggests. The glyphs come from a subset of the
//! Nerd Fonts symbols that ships with the app, `assets/os-icons.ttf`, so they
//! show whatever fonts the system has.

use crate::osinfo::{self, OsInfo};

pub struct Icon {
    pub glyph: &'static str,
    /// "Ubuntu 26.04 on a Raspberry Pi", "Google Pixel, Android 17".
    pub label: String,
}

mod g {
    pub const TUX: &str = "\u{f31a}";
    pub const SERVER: &str = "\u{f233}";
    pub const APPLE: &str = "\u{f179}";
    pub const ANDROID: &str = "\u{f17b}";
    pub const GOOGLE: &str = "\u{f1a0}";
    pub const PHONE: &str = "\u{f10b}";
    pub const TABLET: &str = "\u{f10a}";
    pub const TV: &str = "\u{f0502}";
    pub const WATCH: &str = "\u{f0589}";
    pub const NAS: &str = "\u{f08f3}";
    pub const WINDOWS_11: &str = "\u{f05b3}";
    pub const WINDOWS_10: &str = "\u{f17a}";
    pub const WINDOWS_7: &str = "\u{f0a21}";
    pub const FREEBSD: &str = "\u{f30c}";
    pub const OPENBSD: &str = "\u{f328}";
}

/// The logo for an os-release `ID`, when the font has one. Derivatives without
/// their own logo show their base, since that is the family they belong to.
fn distro(id: &str) -> Option<&'static str> {
    Some(match id {
        "arch" | "archarm" => "\u{f303}",
        "ubuntu" => "\u{f31b}",
        "kubuntu" => "\u{f333}",
        "debian" => "\u{f306}",
        "raspbian" => "\u{f315}",
        "devuan" => "\u{f307}",
        "fedora" | "bazzite" | "ultramarine" => "\u{f30a}",
        "nobara" => "\u{f380}",
        "nixos" => "\u{f313}",
        "opensuse-tumbleweed" => "\u{f37d}",
        "opensuse-leap" => "\u{f37e}",
        "opensuse" | "opensuse-microos" | "opensuse-slowroll" | "sles" | "sled" => "\u{f314}",
        "alpine" => "\u{f300}",
        "centos" => "\u{f304}",
        "rhel" => "\u{f316}",
        "rocky" => "\u{f32b}",
        "almalinux" => "\u{f31d}",
        "gentoo" => "\u{f30d}",
        "void" => "\u{f32e}",
        "manjaro" | "manjaro-arm" => "\u{f312}",
        "endeavouros" => "\u{f322}",
        "cachyos" => "\u{f385}",
        "garuda" => "\u{f337}",
        "artix" => "\u{f31f}",
        "archcraft" => "\u{f345}",
        "arcolinux" => "\u{f346}",
        "pop" => "\u{f32a}",
        "linuxmint" => "\u{f30e}",
        "elementary" => "\u{f309}",
        "zorin" => "\u{f32f}",
        "neon" => "\u{f331}",
        "kali" => "\u{f327}",
        "parrot" => "\u{f329}",
        "mx" => "\u{f33f}",
        "deepin" => "\u{f321}",
        "solus" => "\u{f32d}",
        "slackware" => "\u{f318}",
        "mageia" => "\u{f310}",
        "qubes" => "\u{f342}",
        "tails" => "\u{f343}",
        "trisquel" => "\u{f344}",
        "puppy" => "\u{f341}",
        "aosc" => "\u{f301}",
        "coreos" | "fedora-coreos" => "\u{f305}",
        "vanilla" => "\u{f366}",
        "postmarketos" => "\u{f374}",
        "openwrt" => "\u{f382}",
        "steamos" => "\u{f1b6}",
        "haos" => "\u{f07d0}",
        "synology" | "qnap" | "unraid" | "truenas" => g::NAS,
        _ => return None,
    })
}

/// The words of a device's name, lowercased, split on anything that isn't a
/// letter or digit: "SM-S918B" gives "sm" and "s918b".
fn words(names: &[&str]) -> Vec<String> {
    names
        .iter()
        .flat_map(|n| n.split(|c: char| !c.is_alphanumeric()))
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// What kind of Android device a name suggests. The API sends no model for
/// other devices, but phones and TVs keep their model as the default name.
fn android(w: &[String]) -> Option<(&'static str, &'static str)> {
    let has = |x: &str| w.iter().any(|v| v == x);
    let starts = |x: &str| w.iter().any(|v| v.starts_with(x));
    let google = has("pixel") || has("google");
    let samsung = has("galaxy") || has("samsung") || has("sm");
    Some(if has("shield") {
        (g::TV, "NVIDIA Shield")
    } else if starts("chromecast") {
        (g::TV, "Chromecast with Google TV")
    } else if starts("aft") || has("firetv") || (has("fire") && has("tv")) {
        (g::TV, "Amazon Fire TV")
    } else if has("tv") || has("bravia") || has("mibox") || has("androidtv") || has("googletv") {
        (g::TV, "Android TV")
    } else if has("watch") || has("wear") {
        (g::WATCH, "Wear OS watch")
    } else if has("tablet") || has("tab") || has("pad") {
        let kind = if google {
            "Google Pixel Tablet"
        } else if samsung {
            "Samsung Galaxy Tab"
        } else {
            "Android tablet"
        };
        (g::TABLET, kind)
    } else if google {
        (g::GOOGLE, "Google Pixel")
    } else if samsung {
        (g::ANDROID, "Samsung Galaxy")
    } else if has("oneplus") {
        (g::ANDROID, "OnePlus")
    } else if has("xiaomi") || has("redmi") || has("poco") {
        (g::ANDROID, "Xiaomi")
    } else if has("moto") || has("motorola") {
        (g::ANDROID, "Motorola")
    } else if has("xperia") {
        (g::ANDROID, "Sony Xperia")
    } else if has("fairphone") {
        (g::ANDROID, "Fairphone")
    } else if has("nothing") {
        (g::ANDROID, "Nothing Phone")
    } else {
        return None;
    })
}

/// The badge icon and hover text for a device, from what is known about its
/// system and, for phones and TVs, its name and hostname.
pub fn of(os: &OsInfo, name: &str, hostname: &str) -> Icon {
    let title = os.title();
    let w = words(&[name, hostname]);
    let with = |kind: &str| format!("{kind}, {title}");
    let (glyph, label) = match os.os.as_str() {
        "linux" => {
            let glyph = distro(&os.distro.to_ascii_lowercase()).unwrap_or(g::TUX);
            let kernel = os.os_version.to_ascii_lowercase();
            let pi = kernel.contains("raspi") || kernel.contains("rpi") || kernel.contains("bcm2");
            let label = if pi && os.distro != "raspbian" {
                format!("{title} on a Raspberry Pi")
            } else {
                title
            };
            (glyph, label)
        }
        "windows" => {
            let glyph = match osinfo::windows_release(&os.os_version) {
                Some("11") => g::WINDOWS_11,
                Some("7") => g::WINDOWS_7,
                _ => g::WINDOWS_10,
            };
            (glyph, title)
        }
        "macOS" => (g::APPLE, title),
        "iOS" => {
            if w.iter().any(|v| v.starts_with("ipad")) {
                (g::TABLET, with("iPad"))
            } else {
                (g::PHONE, with("iPhone"))
            }
        }
        "tvOS" => (g::TV, with("Apple TV")),
        "android" => match android(&w) {
            Some((glyph, kind)) => (glyph, with(kind)),
            None => (g::ANDROID, title),
        },
        "freebsd" => (g::FREEBSD, "FreeBSD".into()),
        "openbsd" => (g::OPENBSD, "OpenBSD".into()),
        _ => (g::SERVER, title),
    };
    Icon { glyph, label }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(family: &str, distro: &str, version: &str) -> OsInfo {
        OsInfo {
            os: family.into(),
            distro: distro.into(),
            os_version: version.into(),
            ..Default::default()
        }
    }

    #[test]
    fn distros_get_their_own_logo() {
        let arch = of(&os("linux", "arch", "7.2.11-arch1-1"), "banditbox", "banditbox");
        assert_eq!(arch.glyph, "\u{f303}");
        assert_eq!(arch.label, "Arch Linux");
        assert_eq!(of(&os("linux", "cachyos", ""), "a", "a").glyph, "\u{f385}");
        assert_eq!(of(&os("linux", "opensuse-tumbleweed", ""), "a", "a").glyph, "\u{f37d}");
        // A distro without a logo, or none known yet, is Tux.
        assert_eq!(of(&os("linux", "gokrazy", ""), "a", "a").glyph, g::TUX);
        assert_eq!(of(&os("linux", "", ""), "a", "a").glyph, g::TUX);
        let pi = of(&os("linux", "ubuntu", "7.0.0-1020-raspi"), "diablo-pi", "diablo-pi");
        assert_eq!(pi.glyph, "\u{f31b}");
        assert_eq!(pi.label, "Ubuntu on a Raspberry Pi");
    }

    #[test]
    fn windows_marks_follow_the_build() {
        assert_eq!(of(&os("windows", "", "10.0.26100.1"), "a", "a").glyph, g::WINDOWS_11);
        assert_eq!(of(&os("windows", "", "10.0.19045.1"), "a", "a").glyph, g::WINDOWS_10);
        assert_eq!(of(&os("windows", "", "6.1.7601"), "a", "a").glyph, g::WINDOWS_7);
        assert_eq!(of(&os("windows", "", ""), "a", "a").glyph, g::WINDOWS_10);
    }

    #[test]
    fn handhelds_read_their_kind_from_the_name() {
        let pixel = of(&os("android", "", "17"), "pixel-8-pro", "Pixel 8 Pro");
        assert_eq!(pixel.glyph, g::GOOGLE);
        assert_eq!(pixel.label, "Google Pixel, Android 17");
        let tv = of(&os("android", "", "12"), "shield", "SHIELD Android TV");
        assert_eq!((tv.glyph, tv.label.as_str()), (g::TV, "NVIDIA Shield, Android 12"));
        let tab = of(&os("android", "", "14"), "galaxy-tab-s9", "Galaxy Tab S9");
        assert_eq!((tab.glyph, tab.label.as_str()), (g::TABLET, "Samsung Galaxy Tab, Android 14"));
        let galaxy = of(&os("android", "", "15"), "sm-s918b", "SM-S918B");
        assert_eq!(galaxy.label, "Samsung Galaxy, Android 15");
        // A renamed phone gives nothing away and stays the robot.
        let renamed = of(&os("android", "", "17"), "01x-engineer", "0,1x Engineer");
        assert_eq!((renamed.glyph, renamed.label.as_str()), (g::ANDROID, "Android 17"));
        // "tab" inside another word doesn't make a tablet.
        assert_eq!(of(&os("android", "", "17"), "stable", "stable").glyph, g::ANDROID);
        let ipad = of(&os("iOS", "", "18.1"), "ipad", "iPad");
        assert_eq!((ipad.glyph, ipad.label.as_str()), (g::TABLET, "iPad, iOS 18.1"));
        assert_eq!(of(&os("iOS", "", "18.1"), "phone", "localhost").glyph, g::PHONE);
        assert_eq!(of(&os("macOS", "", "15.6"), "mac", "mac").glyph, g::APPLE);
    }
}
