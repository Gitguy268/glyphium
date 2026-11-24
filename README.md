<div align="center">

# Glyphium

A GPU-rendered terminal emulator in Rust, built on libghostty-vt.

[![CI](https://github.com/Gitguy268/glyphium/actions/workflows/ci.yml/badge.svg)](https://github.com/Gitguy268/glyphium/actions/workflows/ci.yml)

</div>

**Status: early development.** Milestone 1 is closed: the desktop app builds and passes real-window end-to-end scripts, and the core passes fuzz tests. See [docs/PROGRESS.md](docs/PROGRESS.md) and [docs/adr/0001-core.md](docs/adr/0001-core.md).

<p align="center"><img src="docs/images/glass-dark.webp" alt="Glyphium with the Glass Dark theme: translucent title bar with three tab pills and a Queue button" width="800"></p>

<p align="center"><sub>Screenshots are the app's own captures laid over a sample wallpaper, since a translucent window shows whatever is behind it.</sub></p>

<!-- live-links -->
**Project page:** [gitguy268.github.io/glyphium](https://gitguy268.github.io/glyphium/) · [glyphium-red.vercel.app](https://glyphium-red.vercel.app)
<!-- /live-links -->

## Install with Homebrew

```sh
brew trust gitguy268/tap
brew install gitguy268/tap/glyphium
```

This builds Ghostty's terminal core and glyphium from source on macOS (a few minutes; Rust and Zig are fetched as build tools). Then run `glyphium`.

Or install the release build as an app (Apple silicon), which puts Glyphium in Applications and `glyphium` on your PATH:

```sh
brew install --cask gitguy268/tap/glyphium-app
```

It's signed ad hoc, not notarised, so the cask clears the quarantine flag. Use the formula or the cask, not both.

<details>
<summary><b>Linux (Flatpak)</b></summary>

Download `io.github.gitguy268.glyphium.flatpak` from the [latest release](https://github.com/Gitguy268/glyphium/releases/latest), then:

```sh
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install --user ./io.github.gitguy268.glyphium.flatpak
flatpak run io.github.gitguy268.glyphium
```

The Linux build is new and only checked in CI. Your shell runs on the host through host-spawn. This Flatpak is built by this repo; it isn't on Flathub. Details are in [packaging/flatpak](packaging/flatpak/README.md).

</details>

<!-- run-steps:start -->
## Run it locally, step by step

1. **Install [Rust](https://rustup.rs)**, [Zig](https://ziglang.org/download/), which builds the terminal core, and [CMake](https://cmake.org), which builds llama.cpp for the command assistant (`brew install zig cmake` on a Mac).

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

5. **Quit** by closing the window. The first build takes several minutes. To build without the assistant's model runner (no CMake needed), use `cargo run --release -p glyphium --no-default-features`; recipes and typo fixes still work.
<!-- run-steps:end -->

## Using it

### Queue the next command

<p align="center"><img src="docs/images/queue-demo.webp" alt="Queueing two commands behind a running one; each runs when the one before it finishes" width="700"></p>

Click **⏎ Queue** in the title bar, choose **Shell › Queue Next Command…**, or press **Cmd+Shift+Enter**. Type a command and press **Enter**. If the shell is at its prompt, the command runs at once. Otherwise it waits until the current command finishes. You can queue several, and they run one at a time in order. The Queue button shows how many are waiting, the popover lists them, and a tab with queued commands shows `⏳n` in its label.

| In the queue popover | Does |
| --- | --- |
| Enter | Adds the command |
| Esc, or a click outside | Closes the popover |
| Backspace on an empty line | Removes the last queued command |
| Cmd+V | Pastes, with newlines turned into spaces |
| Cmd+Shift+Backspace | Clears the pane's queue (also works outside the popover) |

<details>
<summary>How Glyphium knows a command has finished</summary>

Glyphium checks which process owns the terminal. While the shell is at its prompt, it owns the terminal; while a command runs, that command does. No shell setup is needed. After typing a queued command, Glyphium waits until it has seen that command run and the shell take the terminal back before typing the next one. A program that reads input, such as `head`, `ssh` or a password prompt, therefore never receives the next queued command as its input. Builtins such as `cd` never leave the shell, so for those Glyphium waits until the output has been quiet for 150 ms.

Limits:
- The queue needs a shell. It is turned off when the window runs another program with `-e`.
- A builtin that waits for input, such as `read`, looks finished.
- It does not yet work on Windows.

</details>

<p align="center"><img src="docs/images/queue.webp" alt="The queue popover under the Queue button, with two commands waiting" width="800"></p>

### Ask for a command

<p align="center"><img src="docs/images/ask-demo.webp" alt="A mistyped git command fixed after zsh can't find it, then a plain-English request answered by the model" width="800"></p>

Press **Cmd+K** (or **Shell › Ask for a Command…**) and type what you want, such as `find files bigger than 100MB`, or a command you got wrong, such as `gti stauts`. Glyphium shows one command you can edit. A red line warns about risky parts such as `sudo`, `rm -rf` or a script piped from the internet. **Enter** runs it, through the same queue as above, so it waits for the shell to be free. **Esc** cancels. Nothing runs on its own.

When zsh or fish can't find a command, the same popover opens with a fix. Glyphium adds the hook itself and leaves your own files alone: zsh still reads your `.zshenv`, `.zprofile` and `.zshrc`, and a `command_not_found_handler` you define wins. For bash 4 or newer, add `source ~/.config/glyphium/shell/glyphium.bash` to `~/.bashrc`. macOS's own bash 3.2 has no such hook. Turn it off with `assist_hook = false`.

Answers come from three places, in this order:

1. **Recipes** for things a small model gets wrong: `install brew` (Homebrew's official installer), oh-my-fish, oh-my-zsh, `update everything`, switching shells, and every tool in the Extras menu. Each uses the right package name for Homebrew, apt, apk, dnf or pacman.
2. **Typo fixes** for command names and for the subcommands of git, brew, apt, npm, docker, pip and cargo. Commands that exist on your `PATH` are left alone.
3. **A small language model**, run on this computer with llama.cpp. The first time it's needed, Glyphium asks before downloading it (531 MB) into `~/.config/glyphium/models`. After that it answers in well under a second. Nothing you type leaves the computer.

The default model is [Qwen2.5-Coder 0.5B NL2Bash](https://huggingface.co/burhan2753y/qwen2.5-coder-0.5b-nl2bash-gguf) (Apache-2.0). It scored best of three candidates on [TabShell](https://github.com/Gitguy268/tabshell)'s 42-case eval: 40/42 with recipes and typo fixes in front. `assist_model` switches to `qwen-coder` (Qwen2.5-Coder 0.5B Instruct) or `k2-system1` (K2-Horizon 0.9B System 1). [kev-0.8b](https://huggingface.co/jaredpalmer/kev-0.8b) was considered, but it picks between given answers and can't write a command.

<p align="center"><img src="docs/images/ask.webp" alt="The assistant offering Homebrew's installer, with a warning that it runs a script from the internet" width="800"></p>

### Shells, Extras and the cheatsheet

- **Shell for new tabs:** Settings has a **Shell for new tabs** row: your login shell, or zsh, bash or fish when they're installed. The config key is `shell`.
- **Extras menu:** tools from [terminals-are-sexy](https://github.com/k4m4/terminals-are-sexy), including fish, oh-my-fish, Fisher, oh-my-zsh, zsh-autosuggestions, fzf, zoxide, ripgrep and tmux. Choosing one shows its install command in the assistant popover, to confirm.
- **Help › Cheatsheet (Cmd+/):** Glyphium's shortcuts, command-line editing and a few macOS commands.

<p align="center"><img src="docs/images/cheatsheet.webp" alt="The cheatsheet: Glyphium shortcuts, command-line keys, files and macOS commands in two columns" width="800"></p>

### Themes

Pick a theme from **View › Theme**, in **Settings** (Cmd+,), or cycle with **Cmd+Shift+T**. The default, `auto`, follows the system appearance: Glass Light in light mode, Glass Dark in dark mode.

| Built-in theme | Name in the config |
| --- | --- |
| Glass Dark, Glass Light (translucent) | `glass-dark`, `glass-light` |
| Nujabes, Ye: Graduation | `nujabes`, `graduation` |
| Solarized Dark, Solarized Light | `solarized-dark`, `solarized-light` |
| Dracula | `dracula` |
| Catppuccin Mocha, Catppuccin Latte | `catppuccin-mocha`, `catppuccin-latte` |
| Tokyo Night | `tokyo-night` |
| Ghostty's own colours | `default` |

**Your own themes:** put a file in `~/.config/glyphium/themes/`. Its file name is the theme's name, and it appears in the Theme menu and in Settings. The format is Ghostty's, so the hundreds of theme files from Ghostty and [iTerm2-Color-Schemes](https://github.com/mbadolato/iTerm2-Color-Schemes) (its `ghostty/` folder) work as they are. Missing keys come from Glass Dark.

```text
# ~/.config/glyphium/themes/night-owl
background = #011627
foreground = #d6deeb
cursor-color = #80a4c2
selection-background = #1d3b53
palette = 4=#82aaff
background-opacity = 0.9
```

**One colour on top of any theme:** add the same keys to `config.toml`, for example `background = #101014` or `palette = 1=#ff5f5f`.

<p align="center"><img src="docs/images/glass-light.webp" alt="Glass Light theme, picked automatically in light mode" width="800"></p>

### Settings

<p align="center"><img src="docs/images/settings.webp" alt="The Settings panel with rows for theme, font, size, line height, opacity, blur, padding, cursor and more" width="800"></p>

**Cmd+,** opens the Settings panel. Up and Down choose a row, Left and Right change it, and every change shows at once. **Done** (Enter) saves only what you changed to `~/.config/glyphium/config.toml`, keeping your comments; **Cancel** (Esc) puts everything back.

You can also edit the file in any editor: **Glyphium › Open Config File** creates it with every setting listed and commented out. Save it and switch back to the window, and the changes apply without a restart. A line Glyphium can't read shows a warning and keeps the default for that setting.

<details>
<summary>Every setting</summary>

| Key | Default | Values |
| --- | --- | --- |
| `font_family` | JetBrains Mono NL (bundled) | Any installed monospaced font. `SF Mono` is read from macOS's Terminal.app at run time if it isn't installed |
| `font_size` | `13` | Points, 4–96 |
| `line_height` | `1.2` | Times the font size, 0.8–3 |
| `theme` | `auto` | `auto`, `default`, a theme name, or a path to a theme file |
| `theme_light`, `theme_dark` | `glass-light`, `glass-dark` | What `auto` uses |
| `background`, `foreground`, `cursor_color`, `selection_background`, `palette = N=#hex` | from the theme | Colour overrides |
| `opacity` | from the theme (Glass: 0.80 / 0.84, others 1) | 0.3–1, macOS |
| `blur` | `true` | Blur what is behind a translucent window, macOS |
| `padding` | `8` | Points around the text in each pane |
| `window_columns`, `window_rows` | `100`, `30` | Size of a new window |
| `cursor_style` | `default` | `block`, `bar`, `underline` |
| `cursor_blink` | `default` | `true`, `false` |
| `option_as_meta` | `false` | macOS: Option sends Esc+key instead of special characters |
| `copy_on_select` | `false` | Copy a selection when the mouse is released |
| `confirm_paste` | `true` | Ask before a multi-line paste that may run commands |
| `scrollback` | `10000` | Lines per pane |
| `shell` | your login shell | Shell for new tabs: `zsh`, `bash`, `fish` or a path |
| `assist_hook` | `true` | Offer a fix when zsh or fish can't find a command |
| `assist_model` | `nl2bash` | `nl2bash`, `qwen-coder` or `k2-system1`, downloaded on first use |

Glyphium reads `--config FILE` first, then `$GLYPHIUM_CONFIG`, then `config.toml` in `$GLYPHIUM_CONFIG_DIR` or `~/.config/glyphium`. A program can still change the cursor shape; `cursor_style` is the shape a new pane starts with, and a program that resets the cursor gets Ghostty's default block.

</details>

### Window and menus

The title bar is drawn in the terminal's own colours behind the traffic lights, like the Terminal app's. With more than one tab, the tabs show as pills on a second row: click one to switch, hover for its close button, or use **+** for a new tab. Drag empty title bar space to move the window, and double-click it to zoom. The macOS menu bar has every action, with its shortcut, under Glyphium, Shell, Edit, View, Extras, Window and Help.

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
| Cmd+K | Ask for a command |
| Cmd+/ | Cheatsheet |
| Cmd+, | Settings |
| Cmd+click | Open a link |
| Cmd+Q | Quit |

</details>

## Tests

```sh
cargo test --release -p glyphium-terminal-core -p glyphium-terminal-render -p glyphium-session-host -p glyphium
cargo build --release -p glyphium && tests/e2e/run.sh OUT_DIR   # opens a real window per script
```

Each e2e script gets its own empty settings folder, so your own `~/.config/glyphium` is never read or written. A script whose first lines say `# shell: zsh` runs zsh with that folder as `HOME`, and `# assist-stub: REPLY` stands in for the model.

To check the real model, point a test at a downloaded GGUF:

```sh
GLYPHIUM_TEST_MODEL=~/.config/glyphium/models/qwen2.5-coder-0.5b-bash-gguf.gguf \
  cargo test --release -p glyphium real_model -- --ignored --nocapture
```

CI runs the first line and clippy on macOS for every push and pull request.

## Licence

See [LICENSE](LICENSE).
