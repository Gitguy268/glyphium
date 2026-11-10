# Template for Gitguy268/homebrew-tap Casks/glyphium-app.rb. Fill in SHA256_APP from the
# release's SHA256SUMS. The `glyphium` formula (a bare binary built from source) stays as well.
cask "glyphium-app" do
  version "0.1.0"
  sha256 "SHA256_APP"

  url "https://github.com/Gitguy268/glyphium/releases/download/v#{version}/Glyphium-#{version}-macos-arm64.zip"
  name "Glyphium"
  desc "GPU-rendered terminal emulator built on libghostty-vt"
  homepage "https://github.com/Gitguy268/glyphium"

  depends_on arch: :arm64
  depends_on macos: ">= :ventura"

  conflicts_with formula: "gitguy268/tap/glyphium"

  app "Glyphium.app"
  binary "#{appdir}/Glyphium.app/Contents/MacOS/glyphium"

  # The release build is signed ad hoc, not notarised; clear the download quarantine so it opens.
  postflight do
    system_command "/usr/bin/xattr", args: ["-dr", "com.apple.quarantine", "#{appdir}/Glyphium.app"]
  end

  zap trash: "~/.config/glyphium"

  caveats <<~EOS
    Glyphium is not signed by Apple. The cask removes the quarantine flag after install.
  EOS
end
