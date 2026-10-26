# Contributing

Bug reports, ideas and pull requests are all welcome. This is a small project run in spare time,
so a clear report gets fixed faster than a vague one.

## Filing an issue

1. Search [open and closed issues](https://github.com/marcusbandit/tailscale-desktop-unofficial/issues?q=is%3Aissue)
   first. If yours is already there, add what is different about your case to it.
2. Press **New issue** and pick **Bug report** or **Feature idea**. The form asks for what it
   needs.
3. Keep one problem per issue.

A good bug report has:

- what you did, what you expected, and what happened instead
- the app version, from Settings, and how you installed it
- your OS and desktop, for example Arch Linux on Hyprland or Ubuntu on GNOME
- the output of `tailscale version`
- a screenshot if it is something you can see. Turn on privacy mode first (Ctrl+Shift+P, and
  Hide device names too in Settings), so your IPs, accounts and device names stay private

Never paste your API credential secret, auth keys or a full `tailscale debug netmap` into an
issue. Issues are public.

Problems with Tailscale itself, such as a device that will not connect in the official client
either, belong with [Tailscale](https://github.com/tailscale/tailscale/issues), not here.

## Contributing code

1. For anything bigger than a small fix, open an issue first so we can agree on the approach.
2. Fork the repo and make a branch named after the issue, like `fix/12-graph-label-overlap`.
3. Build and run with `cargo run --release`, and test with `cargo test`. CI runs the same tests
   on every pull request.
4. Open a pull request that says what changed and why, with `Closes #N` for the issue. For
   anything visible, add a screenshot taken with privacy mode on.

Keep pull requests to one change. A refactor and a feature in one PR are hard to review.

## Screenshots for development

`scripts/shot.sh NAME [VAR=value ...]` runs the debug binary inside a headless `gamescope` and
saves a PNG to `captures/`. It does not build, so run `cargo build` first. Turn privacy mode on
for any screenshot that leaves your machine.

## Test hooks

`TSDU_SHOT=out.png` renders for `TSDU_DELAY` seconds (default 6), writes a PNG of the window and
quits. `TSDU_VIEW=graph|taildrop|settings` picks the view, `TSDU_MENU=1` opens the status menu,
`TSDU_PRIVACY=1` and `TSDU_PRIVACY_NAMES=1` turn privacy mode on, `TSDU_SHOW_IPS=0` and
`TSDU_SHOW_KERNEL=1` set the list columns, `TSDU_SCROLL=<px>` holds Settings scrolled down,
`TSDU_SELECT=<device>` opens a device, and `TSDU_PING=1` pings it first. `TSDU_ONBOARD=0..3` opens
the first-run walkthrough at that page, and `TSDU_SCENE=<n>` or `<n>:<click>` holds its setup
replay on one screen. `TSDU_STAGE=a:b` stages those files in Taildrop and `TSDU_TARGETS=x,y` picks
those devices. `TSDU_PAN_AT=<seconds>` throws the graph camera off screen at that moment to test
that it returns.

## How the access graph knows

A device only receives the packet filter for its own inbound traffic, so the graph combines:

- exact: this device's packet filter, which says who may reach it and on which ports
- probed: an ICMP ping through the tunnel to each online peer, which only comes back when the
  peer's filter lets this device in
- inferred: policies are written against users and tags, so devices with the same owner and tags
  are drawn the way a known device of that kind behaves. Inferred links are dashed.

## Icons

The OS logos in `assets/os-icons.ttf` are a subset of Symbols Nerd Font 3.5.1 plus six phone and
TV maker logos converted from Simple Icons. `scripts/build-os-icons.py` rebuilds the font. The
logos are trademarks of their owners and are shown only to say what each device runs.
