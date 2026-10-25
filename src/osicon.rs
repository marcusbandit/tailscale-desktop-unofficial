//! The icon a device's badge shows and what hovering it says: the distro's
//! logo, the mark of its Windows, Apple's mark, or for phones, tablets and TVs
//! the kind of device its name suggests. The glyphs come from a subset of the
//! Nerd Fonts symbols that ships with the app, `assets/os-icons.ttf`, so they
//! show whatever fonts the system has.

use crate::osinfo::{self, OsInfo};

#[derive(Clone)]
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
    /// The flat four squares; the tilted mark is Windows 8 and 10.
    pub const WINDOWS_11: &str = "\u{e8e5}";
    pub const WINDOWS_10: &str = "\u{f17a}";
    pub const WINDOWS_7: &str = "\u{f0a21}";
    // Phone and TV makers, drawn from Simple Icons into the bundled font.
    // Makers whose only logo is a wordmark (Samsung, Sony, Oppo, Vivo) stay
    // the robot: a word shrunk to a badge can't be read.
    pub const MOTOROLA: &str = "\u{100001}";
    pub const ONEPLUS: &str = "\u{100002}";
    pub const XIAOMI: &str = "\u{100003}";
    pub const HUAWEI: &str = "\u{100004}";
    pub const NVIDIA: &str = "\u{100005}";
    pub const AMAZON: &str = "\u{100006}";
    pub const FREEBSD: &str = "\u{f30c}";
    pub const OPENBSD: &str = "\u{f328}";
}

pub struct Distro {
    /// The name as people say it: "Arch Linux", "openSUSE Tumbleweed".
    pub name: &'static str,
    pub glyph: &'static str,
}

/// Every os-release `ID` the app knows, with its name and logo. Derivatives
/// without a logo of their own show their base, the family they belong to.
pub fn distro(id: &str) -> Option<Distro> {
    let (name, glyph) = match id.to_ascii_lowercase().as_str() {
        "arch" => ("Arch Linux", "\u{f303}"),
        "archarm" => ("Arch Linux ARM", "\u{f303}"),
        "ubuntu" => ("Ubuntu", "\u{f31b}"),
        "kubuntu" => ("Kubuntu", "\u{f333}"),
        "debian" => ("Debian", "\u{f306}"),
        "raspbian" => ("Raspberry Pi OS", "\u{f315}"),
        "devuan" => ("Devuan", "\u{f307}"),
        "fedora" => ("Fedora", "\u{f30a}"),
        "bazzite" => ("Bazzite", "\u{f30a}"),
        "ultramarine" => ("Ultramarine Linux", "\u{f30a}"),
        "nobara" => ("Nobara", "\u{f380}"),
        "nixos" => ("NixOS", "\u{f313}"),
        "opensuse-tumbleweed" => ("openSUSE Tumbleweed", "\u{f37d}"),
        "opensuse-leap" => ("openSUSE Leap", "\u{f37e}"),
        "opensuse" => ("openSUSE Leap", "\u{f314}"),
        "opensuse-microos" => ("openSUSE MicroOS", "\u{f314}"),
        "opensuse-slowroll" => ("openSUSE Slowroll", "\u{f314}"),
        "sles" => ("SUSE Linux Enterprise Server", "\u{f314}"),
        "sled" => ("SUSE Linux Enterprise Desktop", "\u{f314}"),
        "alpine" => ("Alpine", "\u{f300}"),
        "centos" => ("CentOS", "\u{f304}"),
        "rhel" => ("Red Hat Enterprise Linux", "\u{f316}"),
        "rocky" => ("Rocky Linux", "\u{f32b}"),
        "almalinux" => ("AlmaLinux", "\u{f31d}"),
        "gentoo" => ("Gentoo", "\u{f30d}"),
        "void" => ("Void Linux", "\u{f32e}"),
        "manjaro" => ("Manjaro", "\u{f312}"),
        "manjaro-arm" => ("Manjaro ARM", "\u{f312}"),
        "endeavouros" => ("EndeavourOS", "\u{f322}"),
        "cachyos" => ("CachyOS", "\u{f385}"),
        "garuda" => ("Garuda Linux", "\u{f337}"),
        "artix" => ("Artix Linux", "\u{f31f}"),
        "archcraft" => ("Archcraft", "\u{f345}"),
        "arcolinux" => ("ArcoLinux", "\u{f346}"),
        "pop" => ("Pop!_OS", "\u{f32a}"),
        "linuxmint" => ("Linux Mint", "\u{f30e}"),
        "elementary" => ("elementary OS", "\u{f309}"),
        "zorin" => ("Zorin OS", "\u{f32f}"),
        "neon" => ("KDE neon", "\u{f331}"),
        "kali" => ("Kali Linux", "\u{f327}"),
        "parrot" => ("Parrot OS", "\u{f329}"),
        "mx" => ("MX Linux", "\u{f33f}"),
        "deepin" => ("deepin", "\u{f321}"),
        "solus" => ("Solus", "\u{f32d}"),
        "slackware" => ("Slackware", "\u{f318}"),
        "mageia" => ("Mageia", "\u{f310}"),
        "qubes" => ("Qubes OS", "\u{f342}"),
        "tails" => ("Tails", "\u{f343}"),
        "trisquel" => ("Trisquel", "\u{f344}"),
        "puppy" => ("Puppy Linux", "\u{f341}"),
        "aosc" => ("AOSC OS", "\u{f301}"),
        "fedora-coreos" => ("Fedora CoreOS", "\u{f305}"),
        "vanilla" => ("Vanilla OS", "\u{f366}"),
        "postmarketos" => ("postmarketOS", "\u{f374}"),
        "openwrt" => ("OpenWrt", "\u{f382}"),
        "steamos" => ("SteamOS", "\u{f1b6}"),
        "haos" => ("Home Assistant OS", "\u{f07d0}"),
        "synology" => ("Synology DSM", g::NAS),
        "qnap" => ("QNAP", g::NAS),
        "unraid" => ("Unraid", g::NAS),
        "truenas" => ("TrueNAS", g::NAS),
        "wdmycloud" => ("WD My Cloud", g::NAS),
        "pfsense" => ("pfSense", g::FREEBSD),
        "opnsense" => ("OPNsense", g::FREEBSD),
        "gokrazy" => ("gokrazy", g::TUX),
        _ => return None,
    };
    Some(Distro { name, glyph })
}

/// Shown for a system the app has nothing better for.
pub const UNKNOWN: &str = g::SERVER;

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
    // Samsung model numbers read SM-S918B: "sm", then a letter and digits.
    let model = |p: &[String]| {
        p[0] == "sm"
            && p[1].starts_with(|c: char| c.is_ascii_alphabetic())
            && p[1].contains(|c: char| c.is_ascii_digit())
    };
    let samsung = has("galaxy") || has("samsung") || w.windows(2).any(model);
    Some(if has("shield") {
        (g::NVIDIA, "NVIDIA Shield")
    } else if starts("chromecast") {
        (g::TV, "Chromecast with Google TV")
    } else if has("firetv") || (has("fire") && has("tv")) {
        (g::AMAZON, "Amazon Fire TV")
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
        (g::ONEPLUS, "OnePlus")
    } else if has("xiaomi") || has("redmi") || has("poco") {
        (g::XIAOMI, "Xiaomi")
    } else if has("moto") || has("motorola") || has("razr") {
        (g::MOTOROLA, "Motorola")
    } else if has("huawei") {
        (g::HUAWEI, "Huawei")
    } else if has("honor") {
        (g::ANDROID, "Honor")
    } else if has("oppo") {
        (g::ANDROID, "Oppo")
    } else if has("vivo") {
        (g::ANDROID, "vivo")
    } else if has("nokia") {
        (g::ANDROID, "Nokia")
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
            let glyph = distro(&os.distro).map_or(g::TUX, |d| d.glyph);
            let kernel = os.os_version.to_ascii_lowercase();
            let pi = kernel.contains("raspi") || kernel.contains("rpi") || kernel.contains("bcm2");
            let label = if pi && !os.distro.eq_ignore_ascii_case("raspbian") {
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
        "freebsd" => (distro(&os.distro).map_or(g::FREEBSD, |d| d.glyph), title),
        "openbsd" => (g::OPENBSD, title),
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
        let rpi = of(&os("linux", "Raspbian", "6.6.51+rpt-rpi-v8"), "pi", "pi");
        assert_eq!(rpi.label, "Raspberry Pi OS");
        assert_eq!(of(&os("linux", "wdmycloud", ""), "a", "a").glyph, g::NAS);
        let pf = of(&os("freebsd", "pfsense", ""), "fw", "fw");
        assert_eq!((pf.glyph, pf.label.as_str()), (g::FREEBSD, "pfSense"));
        assert_eq!(of(&os("freebsd", "", ""), "a", "a").label, "FreeBSD");
        assert_eq!(of(&os("freebsd", "truenas", ""), "a", "a").glyph, g::NAS);
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
        assert_eq!((tv.glyph, tv.label.as_str()), (g::NVIDIA, "NVIDIA Shield, Android 12"));
        let tab = of(&os("android", "", "14"), "galaxy-tab-s9", "Galaxy Tab S9");
        assert_eq!((tab.glyph, tab.label.as_str()), (g::TABLET, "Samsung Galaxy Tab, Android 14"));
        assert_eq!(of(&os("android", "", "14"), "oneplus-12", "OnePlus 12").glyph, g::ONEPLUS);
        assert_eq!(of(&os("android", "", "14"), "redmi-note-13", "Redmi Note 13").glyph, g::XIAOMI);
        assert_eq!(of(&os("android", "", "14"), "moto-g84", "moto g84").glyph, g::MOTOROLA);
        let galaxy = of(&os("android", "", "15"), "sm-s918b", "SM-S918B");
        assert_eq!(galaxy.label, "Samsung Galaxy, Android 15");
        // A renamed phone gives nothing away and stays the robot.
        let renamed = of(&os("android", "", "17"), "01x-engineer", "0,1x Engineer");
        assert_eq!((renamed.glyph, renamed.label.as_str()), (g::ANDROID, "Android 17"));
        // "tab" inside another word doesn't make a tablet, nor "aft" a Fire TV,
        // nor initials SM a Samsung.
        assert_eq!(of(&os("android", "", "17"), "stable", "stable").glyph, g::ANDROID);
        assert_eq!(of(&os("android", "", "14"), "aftab-phone", "aftab").glyph, g::ANDROID);
        assert_eq!(of(&os("android", "", "14"), "sm-phone", "SM phone").label, "Android 14");
        let fire = of(&os("android", "", "9"), "fire-tv-stick", "Fire TV Stick");
        assert_eq!((fire.glyph, fire.label.as_str()), (g::AMAZON, "Amazon Fire TV, Android 9"));
        let ipad = of(&os("iOS", "", "18.1"), "ipad", "iPad");
        assert_eq!((ipad.glyph, ipad.label.as_str()), (g::TABLET, "iPad, iOS 18.1"));
        assert_eq!(of(&os("iOS", "", "18.1"), "phone", "localhost").glyph, g::PHONE);
        assert_eq!(of(&os("macOS", "", "15.6"), "mac", "mac").glyph, g::APPLE);
    }
}
