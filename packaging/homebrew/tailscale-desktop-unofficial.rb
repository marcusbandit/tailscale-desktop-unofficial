cask "tailscale-desktop-unofficial" do
  version "@VERSION@"
  sha256 "@SHA_MACOS@"

  url "https://github.com/marcusbandit/tailscale-desktop-unofficial/releases/download/v#{version}/tailscale-desktop-unofficial-#{version}-macos-universal.zip"
  name "Tailscale Desktop (unofficial)"
  desc "Unofficial desktop app for Tailscale: devices, access graph and Taildrop"
  homepage "https://github.com/marcusbandit/tailscale-desktop-unofficial"

  depends_on macos: ">= :big_sur"

  app "Tailscale Desktop (unofficial).app"

  # The app is signed ad hoc, not with a Developer ID, so Gatekeeper would
  # refuse it straight from the download. Lift the quarantine flag instead.
  postflight do
    system_command "/usr/bin/xattr",
                   args: ["-dr", "com.apple.quarantine", "#{appdir}/Tailscale Desktop (unofficial).app"]
  end

  zap trash: [
    "~/Library/Application Support/tailscale-desktop-unofficial",
    "~/Library/Caches/tailscale-desktop-unofficial",
  ]

  caveats <<~EOS
    Not made by or affiliated with Tailscale Inc. It needs the Tailscale app
    installed and running: brew install --cask tailscale-app
  EOS
end
