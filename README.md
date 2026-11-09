<div align="center">

# Glyphium

A GPU-rendered terminal emulator in Rust, built on libghostty-vt.

[![CI](https://github.com/Gitguy268/glyphium/actions/workflows/ci.yml/badge.svg)](https://github.com/Gitguy268/glyphium/actions/workflows/ci.yml)

</div>

**Status: early development.** Milestone 1 is closed: the desktop app builds and passes real-window end-to-end scripts, and the core passes fuzz tests. See [docs/PROGRESS.md](docs/PROGRESS.md) and [docs/adr/0001-core.md](docs/adr/0001-core.md).

<!-- live-links -->
**Project page:** [gitguy268.github.io/glyphium](https://gitguy268.github.io/glyphium/) · [glyphium-red.vercel.app](https://glyphium-red.vercel.app)
<!-- /live-links -->

## Install with Homebrew

```sh
brew trust gitguy268/tap
brew install gitguy268/tap/glyphium
```

This builds Ghostty's terminal core and glyphium from source on macOS (a few minutes; Rust and Zig are fetched as build tools). Then run `glyphium`.

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

## Using it

### Queue the next command

Press **Cmd+Shift+Enter** to open a `Queue next:` line in the top bar. Type a command and press **Enter**. If the shell is at its prompt, the command runs at once. Otherwise it waits until the current command finishes. You can queue several, and they run one at a time in order. The top bar shows what is waiting (`Queued (2): npm run build → git push`), and a tab with queued commands shows `⏳n` in its label.

| In the queue line | Does |
| --- | --- |
| Enter | Adds the command |
| Esc | Closes the line |
| Backspace on an empty line | Removes the last queued command |
| Cmd+V | Pastes, with newlines turned into spaces |
| Cmd+Shift+Backspace | Clears the pane's queue (also works outside the line) |

<details>
<summary>How Glyphium knows a command has finished</summary>

Glyphium checks which process owns the terminal. While the shell is at its prompt, it owns the terminal; while a command runs, that command does. No shell setup is needed. After typing a queued command, Glyphium waits until it has seen that command run and the shell take the terminal back before typing the next one. A program that reads input, such as `head`, `ssh` or a password prompt, therefore never receives the next queued command as its input. Builtins such as `cd` never leave the shell, so for those Glyphium waits until the output has been quiet for 150 ms.

Limits:
- The queue needs a shell. It is turned off when the window runs another program with `-e`.
- A builtin that waits for input, such as `read`, looks finished.
- It does not yet work on Windows.

</details>

### Themes and settings

**Cmd+Shift+T** cycles the colour theme for every pane: default, Nujabes, Ye: Graduation, then back to default. To keep a theme, or to change the font size or scrollback, create `~/.config/glyphium/config.toml`:

```toml
font_size = 14        # points
theme = "nujabes"     # default, nujabes or graduation
scrollback = 20000    # lines per pane
```

Glyphium reads `--config FILE` first, then `$GLYPHIUM_CONFIG`, then that file. A line it can't read prints a warning and keeps the default.

<details>
<summary>All shortcuts</summary>

On Linux and Windows, use Ctrl+Shift where macOS uses Cmd (the theme shortcut is Ctrl+Shift+Alt+T).

| Shortcut | Does |
| --- | --- |
| Cmd+T | New tab |
| Cmd+W | Close pane |
| Cmd+D / Cmd+Shift+D | Split right / split down |
| Cmd+Alt+Arrow | Move focus between splits |
| Cmd+[ / Cmd+] | Previous / next tab |
| Cmd+1…9 | Go to tab |
| Cmd+C / Cmd+V | Copy selection / paste |
| Cmd+= / Cmd+- / Cmd+0 | Bigger / smaller / reset font |
| Shift+PageUp / Shift+PageDown | Scroll back |
| Cmd+Shift+Enter | Queue the next command |
| Cmd+Shift+Backspace | Clear the queue |
| Cmd+Shift+T | Next theme |
| Cmd+click | Open a link |
| Cmd+Q | Quit |

</details>

## Tests

```sh
cargo test --release -p glyphium-terminal-core -p glyphium-terminal-render -p glyphium-session-host -p glyphium
cargo build --release -p glyphium && tests/e2e/run.sh OUT_DIR   # opens a real window per script
```

CI runs the first line and clippy on macOS for every push and pull request.

## Licence

See [LICENSE](LICENSE).
