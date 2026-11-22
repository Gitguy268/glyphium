//! Shell integration for the assistant: when zsh or fish can't find a command, it tells the
//! terminal with a private escape sequence (`OSC 7777 ; assist ; <base64 of the line>`), and
//! Glyphium offers a fix. The files are written to `shell/` in the settings folder at start-up.
//!
//! zsh gets them through `ZDOTDIR`: Glyphium's `.zshenv` puts the user's `ZDOTDIR` back, reads
//! their `.zshenv`, and defines the hook, so the user's own files then load as usual. fish gets
//! them through `XDG_DATA_DIRS` (a `vendor_conf.d` file), which is put back the same way. A
//! handler the user defines themselves wins in both. bash needs one line in `~/.bashrc`, since
//! macOS's bash 3.2 has no hook at all.

use std::path::{Path, PathBuf};

use glyphium_session_host::Profile;

const ZSHENV: &str = r#"# Written by Glyphium on every start. Puts your ZDOTDIR back, reads your .zshenv, then adds the
# command-not-found hook for the assistant. Your own .zprofile and .zshrc load as usual.
if [[ -n "${GLYPHIUM_ZDOTDIR+x}" ]]; then ZDOTDIR=$GLYPHIUM_ZDOTDIR; else unset ZDOTDIR; fi
unset GLYPHIUM_ZDOTDIR
[[ -r "${ZDOTDIR:-$HOME}/.zshenv" ]] && source "${ZDOTDIR:-$HOME}/.zshenv"
[[ -o interactive ]] && source "${${(%):-%x}:A:h:h}/glyphium.zsh"
"#;

const ZSH_HOOK: &str = r#"# Glyphium: tell the terminal about a command zsh can't find, so it can offer a fix.
if (( ! $+functions[command_not_found_handler] )); then
  command_not_found_handler() {
    [[ -t 2 ]] && printf '\e]7777;assist;%s\a' "$(printf '%s' "$*" | base64 | tr -d '\n')" >&2
    print -u2 "zsh: command not found: $1"
    return 127
  }
fi
"#;

const FISH_HOOK: &str = r#"# Written by Glyphium on every start. Puts XDG_DATA_DIRS back, then tells the terminal about
# commands fish can't find, so it can offer a fix.
if set -q GLYPHIUM_XDG_DATA_DIRS
    if test -n "$GLYPHIUM_XDG_DATA_DIRS"
        set --path -gx XDG_DATA_DIRS (string split : -- $GLYPHIUM_XDG_DATA_DIRS)
    else
        set -e XDG_DATA_DIRS
    end
    set -e GLYPHIUM_XDG_DATA_DIRS
end
if status is-interactive
    functions -q __glyphium_previous_cnf; or functions -c fish_command_not_found __glyphium_previous_cnf
    function fish_command_not_found
        isatty stderr; and printf '\e]7777;assist;%s\a' (string join ' ' -- $argv | base64 | string join '') >&2
        __glyphium_previous_cnf $argv
    end
end
"#;

const BASH_HOOK: &str = r#"# Glyphium: add `source ~/.config/glyphium/shell/glyphium.bash` to ~/.bashrc (bash 4 or newer)
# to get fix suggestions for commands bash can't find.
if [ -n "$BASH_VERSION" ] && ! declare -F command_not_found_handle >/dev/null; then
  command_not_found_handle() {
    [ -t 2 ] && [ "$TERM_PROGRAM" = Glyphium ] && printf '\033]7777;assist;%s\007' "$(printf '%s' "$*" | base64 | tr -d '\n')" >&2
    printf 'bash: %s: command not found\n' "$1" >&2
    return 127
  }
fi
"#;

/// Writes the integration files under `dir` (normally `<settings>/shell`). Returns `dir` if all
/// were written.
pub fn install(dir: &Path) -> Option<PathBuf> {
    let files: [(PathBuf, &str); 4] = [
        (dir.join("zsh/.zshenv"), ZSHENV),
        (dir.join("glyphium.zsh"), ZSH_HOOK),
        (dir.join("fish-data/fish/vendor_conf.d/glyphium.fish"), FISH_HOOK),
        (dir.join("glyphium.bash"), BASH_HOOK),
    ];
    for (path, text) in files {
        std::fs::create_dir_all(path.parent()?).ok()?;
        if std::fs::read_to_string(&path).ok().as_deref() != Some(text) {
            std::fs::write(&path, text).ok()?;
        }
    }
    Some(dir.to_path_buf())
}

/// The shell's name from a program path or login-shell argv[0]: `/bin/zsh` or `-zsh` give `zsh`.
pub fn shell_name(program: &str) -> &str {
    program.rsplit('/').next().unwrap_or(program).trim_start_matches('-')
}

/// Adds the environment that loads the hook for `program`, if it's zsh or fish. `env` reads the
/// current environment (a parameter for tests).
pub fn apply(profile: &mut Profile, program: &str, dir: &Path, env: &dyn Fn(&str) -> Option<String>) {
    match shell_name(program) {
        "zsh" => {
            if let Some(z) = env("ZDOTDIR") {
                profile.env.insert("GLYPHIUM_ZDOTDIR".into(), z);
            }
            profile.env.insert("ZDOTDIR".into(), dir.join("zsh").display().to_string());
        }
        "fish" => {
            let original = env("XDG_DATA_DIRS").unwrap_or_default();
            let base = if original.is_empty() { "/usr/local/share:/usr/share".to_string() } else { original.clone() };
            profile.env.insert("GLYPHIUM_XDG_DATA_DIRS".into(), original);
            profile.env.insert("XDG_DATA_DIRS".into(), format!("{}:{base}", dir.join("fish-data").display()));
        }
        _ => {}
    }
}

/// Finds a shell by name or path: a path must exist; a name is looked up on `PATH` and in
/// Homebrew's folders.
pub fn resolve(shell: &str) -> Option<String> {
    if shell.contains('/') {
        return Path::new(shell).exists().then(|| shell.to_string());
    }
    let path = std::env::var("PATH").unwrap_or_default();
    path.split(':').chain(["/opt/homebrew/bin", "/usr/local/bin", "/bin", "/usr/bin"]).map(|d| Path::new(d).join(shell)).find(|p| p.is_file()).map(|p| p.display().to_string())
}

/// True if `cmd` runs as a program on `PATH`, so a typo fix leaves it alone.
pub fn on_path(cmd: &str) -> bool {
    !cmd.contains('/') && std::env::var("PATH").unwrap_or_default().split(':').any(|d| Path::new(d).join(cmd).is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_from_paths_and_login_argv() {
        assert_eq!(shell_name("/bin/zsh"), "zsh");
        assert_eq!(shell_name("-zsh"), "zsh");
        assert_eq!(shell_name("/opt/homebrew/bin/fish"), "fish");
        assert_eq!(shell_name("bash"), "bash");
    }

    #[test]
    fn zsh_gets_zdotdir_and_keeps_the_users() {
        let dir = Path::new("/cfg/shell");
        let mut p = Profile::default();
        apply(&mut p, "/bin/zsh", dir, &|_| None);
        assert_eq!(p.env.get("ZDOTDIR").map(String::as_str), Some("/cfg/shell/zsh"));
        assert!(!p.env.contains_key("GLYPHIUM_ZDOTDIR"));
        let mut p = Profile::default();
        apply(&mut p, "zsh", dir, &|k| (k == "ZDOTDIR").then(|| "/home/me/.zsh".into()));
        assert_eq!(p.env.get("GLYPHIUM_ZDOTDIR").map(String::as_str), Some("/home/me/.zsh"));
    }

    #[test]
    fn fish_gets_a_data_dir_in_front() {
        let mut p = Profile::default();
        apply(&mut p, "/opt/homebrew/bin/fish", Path::new("/cfg/shell"), &|_| None);
        assert_eq!(p.env.get("XDG_DATA_DIRS").map(String::as_str), Some("/cfg/shell/fish-data:/usr/local/share:/usr/share"));
        assert_eq!(p.env.get("GLYPHIUM_XDG_DATA_DIRS").map(String::as_str), Some(""));
    }

    #[test]
    fn other_programs_are_untouched() {
        let mut p = Profile::default();
        apply(&mut p, "/bin/bash", Path::new("/x"), &|_| None);
        apply(&mut p, "/usr/bin/vim", Path::new("/x"), &|_| None);
        assert!(p.env.is_empty());
    }

    #[test]
    fn install_writes_every_file_once() {
        let dir = std::env::temp_dir().join(format!("glyphium-shell-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(install(&dir), Some(dir.clone()));
        assert!(dir.join("zsh/.zshenv").is_file());
        assert!(dir.join("fish-data/fish/vendor_conf.d/glyphium.fish").is_file());
        let before = std::fs::metadata(dir.join("glyphium.zsh")).unwrap().modified().unwrap();
        assert_eq!(install(&dir), Some(dir.clone()));
        assert_eq!(std::fs::metadata(dir.join("glyphium.zsh")).unwrap().modified().unwrap(), before, "unchanged files aren't rewritten");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The hook really runs in zsh: a missing command prints the OSC with the typed words.
    #[test]
    fn zsh_hook_emits_the_sequence() {
        if !Path::new("/bin/zsh").exists() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("glyphium-zsh-hook-{}", std::process::id()));
        let home = dir.join("home");
        std::fs::create_dir_all(&home).unwrap();
        install(&dir.join("shell")).unwrap();
        // `script` gives zsh a terminal, so `-t 2` holds and the shell is interactive.
        let out = std::process::Command::new("script")
            .args(if cfg!(target_os = "macos") { vec!["-q", "/dev/null", "/bin/zsh", "-i", "-c", "gti stauts"] } else { vec!["-qc", "/bin/zsh -i -c 'gti stauts'", "/dev/null"] })
            .env("ZDOTDIR", dir.join("shell/zsh"))
            .env("HOME", &home)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        let _ = std::fs::remove_dir_all(&dir);
        let mut scanner = crate::assist::OscScanner::default();
        assert_eq!(scanner.feed(text.as_bytes()), vec!["gti stauts".to_string()], "{text:?}");
    }
}
