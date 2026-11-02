<div align="center">

# Glyphium

A GPU-rendered terminal emulator in Rust, built on libghostty-vt.

</div>

**Status: early development.** Milestone 1 is closed: the desktop app builds and passes real-window end-to-end scripts, and the core passes fuzz tests. See [docs/PROGRESS.md](docs/PROGRESS.md) and [docs/adr/0001-core.md](docs/adr/0001-core.md).

<!-- run-steps:start -->
## Run it locally, step by step

1. **Install [Rust](https://rustup.rs)** and [Zig](https://ziglang.org/download/), which builds the terminal core.

2. **Get the code:**

   ```sh
   git clone https://github.com/Gitguy268/glyphium.git
   cd glyphium
   ```

3. **Build the terminal core from Ghostty:**

   ```sh
   git clone https://github.com/ghostty-org/ghostty.git .tools/ghostty
   git -C .tools/ghostty checkout cec8b92
   (cd .tools/ghostty && zig build -Demit-lib-vt -Doptimize=ReleaseFast)
   ```

4. **Build and start the desktop app:**

   ```sh
   cargo run --release -p glyphium
   ```

5. **Quit** by closing the window. The first build takes several minutes.
<!-- run-steps:end -->

## Licence

See [LICENSE](LICENSE).
