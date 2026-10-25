# Glyphium progress log

Build plan: `~/terminal-research-handoff/CLAUDE-BUILD-PLAN.md` (read `CLAUDE-START-HERE.md` first).
Apple decompiler output stays in the handoff folder and never enters this repo.

## 2026-10-25: M0 started

Done:
- Name chosen by the user: Glyphium. Repo created at `~/glyphium` (MIT, local only; do not push until the user says so).

Toolchain found on this Mac (Apple M4, macOS 27.0.1):
- Rust 1.99.1 (Homebrew first on PATH; rustup toolchain with `wasm32-unknown-unknown` is in `~/.cargo/bin`, so put that first for WASM builds).
- wasm-bindgen 0.2.129, trunk, Node 26.12.1, Docker 29.9.1, cmake. Not installed: zig, wasm-pack, vttest.
- Playwright Chromium with WebGPU: launch with `--enable-unsafe-webgpu --use-angle=metal` (Playwright lives in `~/vinyl-player-realistic-map/*/test/node_modules`).

Bake-off inputs checked:
- libghostty-vt: Ghostty main is 1.3.2-dev and needs Zig >= 0.17.0.
  Zig 0.17.0 for aarch64-macos: https://ziglang.org/download/0.17.0/zig-aarch64-macos-0.17.0.tar.xz,
  sha256 b607e9b9234790a008116ae5bdb71c6243b84b9fb42a53a9e70fde41c06c536a. Unpack it into `.tools/`, not system-wide.
  WASM build: `zig build -Demit-lib-vt -Dtarget=wasm32-freestanding -Doptimize=ReleaseSmall` writes `zig-out/bin/ghostty-vt.wasm`.
  Interactive feature set: `-Dvt-features=-all,+render-state,+input-encode,+selection,+color`. It has a render-state API that a renderer can consume.
- alacritty_terminal: latest on crates.io is 0.26.0.
- wezterm term crate: not on crates.io; use a git dependency on wezterm/wezterm (`term/`), pinned to a commit.

Next (M0):
1. Fixture generator (deterministic bytes: ASCII/SGR flood, CJK/emoji/combining, long lines, TUI redraw, scroll regions, 100k lines).
2. TabShell baseline: xterm.js 6.0.0 + WebGL addon 0.19.0 installed here in `bench/web-baseline` (do not touch `~/tabshell`); measure parse and render throughput in headless Chromium; save the environment manifest and raw results.

Next (M1): adapter trait and a final-state hash for the three cores; chunk-invariance, native throughput, wasm32 build in a Web Worker, sizes; then `docs/adr/0001-core.md`.
