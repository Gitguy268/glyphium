# Vendored crates

Each crate was committed unmodified first, then patched in a separate commit, so `git diff <vendor-commit> -- <dir>` shows exactly what Glyphium changed.

| Crate | Source | Licence | Why vendored |
| --- | --- | --- | --- |
| `alacritty_terminal` | crates.io 0.26.0 | Apache-2.0 | wasm32 build |
| `termwiz` | wezterm/wezterm `e2a98e1b` | MIT | wasm32 build, used by `wezterm-term` |
| `wezterm-escape-parser` | wezterm/wezterm `e2a98e1b` | MIT | wasm32 build with the `kitty-shm` feature on |
| `wezterm-term` | wezterm/wezterm `e2a98e1b` | MIT | wasm32 build (no threads) and deterministic replies |

## Patches (M1 bake-off gate: patch size)

All patches only gate OS-specific code behind `cfg(not(target_family = "wasm"))` or `cfg(unix)`. No terminal-state code was changed.

- **alacritty_terminal** (13 lines of code and manifest): `home`, `libc` and `polling` become non-wasm dependencies; the `event_loop`, `thread` and `tty` modules are left out on wasm. The `ref` test entry was removed because `tests/` was not vendored.
- **termwiz** (8 lines of code; the manifest was rewritten): workspace dependencies replaced with explicit versions taken from wezterm's workspace `Cargo.toml` at the same commit; `filedescriptor`, `libc` and `terminfo` become non-wasm dependencies; `caps`, `istty`, `lineedit`, `render`, `terminal` and `widgets` are left out on wasm, along with the matching error variants.
- **wezterm-escape-parser** (11 lines of code; manifest rewritten the same way): `nix` moves to `cfg(unix)`; a stub `read_shared_memory_data` returns `Unsupported` on targets with neither POSIX nor Win32 shared memory.

- **wezterm-term** (code: one struct replaced; manifest rewritten the same way): upstream sends replies to the PTY writer through a background thread (`ThreadedWriter`), so writes never block. wasm32 cannot spawn threads (it panics in `Terminal::new`), and on native the thread makes reply delivery race the state snapshot. The patch writes replies synchronously. This is not a state change: it moves the non-blocking write out of the core and into Glyphium's session layer. Before the patch, the native edge-case gate showed 445 failures; 367 of them were this race.

`/.cargo/config.toml` sets `getrandom_backend="wasm_js"` for wasm32, needed by `getrandom` 0.3 through `wezterm-blob-leases`.

libghostty-vt is not vendored. It is built from a Ghostty checkout in `.tools/ghostty` (ignored by git); the commit is recorded in `docs/PROGRESS.md`.
