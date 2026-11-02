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

## 2026-10-25: M0 baseline

- `cargo run --release -p glyphium-fixtures` writes 5 deterministic fixtures (2.4–11.4 MB). `fixtures/MANIFEST.txt` holds their SHA-256; two runs produced identical bytes.
- `bench/web`: xterm.js 6.0.0 + WebGL 0.19.0 with TabShell's options, in Playwright Chromium 153 (headless, ANGLE Metal on the M4). `npm run baseline` → `bench/results/2026-10-25-xterm-baseline/`.
  - 20 trials after 3 warmups, randomized order, 4 KiB chunks. Medians: 25 MB/s ASCII, 25–30 MB/s long lines, 20–22 MB/s TUI redraw, 16–18 MB/s Unicode, **3.4–3.6 MB/s truecolor SGR**, with 83 ms median frame gaps (the main thread is blocked while parsing).
  - Caveat: Zig/Ghostty/cargo downloads ran during this run. Re-run alongside Glyphium before any comparison is claimed.
- **xterm.js 6.0.0 bug found:** when a `Uint8Array` write ends after the 2nd of the 3 UTF-8 bytes of U+200D (ZWJ), the ZWJ is dropped (`👩‍👧` becomes `👩👧`). Splits at other bytes are fine, and so is the string path TabShell uses (`TextDecoder` with `stream: true`). So the TabShell backlog item "pass Uint8Array straight to term.write" must not ship on 6.0.0. Reporting it upstream is outward-facing, so ask the user first.

## 2026-10-26: M1 core bake-off (ADR 0001 proposed)

Result: **libghostty-vt proposed** (`docs/adr/0001-core.md`, status Proposed). It adds Zig to the toolchain, so the user signs off before M2 builds on it.
- Ghostty commit: `cec8b92f1ecd176a20a56aa405021e7ae991b996`. Builds: `.tools/ghostty/zig-out` (native ReleaseFast), `zig-out-wasm` (wasm32 ReleaseSmall), `zig-out-wasm-interactive` (size check only).
- `bench/bakeoff`: one `Core` trait over the three cores, with these commands:
  - `invariance [--mode2027]`
  - `dump`
  - `diff --core X --chunking split-N`
  - `hashes`
  - `probe`
  - `throughput`
- `bench/bakeoff/build-wasm.sh` makes one wasm build per core. `bench/web/run-bakeoff.mjs` runs them in a Web Worker and compares the hashes with native.
- Fixture `edge-cases` added (3.4 KB, split at every byte). MANIFEST updated.
- Vendored and patched in `third_party/` (see its README):
  - alacritty_terminal: wasm32 gating;
  - termwiz and wezterm-escape-parser: wasm32;
  - wezterm-term: synchronous replies;
  - vte 0.15.0: split-UTF-8 fix, alacritty/vte#159, already known upstream, so nothing to report.
- Findings:
  - Ghostty passes every invariance run, with mode 2027 off and on.
  - Native and wasm hashes are identical for all three cores.
  - wezterm-term breaks grapheme clusters split across writes.
  - Ghostty's default scrollback byte limit kept only about 565 lines, so the adapter removes it.
- Not done: vttest (moves to M2), Windows Zig CI, and a throughput re-run on an idle machine (load average was about 5).
- Disk ran out once during this session (1 GB free). `target/` regrows to about 2–3 GB. `bench/results/2026-10-25-bakeoff/invariance-big.log` is a stale, untracked log from the pre-fix run.

## 2026-11-01: M1 closed

- ADR 0001 accepted (user replied "build"). FFI moved to `crates/core-ffi`; bake-off hashes unchanged after the move.
- `crates/terminal-core`: frames through Ghostty's render-state API, dirty rows only, mode 2027 on by default, replies drained in order.
- `crates/terminal-render`: wgpu instanced quads, cosmic-text + swash shaping, one bounded 2048px page each for mask and colour glyphs (clear-all eviction), block and box characters drawn as rectangles.
- `crates/terminal-wasm`: browser build, 2.87 MB wasm / 1.16 MB gzip with the four bundled fonts.
- **M1 text gate:** the demo frame rendered by native Metal and by WebGPU in Playwright Chromium on the Apple M4 (`metal-3`, not a fallback adapter) differs in 0 of 384,000 pixels (`bench/results/2026-11-01-render/`). Run: `bench/web/build-render.sh`, build the `render_png` example, `node bench/web/run-render.mjs OUT`.
- The trimmed Ghostty feature set is not used: it lacks `ghostty_cell_get`, which frames need for cell widths.

## 2026-11-01: input layer (M3 next)

- `crates/terminal-core/src/input.rs`: key, mouse, focus and paste encoding through Ghostty's encoders; viewport scrolling; selection and selection text. W3C key names map to Ghostty keys, so winit and browser events share one table. 13 core tests pass.
- Next, in order:
  1. `crates/session-host` (portable-pty 0.9): reader thread with a bounded queue, writer thread, generation-tagged events; test against a real bash, then vim, less, htop and tmux.
  2. `apps/desktop` (winit 0.30 + wgpu): single pane first, surface format without the sRGB suffix, draw only when `Renderer::update` returns true; then tabs, splits, IME.
  3. Renderer tests: idle draws nothing, only dirty rows rebuild, atlas eviction, device loss.
  4. M2: vttest and a fuzz corpus, driven through the PTY layer. Web client: canvas surface and input bridge.
- Known gaps (both since closed: `crates/terminal-render/tests/gpu.rs`, and the paste banner in `apps/desktop`): `crates/terminal-render` has no automated tests yet (M1 evidence is the pixel comparison). Unbracketed multi-line paste has `paste_needs_confirmation` but no UI uses it yet.
- Open questions for the user: report the wezterm grapheme-split bug and the xterm.js 6.0.0 ZWJ bug upstream? Delete the stale `bench/results/2026-10-25-bakeoff/invariance-big.log`?

## 2026-11-01: colour themes

- `crates/terminal-core/src/theme.rs`: Nujabes and Ye: Graduation. `Terminal::set_theme(Some(&theme))` sets Ghostty's default foreground, background, cursor and first 16 palette entries; `None` restores the defaults. Colours a program set through OSC survive a change. `set_theme` bumps a generation that `update_frame` compares, because a palette-only change leaves Ghostty's render state clean.
- `Frame::selection`: the renderer uses it as the selection background, and still inverts when it is `None`, so the default look is unchanged.
- Exposed as `WebTerminal::set_theme(name)` and `render_png --theme NAME`. No picker yet: `apps/desktop` needs a config or menu to call `set_theme`.
- Default demo frame re-checked: 0 of 384,000 pixels differ between native and browser. Both themes through `set_theme` also differ in 0 pixels. Credits are in NOTICE.

## 2026-11-01: desktop app runs; M2 fuzzing

Stopped at 3.0 GB free (limit 5 GB; several sessions share this disk). Nothing here is committed: `crates/terminal-render/src/lib.rs` and `Cargo.*` also hold the themes session's changes.

- `apps/desktop` (binary `glyphium`): tabs, splits, zoom, clipboard, paste confirmation, IME preedit, GPU-loss recovery. `tests/e2e/run.sh [OUT_DIR] [names]` runs 9 scripts through the real window, Metal, PTY and `/bin/bash`; all exit 0 on the final binary, using `target/release/glyphium`. Screenshots were read by eye for splits, CJK and emoji, the paste banner, vim and GPU loss.
- Fixed while verifying:
  - wgpu 30 presents through `queue.present`.
  - Offscreen readback used a fixed format, which crashed on the window's BGRA pipeline.
  - The device-lost callback needs a poll, so `redraw` polls first.
  - Pressing Cmd alone cancelled the paste banner, so Cmd+V could never confirm. This fix is by code reading: scripts enter past the OS event and do not press real keys.
- `[profile.release] debug = 0` (was 1) to save disk.
- `crates/terminal-core/tests/fuzz.rs`: seeded hostile streams. 1000 seeds each pass: well-formed frames under resizes and user actions, chunking changes neither screen nor replies, incremental frames equal a full read (585,145 of 5,738,027 rows rebuilt, so dirty tracking was exercised), 16 MB control strings stay bounded. Run with `FUZZ_SEEDS=1000 cargo test --release -p glyphium-terminal-core --test fuzz`.
- Not tested:
  - Real OS input: physical keys, IME composition, Cmd shortcuts, clicking hyperlinks, mouse reporting to programs. Selection and wheel are scripted past the OS event.
  - tmux and htop (not installed here); only vim and less ran as full-screen programs inside the app.
  - Linux and Windows.
  - A steady cursor drawing nothing: `idle.script` allows up to 7 draws, the same as the blink rate, and does not print the count, so a redraw on every blink tick would pass. Next time set `\e[2 q`, expect 0 draws and print the count.
- Blocked: vttest. The permission system refused downloading and compiling it, so M2's vttest gate is open.
- Next, in order: user try-out of real input; vttest once allowed; web client (canvas surface, input bridge; needs a wasm build, so free disk first); theme choice in `apps/desktop`.
