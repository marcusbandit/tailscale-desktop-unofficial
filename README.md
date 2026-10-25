# Tailscale Desktop (unofficial)

<img src="assets/icon.svg" width="96" alt="The Tailscale mark with its top right dot lit blue">

**Unofficial.** This app is made independently and is not affiliated with, endorsed by or made by
Tailscale Inc. Tailscale and the Tailscale logo are trademarks of Tailscale Inc. The logo is used
so people can see at once what the app is for. The top right dot is lit blue to mark it as the
unofficial one. If anyone at Tailscale wants this code, it is theirs to take.

A small native Linux window for Tailscale, written in Rust with egui. It talks to the local
`tailscaled` over its socket, so it needs no API key and no web UI.

On first launch a short walkthrough explains the app and offers to set up OS versions. Welcome
tour in Settings, or in the menu behind the status pill, opens it again.

What it shows and does:

- every device's own OS logo: the distro (Arch, Ubuntu, Debian, Fedora, openSUSE, NixOS, Mint,
  CachyOS and about 45 more), the mark of its Windows 11, 10 or 7, Apple's mark, and for phones,
  tablets and TVs the kind of device its name suggests. Hover a badge to read what it is
- every device on the tailnet, online or when it was last seen, owner or tags, OS, and a note only
  when something is off: a relayed path, or access blocked one way or both
- search by name or IP, or narrow it with `tag:`, `os:`, `user:` and `is:offline`
- click any IP or MagicDNS name to copy it
- privacy mode, from the eye in the header or Ctrl+Shift+P: IPs show as `100.••.••.21`, accounts,
  owners and endpoints are hidden, and device names can become device-1, device-2. Copying still
  copies the real address. It stays on until you turn it off
- ping a device five times and see latency and the path
- an access graph of who can open connections to whom, with one-way links drawn as arrows
- Taildrop: files waiting for this device, save them, send files to a device (or drop them on the window)
- in Settings: exit node picker, LAN access, subnet routes, offering this device as an exit node,
  shields up, which columns the device list shows, and version info
- an on/off switch for Tailscale itself in the status menu
- health warnings from tailscaled, with a one-click fix where there is one (IP forwarding)

## Run

    cargo run --release

or install it for your user, with a desktop entry and icon:

    scripts/install.sh

Your user should be the Tailscale operator so changes work without root:

    sudo tailscale set --operator=$USER

If it is not, the on/off switch falls back to asking for root through polkit or a terminal running sudo.

## OS versions

tailscaled knows this device's distro and kernel, but peers only report their OS family. For
the rest the app asks the Tailscale API. The first-run walkthrough sets this up, and Set up under OS
versions in Settings opens the same steps: create an OAuth trust credential with
the All - Read scope, and paste its secret. It is kept in the
desktop keyring through `secret-tool` and can't change anything on the tailnet. The API gives
the distro, the kernel on Linux, and the OS version on Android, Windows, macOS and iOS. Results
are cached in `~/.cache/tailscale-desktop-unofficial/os.json` and refresh every 15 minutes.

## How the access graph knows

A device only receives the packet filter for its own inbound traffic, so the graph combines:

- exact: this device's packet filter, which says who may reach it and on which ports
- probed: an ICMP ping through the tunnel to each online peer, which only comes back when the
  peer's filter lets this device in
- inferred: policies are written against users and tags, so devices with the same owner and tags
  are drawn the way a known device of that kind behaves. Inferred links are dashed.

## Test hooks

`TSDU_SHOT=out.png` renders for `TSDU_DELAY` seconds (default 6), writes a PNG of the window and
quits. `TSDU_VIEW=graph|taildrop|settings` picks the view, `TSDU_MENU=1` opens the status menu,
`TSDU_PRIVACY=1` and `TSDU_PRIVACY_NAMES=1` turn privacy mode on, `TSDU_SHOW_IPS=0` and
`TSDU_SHOW_KERNEL=1` set the list columns, `TSDU_SCROLL=<px>` holds Settings scrolled down, `TSDU_SELECT=<device>` opens a device, and
`TSDU_PING=1` pings it first. `TSDU_ONBOARD=0..3` opens the first-run walkthrough at that page. `TSDU_PAN_AT=<seconds>` throws the graph camera off screen
at that moment to test that it returns.

## Icons

The OS logos in `assets/os-icons.ttf` are a subset of Symbols Nerd Font 3.5.1 from
[Nerd Fonts](https://github.com/ryanoasis/nerd-fonts), MIT licensed; see
`assets/os-icons-LICENSE.txt`. The logos are trademarks of their owners and are shown only to
say what each device runs.
