<div align="center">

<img src="assets/icon.svg" width="96" alt="App icon: the Tailscale mark with its top right dot lit blue">

# Tailscale Desktop (unofficial)

A desktop app for your tailnet. See every device, who can reach whom, and send files with Taildrop.

[![platform](https://img.shields.io/badge/platform-linux-5B9BFF?style=for-the-badge&logo=linux&logoColor=E7E9EE&labelColor=0D0F13)](#install)
[![built with rust](https://img.shields.io/badge/built_with-rust_+_egui-5B9BFF?style=for-the-badge&logo=rust&logoColor=E7E9EE&labelColor=0D0F13)](https://github.com/emilk/egui)
[![last commit](https://img.shields.io/github/last-commit/marcusbandit/tailscale-desktop-unofficial?style=for-the-badge&logo=git&logoColor=E7E9EE&labelColor=0D0F13&color=3DD68C)](https://github.com/marcusbandit/tailscale-desktop-unofficial/commits/main)
[![license](https://img.shields.io/github/license/marcusbandit/tailscale-desktop-unofficial?style=for-the-badge&labelColor=0D0F13&color=8B93A1)](LICENSE)
[![ko-fi](https://img.shields.io/badge/ko--fi-buy_me_a_coffee-FF7A4D?style=for-the-badge&logo=kofi&logoColor=FF7A4D&labelColor=0D0F13)](https://ko-fi.com/marcusbandit)

</div>

> **Unofficial.** This app is made independently and is not affiliated with, endorsed by or made
> by Tailscale Inc. Tailscale and the Tailscale logo are trademarks of Tailscale Inc. The logo is
> used so people can see at once what the app is for. Its top right dot is lit blue to mark it as
> the unofficial one.

![The device list with each device's OS, status and address](assets/screenshots/devices.png)

## Install

Linux for now. Windows and macOS are planned ([#31](https://github.com/marcusbandit/tailscale-desktop-unofficial/issues/31)).

You need Tailscale running, a Rust toolchain and `python3`. Then:

```sh
git clone https://github.com/marcusbandit/tailscale-desktop-unofficial
cd tailscale-desktop-unofficial
scripts/install.sh
sudo tailscale set --operator=$USER
```

The last line lets the app change Tailscale settings without asking for your password. Skip it
and the app asks for root through polkit or sudo when it needs to.

<details>
<summary>Build dependencies</summary>

Debian and Ubuntu need `pkg-config libxkbcommon-dev libwayland-dev` to build. The Taildrop file picker uses `xdg-desktop-portal`, and the API secret is
stored with `secret-tool` from `libsecret-tools`.
</details>

## API credential

Your devices, the access graph, Taildrop and settings come straight from the Tailscale running
on your machine. Some details about other devices, like their OS version and Linux distro, only
come from the Tailscale API, so for those the app needs a read-only API credential.

The app walks you through it on first launch, or later from Settings, Device details, Set up. In
the [admin console](https://console.tailscale.com/admin/settings/trust-credentials):

1. Press **Credential**, keep **OAuth** and press **Continue**.
2. Under **Custom scopes**, pick **All - Read**.
3. Press **Generate credential**, copy the secret and paste it into the app. Tailscale shows it
   only once.

The credential can only read. The app keeps it in your system keyring. You can skip it; devices
then show their OS family without the version. OS details are cached in
`~/.cache/tailscale-desktop-unofficial/os.json`.

## What it does

- Lists every device with its OS logo, online state, owner or tags, and a note when the
  connection is relayed or blocked
- Search by name or IP, or filter with `tag:`, `os:`, `user:` and `is:offline`
- Draws an access graph of who can open connections to whom
- Sends files to one or more devices with Taildrop, and shows files waiting for you
- Pings a device and shows latency and the path
- Turns Tailscale on and off, picks an exit node, offers this device as one, and switches LAN
  access, subnet routes and shields up
- Shows health warnings from Tailscale, with a one-click fix for IP forwarding
- Privacy mode (Ctrl+Shift+P) masks IPs and accounts for streams and screenshots, and device
  names too if you turn that on

The welcome tour reopens from Settings or the status menu.

## Support

The app is free and stays free. If it saves you a trip to the admin console and you feel like
buying me a coffee, there is a [Ko-fi](https://ko-fi.com/marcusbandit). That money goes to me,
not to Tailscale Inc. A star or a good bug report helps just as much.

<div align="center">

[![ko-fi](https://ko-fi.com/img/githubbutton_sm.svg)](https://ko-fi.com/marcusbandit)

</div>

Found a bug or have an idea? See [how to file an issue](CONTRIBUTING.md#filing-an-issue).

## License

MIT, see [LICENSE](LICENSE). Tailscale and the Tailscale logo are trademarks of Tailscale Inc.
and are used here only to say what the app is for. The OS logos come from
[Nerd Fonts](https://github.com/ryanoasis/nerd-fonts) (MIT) and
[Simple Icons](https://simpleicons.org) (CC0); see `assets/os-icons-LICENSE.txt`. Building and
testing notes are in [CONTRIBUTING.md](CONTRIBUTING.md).
