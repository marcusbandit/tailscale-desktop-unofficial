# Tailscale Desktop (unofficial)

A small native Linux window for Tailscale, written in Rust with egui. It talks to the local
`tailscaled` over its socket, so it needs no API key and no web UI.

What it shows and does:

- every device on the tailnet, online or when it was last seen, owner or tags, OS, direct or relayed
- click any IP or MagicDNS name to copy it
- ping a device five times and see latency and the path
- an access graph of who can open connections to whom, with one-way links drawn as arrows
- Taildrop: files waiting for this device, save them, send files to a device (or drop them on the window)
- exit node picker, LAN access, subnet routes, offering this device as an exit node, shields up
- an on/off switch for Tailscale itself and the daemon version

## Run

    cargo run --release

Your user should be the Tailscale operator so changes work without root:

    sudo tailscale set --operator=$USER

If it is not, the on/off switch falls back to asking for root through polkit or a terminal running sudo.

## How the access graph knows

A device only receives the packet filter for its own inbound traffic, so the graph combines:

- exact: this device's packet filter, which says who may reach it and on which ports
- probed: an ICMP ping through the tunnel to each online peer, which only comes back when the
  peer's filter lets this device in
- inferred: policies are written against users and tags, so devices with the same owner and tags
  are drawn the way a known device of that kind behaves. Inferred links are dashed.

## Test hooks

`TSDU_SHOT=out.png` renders for `TSDU_DELAY` seconds (default 6), writes a PNG of the window and
quits. `TSDU_VIEW=graph|taildrop` picks the view, `TSDU_SELECT=<device>` opens a device, and
`TSDU_PING=1` pings it first.
