//! The command assistant's logic: plain English or a mistyped command in, one command out, which
//! the user confirms before it runs. Answers come from a fixed recipe, then a typo fix, then a
//! small language model (see `assist_model`). Pure functions, tested without a window. The same
//! rules as TabShell's `src/assist-core.js`.

/// The package manager and wording for the system prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum System {
    Mac,
    Apt,
    Apk,
    Dnf,
    Pacman,
}

impl System {
    /// The system this app runs on.
    pub fn detect() -> Self {
        if cfg!(target_os = "macos") {
            return System::Mac;
        }
        let has = |p: &str| std::path::Path::new(p).exists();
        if has("/sbin/apk") {
            System::Apk
        } else if has("/usr/bin/dnf") {
            System::Dnf
        } else if has("/usr/bin/pacman") {
            System::Pacman
        } else {
            System::Apt
        }
    }

    pub fn manager(self) -> &'static str {
        match self {
            System::Mac => "brew",
            System::Apt => "apt",
            System::Apk => "apk",
            System::Dnf => "dnf",
            System::Pacman => "pacman",
        }
    }

    fn os(self) -> &'static str {
        match self {
            System::Mac => "macOS",
            System::Apt => "Debian or Ubuntu Linux",
            System::Apk => "Alpine Linux",
            System::Dnf => "Fedora Linux",
            System::Pacman => "Arch Linux",
        }
    }

    fn user(self) -> &'static str {
        match self {
            System::Mac => "a normal user; Homebrew may or may not be installed",
            _ => "a normal user with sudo",
        }
    }

    pub fn install(self, pkg: &str) -> String {
        match self {
            System::Mac => format!("brew install {pkg}"),
            System::Apt => format!("sudo apt install -y {pkg}"),
            System::Apk => format!("sudo apk add {pkg}"),
            System::Dnf => format!("sudo dnf install -y {pkg}"),
            System::Pacman => format!("sudo pacman -S --noconfirm {pkg}"),
        }
    }

    fn update(self) -> &'static str {
        match self {
            System::Mac => "brew update && brew upgrade",
            System::Apt => "sudo apt update && sudo apt upgrade -y",
            System::Apk => "sudo apk update && sudo apk upgrade",
            System::Dnf => "sudo dnf upgrade -y",
            System::Pacman => "sudo pacman -Syu",
        }
    }
}

/// A tool picked from k4m4/terminals-are-sexy (CC0-1.0). `pkg` is the Homebrew name; Linux
/// managers use `linux` when it differs.
pub struct Extra {
    pub name: &'static str,
    /// The Extras menu's submenu (macOS).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub group: &'static str,
    pub what: &'static str,
    /// The project's home page, for the README and NOTICE.
    #[allow(dead_code)]
    pub url: &'static str,
    pub bin: Option<&'static str>,
    pub brew: Option<&'static str>,
    pub linux: Option<&'static str>,
    pub script: Option<&'static str>,
    pub needs: &'static str,
}

const fn pkg(name: &'static str, group: &'static str, bin: &'static str, what: &'static str, url: &'static str) -> Extra {
    Extra { name, group, what, url, bin: Some(bin), brew: Some(name), linux: Some(name), script: None, needs: "" }
}

pub const OMF_INSTALL: &str = "curl -fsSL https://raw.githubusercontent.com/oh-my-fish/oh-my-fish/master/bin/install | fish";
const OMZ_INSTALL: &str = "sh -c \"$(curl -fsSL https://raw.githubusercontent.com/ohmyzsh/ohmyzsh/master/tools/install.sh)\"";
const FISHER_INSTALL: &str = "fish -c 'curl -sL https://raw.githubusercontent.com/jorgebucaran/fisher/main/functions/fisher.fish | source && fisher install jorgebucaran/fisher'";
pub const BREW_INSTALL: &str = "/bin/bash -c \"$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)\"";

pub const EXTRAS: &[Extra] = &[
    pkg("fish", "Shells", "fish", "Smart, friendly shell with suggestions as you type.", "https://fishshell.com"),
    pkg("zsh", "Shells", "zsh", "Powerful shell; the macOS default.", "https://www.zsh.org"),
    pkg("bash", "Shells", "bash", "GNU bash (newer than the one macOS ships).", "https://www.gnu.org/software/bash/"),
    Extra { name: "oh-my-fish", group: "Shells", what: "The fish shell framework: packages and themes through omf.", url: "https://github.com/oh-my-fish/oh-my-fish", bin: None, brew: None, linux: None, script: Some(OMF_INSTALL), needs: "fish" },
    Extra { name: "Fisher", group: "Shells", what: "Plugin manager for the fish shell.", url: "https://github.com/jorgebucaran/fisher", bin: None, brew: None, linux: None, script: Some(FISHER_INSTALL), needs: "fish" },
    Extra { name: "oh-my-zsh", group: "Shells", what: "Community framework for zsh settings, themes and plugins.", url: "https://github.com/ohmyzsh/ohmyzsh", bin: None, brew: None, linux: None, script: Some(OMZ_INSTALL), needs: "zsh, git and curl" },
    Extra { name: "zsh-autosuggestions", group: "Shells", what: "Fish-like suggestions for zsh as you type.", url: "https://github.com/zsh-users/zsh-autosuggestions", bin: None, brew: Some("zsh-autosuggestions"), linux: Some("zsh-autosuggestions"), script: None, needs: "" },
    Extra { name: "zsh-syntax-highlighting", group: "Shells", what: "Fish-like syntax highlighting for zsh.", url: "https://github.com/zsh-users/zsh-syntax-highlighting", bin: None, brew: Some("zsh-syntax-highlighting"), linux: Some("zsh-syntax-highlighting"), script: None, needs: "" },
    pkg("fzf", "Tools", "fzf", "Fuzzy finder for files, history and anything you pipe in.", "https://github.com/junegunn/fzf"),
    pkg("zoxide", "Tools", "zoxide", "A cd that remembers where you go: z proj jumps there.", "https://github.com/ajeetdsouza/zoxide"),
    pkg("tmux", "Tools", "tmux", "Terminal multiplexer: panes, windows, detachable sessions.", "https://github.com/tmux/tmux"),
    pkg("eza", "Tools", "eza", "A modern ls with colours and git status (successor of exa).", "https://github.com/eza-community/eza"),
    pkg("ripgrep", "Tools", "rg", "Very fast recursive search: rg pattern.", "https://github.com/BurntSushi/ripgrep"),
    Extra { name: "the_silver_searcher", group: "Tools", what: "Code search like ack, but faster: ag pattern.", url: "https://github.com/ggreer/the_silver_searcher", bin: Some("ag"), brew: Some("the_silver_searcher"), linux: Some("silversearcher-ag"), script: None, needs: "" },
    pkg("ranger", "Tools", "ranger", "Console file manager with vi keys.", "https://github.com/ranger/ranger"),
    pkg("nnn", "Tools", "nnn", "Tiny, very fast file manager.", "https://github.com/jarun/nnn"),
    pkg("micro", "Tools", "micro", "Easy terminal text editor with normal shortcuts.", "https://github.com/zyedidia/micro"),
    pkg("htop", "Tools", "htop", "Interactive process viewer.", "https://htop.dev"),
    pkg("glances", "Tools", "glances", "System monitor: CPU, memory, disks and network.", "https://github.com/nicolargo/glances"),
    pkg("tig", "Tools", "tig", "Text-mode interface for git.", "https://github.com/jonas/tig"),
    Extra { name: "tldr", group: "Tools", what: "Short, example-first man pages.", url: "https://github.com/tldr-pages/tldr", bin: Some("tldr"), brew: Some("tlrc"), linux: Some("tldr"), script: None, needs: "" },
    pkg("fastfetch", "Tools", "fastfetch", "System info with a logo, in the spirit of neofetch.", "https://github.com/fastfetch-cli/fastfetch"),
    pkg("asciinema", "Tools", "asciinema", "Record and share terminal sessions.", "https://github.com/asciinema/asciinema"),
    pkg("lolcat", "Tools", "lolcat", "Rainbows: echo hi | lolcat.", "https://github.com/busyloop/lolcat"),
];

impl Extra {
    /// The command that installs this on `sys`, if there is one.
    pub fn command(&self, sys: System) -> Option<String> {
        if let Some(s) = self.script {
            return Some(if self.needs == "fish" && sys == System::Mac && self.name == "oh-my-fish" { format!("brew install fish && {s}") } else { s.to_string() });
        }
        let p = if sys == System::Mac { self.brew } else { self.linux };
        p.map(|p| sys.install(p))
    }

    pub fn note(&self) -> String {
        if self.needs.is_empty() { self.what.to_string() } else { format!("{} Needs {}.", self.what, self.needs) }
    }
}

/// An answer to show for confirmation, or a polite "not here".
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    Command { cmd: String, note: String },
    No(String),
}

fn cmd(cmd: impl Into<String>, note: impl Into<String>) -> Option<Answer> {
    Some(Answer::Command { cmd: cmd.into(), note: note.into() })
}

// ---------- string helpers ----------

/// Damerau-Levenshtein distance: an adjacent swap is one edit, so "gti" is 1 from "git".
pub fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

/// Edits allowed before a word counts as a typo of another: none for very short words.
fn allowed(len: usize) -> usize {
    match len {
        0..=2 => 0,
        3..=7 => 1,
        _ => 2,
    }
}

/// The closest word in `vocab` within the allowed edits. Ties keep the earlier word.
pub fn nearest<'a>(word: &str, vocab: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let mut best = None;
    let mut best_d = usize::MAX;
    for v in vocab {
        let d = distance(word, v);
        if d < best_d && d <= allowed(word.chars().count().max(v.chars().count())) {
            best = Some(v);
            best_d = d;
        }
    }
    best
}

// ---------- recipes ----------

const FILLER: &[&str] = &["please ", "pls ", "can you ", "could you ", "how do i ", "how to ", "i want to ", "i'd like to ", "help me ", "let's ", "lets "];
const VOCAB: &[&str] = &["install", "update", "upgrade", "everything", "system", "packages", "switch", "change", "use", "start", "open", "shell", "homebrew", "brew", "oh", "my", "fish", "zsh", "bash", "fisher", "omf", "the", "to", "a", "get", "add", "setup", "set", "up"];

/// Lower case, filler words dropped, each word snapped to the recipe vocabulary when it's a near miss.
pub fn normalize(text: &str) -> String {
    let mut s: String = text.to_lowercase().chars().map(|c| if "?!.,".contains(c) { ' ' } else { c }).collect();
    s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    loop {
        let before = s.len();
        for f in FILLER {
            if let Some(rest) = s.strip_prefix(f) {
                s = rest.to_string();
            }
        }
        if s.len() == before {
            break;
        }
    }
    let names: Vec<String> = EXTRAS.iter().map(|e| e.name.to_lowercase()).collect();
    s.split(' ')
        .map(|w| if VOCAB.contains(&w) || names.iter().any(|n| n == w) { w.to_string() } else { nearest(w, VOCAB.iter().copied()).unwrap_or(w).to_string() })
        .collect::<Vec<_>>()
        .join(" ")
}

fn extra_by_word(word: &str) -> Option<&'static Extra> {
    EXTRAS.iter().find(|e| e.name.to_lowercase() == word || e.bin == Some(word)).or_else(|| EXTRAS.iter().find(|e| distance(&e.name.to_lowercase(), word) <= allowed(word.chars().count())))
}

/// A fixed answer for requests a small model gets wrong or that need an exact URL.
pub fn recipe(text: &str, sys: System) -> Option<Answer> {
    let s = normalize(text);
    let words: Vec<&str> = s.split(' ').collect();
    let install = matches!(words.first(), Some(&("install" | "get" | "add" | "setup"))) || s.starts_with("set up ");
    let what = if install {
        let rest = s.strip_prefix("set up ").unwrap_or_else(|| s.split_once(' ').map_or("", |(_, r)| r));
        Some(rest.trim_start_matches("the ").trim_start_matches("a ").to_string())
    } else {
        None
    };
    if let Some(w) = &what {
        match w.as_str() {
            "brew" | "homebrew" | "home brew" => {
                return if sys == System::Mac || sys == System::Apt {
                    cmd(BREW_INSTALL, "Homebrew's official installer. Follow the lines it prints at the end to add brew to your PATH.")
                } else {
                    cmd(BREW_INSTALL, "Homebrew's official installer. Homebrew supports macOS and 64-bit Linux.")
                };
            }
            "oh my fish" | "oh-my-fish" | "omf" => {
                let c = if sys == System::Mac { format!("brew install fish && {OMF_INSTALL}") } else { OMF_INSTALL.to_string() };
                return cmd(c, "oh-my-fish's installer. It needs fish.");
            }
            "oh my zsh" | "oh-my-zsh" | "ohmyzsh" => return cmd(OMZ_INSTALL, "The oh-my-zsh installer. It needs zsh, git and curl."),
            "fisher" => return cmd(FISHER_INSTALL, "Installs Fisher inside fish."),
            _ => {}
        }
        if !w.is_empty() && w.chars().all(|c| c.is_alphanumeric() || "._+-".contains(c))
            && let Some(e) = extra_by_word(w)
        {
            return match e.command(sys) {
                Some(c) => cmd(c, e.note()),
                None => Some(Answer::No(format!("{} isn't packaged for {}.", e.name, sys.manager()))),
            };
        }
    }
    let update = matches!(words.first(), Some(&("update" | "upgrade")));
    if update && (words.len() == 1 || words[1..].iter().all(|w| matches!(*w, "the" | "everything" | "system" | "packages" | "and" | "&" | "update" | "upgrade"))) {
        return cmd(sys.update(), "");
    }
    if matches!(words.first(), Some(&("switch" | "change" | "use" | "start" | "open"))) {
        let rest: Vec<&str> = words[1..].iter().copied().filter(|w| !matches!(*w, "to" | "the" | "a" | "shell")).collect();
        if let [sh @ ("zsh" | "fish" | "bash")] = rest.as_slice() {
            return cmd(format!("exec {sh} -l"), "Starts it in this pane. Settings → Shell makes it the default for new tabs.");
        }
    }
    None
}

// ---------- typo fixes ----------

const STOPWORDS: &[&str] = &["the", "a", "an", "in", "of", "to", "my", "this", "that", "all", "for", "with", "called", "and", "from", "on", "is", "are", "what", "which", "how", "me", "every", "into", "named"];

/// True for plain English ("count the lines in notes.txt"), which goes to the model instead.
pub fn looks_like_sentence(text: &str) -> bool {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    words.len() >= 3 && words.iter().any(|w| STOPWORDS.contains(w))
}

const COMMANDS: &[&str] = &[
    "ls", "cd", "pwd", "cat", "less", "more", "head", "tail", "grep", "find", "echo", "touch", "mkdir", "rmdir", "rm", "cp", "mv", "ln", "chmod", "chown", "clear", "exit", "history", "man", "which", "whoami", "uname", "date", "top", "ps", "kill", "killall", "df", "du", "free", "tar", "gzip", "gunzip", "zip", "unzip", "curl", "wget", "ssh", "scp", "ping", "git", "nano", "vi", "vim", "python3", "python", "pip", "pip3", "node", "npm", "npx", "make", "sudo", "apt", "apt-get", "apk", "brew", "dnf", "pacman", "docker", "container", "podman", "sort", "uniq", "wc", "diff", "sed", "awk", "xargs", "env", "export", "source", "alias", "sh", "bash", "zsh", "fish", "omf", "tree", "tee", "ip", "jq", "file", "stat", "watch", "nc", "dig", "host", "mount", "umount", "lsof", "crontab", "passwd", "su", "id", "hostname", "tr", "cut", "paste", "seq", "sleep", "time", "bc", "base64", "openssl", "rsync", "screen", "emacs", "code", "go", "cargo", "rustc", "rustup", "java", "ruby", "perl", "php", "gcc", "clang", "test", "true", "false", "type", "command", "jobs", "fg", "bg", "exec", "printf", "read", "set", "unset", "open", "say", "pbcopy", "pbpaste", "defaults", "softwareupdate", "xcode-select", "mas", "caffeinate", "diskutil", "mdfind", "networksetup", "glyphium",
];

fn subcommands(cmd: &str) -> &'static [&'static str] {
    match cmd {
        "git" => &["status", "add", "commit", "push", "pull", "clone", "checkout", "switch", "branch", "merge", "rebase", "log", "diff", "init", "fetch", "stash", "reset", "remote", "tag", "restore", "show"],
        "apt" => &["install", "remove", "update", "upgrade", "search", "show", "list", "purge", "autoremove"],
        "apt-get" => &["install", "remove", "update", "upgrade", "purge", "autoremove"],
        "apk" => &["add", "del", "update", "upgrade", "search", "info", "fix"],
        "brew" => &["install", "uninstall", "update", "upgrade", "search", "info", "list", "doctor", "cleanup", "tap"],
        "npm" => &["install", "uninstall", "run", "test", "start", "init", "update", "ci", "publish", "list"],
        "docker" => &["run", "ps", "images", "pull", "build", "exec", "stop", "rm", "rmi", "logs", "compose"],
        "pip" | "pip3" => &["install", "uninstall", "list", "show", "freeze"],
        "cargo" => &["build", "run", "test", "check", "clippy", "fmt", "add", "install", "update", "new", "init"],
        _ => &[],
    }
}

/// Fixes a mistyped command name, or a mistyped subcommand of a few well-known tools: "gti stauts"
/// becomes "git status". `exists` says whether a command is installed, so real ones are left alone.
pub fn fix_typos(line: &str, exists: &dyn Fn(&str) -> bool) -> Option<String> {
    if looks_like_sentence(line) {
        return None;
    }
    let trimmed = line.trim();
    let (lead, rest) = match trimmed.strip_prefix("sudo ") {
        Some(r) => ("sudo ", r.trim_start()),
        None => ("", trimmed),
    };
    let (first, mut tail) = match rest.split_once(char::is_whitespace) {
        Some((f, t)) => (f.to_string(), format!(" {}", t.trim_start())),
        None => (rest.to_string(), String::new()),
    };
    if first.is_empty() {
        return None;
    }
    let extra_bins = EXTRAS.iter().filter_map(|e| e.bin);
    let mut cmd = first.clone();
    let mut changed = false;
    if !COMMANDS.contains(&first.as_str()) && !EXTRAS.iter().any(|e| e.bin == Some(first.as_str())) && !exists(&first) {
        cmd = nearest(&first, COMMANDS.iter().copied().chain(extra_bins))?.to_string();
        changed = true;
    }
    let subs = subcommands(&cmd);
    if !subs.is_empty() {
        let t = tail.trim_start();
        let (sub, after) = t.split_once(char::is_whitespace).map_or((t, ""), |(s, a)| (s, a));
        if !sub.is_empty() && !sub.starts_with('-') && !subs.contains(&sub)
            && let Some(fix) = nearest(sub, subs.iter().copied())
        {
            tail = if after.is_empty() { format!(" {fix}") } else { format!(" {fix} {after}") };
            changed = true;
        }
    }
    changed.then(|| format!("{lead}{cmd}{tail}"))
}

/// A recipe, or a typo fix, or `None` when the model has to answer.
pub fn quick_answer(text: &str, sys: System, exists: &dyn Fn(&str) -> bool) -> Option<Answer> {
    recipe(text, sys).or_else(|| fix_typos(text, exists).map(|c| Answer::Command { cmd: c, note: "Spelling fixed.".into() }))
}

// ---------- the model ----------

/// Chat messages for the model: system prompt, a few worked examples, then the request.
pub fn messages(input: &str, sys: System, shell: &str) -> Vec<(&'static str, String)> {
    let system = format!(
        "You turn a request or a mistyped command into one shell command.\nSystem: {}. User: {}. Shell: {shell}. Packages: {}.\nReply with exactly one line: CMD: <command>\nIf it can't be done on this system, reply with one line: NO: <short reason>\nFix spelling mistakes. Never explain. Never use markdown.",
        sys.os(),
        sys.user(),
        sys.manager()
    );
    let python = if sys == System::Mac { "python" } else { "python3" };
    let shots = [
        ("show the files in this folder with their sizes", "CMD: ls -lh".to_string()),
        ("gti stauts", "CMD: git status".to_string()),
        ("install python", format!("CMD: {}", sys.install(python))),
        ("find every .log file under this folder", "CMD: find . -name '*.log'".to_string()),
    ];
    let mut out = vec![("system", system)];
    for (u, a) in shots {
        out.push(("user", u.to_string()));
        out.push(("assistant", a));
    }
    out.push(("user", input.to_string()));
    out
}

/// Reads a model reply: `CMD: x`, `NO: reason`, or a bare command on the first line.
pub fn parse_reply(text: &str) -> Option<Answer> {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with("```"))?;
    if let Some(rest) = line.strip_prefix("NO:").or_else(|| line.strip_prefix("No:")) {
        return Some(Answer::No(rest.trim().to_string()));
    }
    let mut c = line.strip_prefix("CMD:").or_else(|| line.strip_prefix("cmd:")).unwrap_or(line).trim();
    c = c.trim_matches('`');
    c = c.strip_prefix("$ ").unwrap_or(c).trim();
    // Small models like placeholder folders; the current folder is nearly always what was meant.
    let c: Vec<String> = c
        .split(' ')
        .map(|w| match w.trim_end_matches('/') {
            "/path/to/directory" | "/path/to/folder" | "/path/to/dir" => ".".to_string(),
            _ => w.to_string(),
        })
        .collect();
    let c = c.join(" ");
    if c.is_empty() || c.len() > 500 {
        return None;
    }
    Some(Answer::Command { cmd: c, note: "Suggested by the model. Check it before running.".into() })
}

// ---------- safety ----------

/// Plain-English warnings for risky parts of a command; empty when nothing stands out.
pub fn dangers(cmd: &str) -> Vec<&'static str> {
    let words: Vec<&str> = cmd.split(|c: char| c.is_whitespace() || c == ';' || c == '&' || c == '|').filter(|w| !w.is_empty()).collect();
    let has = |w: &str| words.contains(&w);
    let mut out = vec![];
    let mut add = |cond: bool, why: &'static str| {
        if cond && !out.contains(&why) {
            out.push(why);
        }
    };
    add(has("sudo"), "runs as administrator (sudo)");
    let rm_flags = cmd.split_whitespace().collect::<Vec<_>>().windows(2).any(|w| w[0] == "rm" && (w[1].starts_with('-') && w[1][1..].contains(['r', 'R', 'f']) || w[1] == "--recursive" || w[1] == "--force"));
    add(rm_flags, "deletes files without asking (rm -r / -f)");
    add(has("dd") && cmd.contains("of="), "writes raw data to a disk or file (dd)");
    add(words.iter().any(|w| *w == "mkfs" || w.starts_with("mkfs.")), "formats a disk (mkfs)");
    let piped_to_shell = cmd.split('|').skip(1).any(|part| matches!(part.split_whitespace().find(|w| *w != "sudo"), Some("sh" | "bash" | "zsh" | "fish" | "python" | "python3")));
    add((cmd.contains("curl") || cmd.contains("wget")) && (piped_to_shell || cmd.contains("$(curl") || cmd.contains("$(wget")), "runs a script straight from the internet");
    add(cmd.contains(">/dev/sd") || cmd.contains("> /dev/sd") || cmd.contains("> /dev/disk") || cmd.contains(">/dev/disk") || cmd.contains("> /dev/nvme"), "writes directly to a disk device");
    add(cmd.contains("chmod") && (has("777") || has("0777")), "makes files writable by everyone (chmod 777)");
    add(cmd.contains(":(){") || cmd.contains(":() {"), "a fork bomb: it freezes the machine");
    add(has("shutdown") || has("reboot") || has("halt") || has("poweroff"), "turns off or restarts the machine");
    add((has("kill") || has("killall")) && has("-9"), "force-kills processes");
    out
}

// ---------- the shell hook ----------

/// Finds `OSC 7777 ; assist ; <base64> BEL|ST` in shell output, across chunk boundaries, and
/// returns the decoded command lines. The bytes still go to the terminal, which ignores them.
#[derive(Default)]
pub struct OscScanner {
    buf: Vec<u8>,
    inside: bool,
    esc: bool,
}

const OSC_START: &[u8] = b"\x1b]7777;";
const MAX_OSC: usize = 4096;

impl OscScanner {
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<String> {
        let mut found = vec![];
        for &b in bytes {
            if !self.inside {
                // Track a possible start sequence in `buf`.
                if b == OSC_START[self.buf.len()] {
                    self.buf.push(b);
                    if self.buf.len() == OSC_START.len() {
                        self.buf.clear();
                        self.inside = true;
                    }
                } else {
                    self.buf.clear();
                    if b == OSC_START[0] {
                        self.buf.push(b);
                    }
                }
                continue;
            }
            let end = b == 0x07 || (self.esc && b == b'\\');
            if end {
                if self.esc {
                    self.buf.pop();
                }
                if let Some(line) = parse_hook(&self.buf) {
                    found.push(line);
                }
                self.buf.clear();
                self.inside = false;
                self.esc = false;
                continue;
            }
            self.esc = b == 0x1b;
            self.buf.push(b);
            if self.buf.len() > MAX_OSC {
                self.buf.clear();
                self.inside = false;
            }
        }
        found
    }
}

/// `assist;<base64 of the typed line>` to the line.
pub fn parse_hook(payload: &[u8]) -> Option<String> {
    let b64 = payload.strip_prefix(b"assist;")?;
    let bytes = base64_decode(b64)?;
    let line = String::from_utf8(bytes).ok()?;
    let line = line.trim();
    (!line.is_empty() && line.len() <= 1000).then(|| line.to_string())
}

fn base64_decode(s: &[u8]) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let s: Vec<u8> = s.iter().copied().filter(|c| !c.is_ascii_whitespace()).collect();
    let body = s.strip_suffix(b"==").or_else(|| s.strip_suffix(b"=")).unwrap_or(&s);
    let mut out = Vec::with_capacity(body.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0;
    for &c in body {
        acc = (acc << 6) | val(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    (!out.is_empty() || body.is_empty()).then_some(out)
}

// ---------- the cheatsheet ----------

/// The Help → Cheatsheet overlay. Original text; the idea of a short list of the most used
/// commands and shortcuts comes from 0nn0/terminal-mac-cheatsheet (no licence, nothing copied).
pub fn cheatsheet() -> Vec<(&'static str, Vec<(String, &'static str)>)> {
    let mac = cfg!(target_os = "macos");
    let (k, ks) = if mac { ("⌘", "⇧⌘") } else { ("Ctrl+Shift+", "Ctrl+Shift+") };
    let glyphium = vec![
        (format!("{k}K"), "ask for a command in plain English"),
        (format!("{ks}⏎"), "queue a command for after this one"),
        (format!("{k}T  /  {k}W"), "new tab  /  close"),
        (format!("{k}D"), "split right (and down with Shift)"),
        (format!("{k}[  {k}]"), "previous / next tab"),
        (format!("{k}+  {k}−  {k}0"), "bigger / smaller / actual size"),
        (format!("{k},"), "settings, including the shell for new tabs"),
    ];
    let own = |rows: &[(&str, &'static str)]| rows.iter().map(|(a, b)| (a.to_string(), *b)).collect::<Vec<_>>();
    let mut groups = vec![
        ("Glyphium", glyphium),
        ("The command line", own(&[
            ("Ctrl+A / Ctrl+E", "start / end of the line"),
            ("Ctrl+U / Ctrl+K", "delete to the start / to the end"),
            ("Ctrl+W", "delete the word before the cursor"),
            ("Ctrl+R", "search earlier commands"),
            ("Ctrl+C", "stop the running program"),
            ("Ctrl+L", "clear the screen"),
            ("Tab", "complete a name"),
            ("!!", "the last command again (sudo !!)"),
        ])),
        ("Files", own(&[
            ("cd -", "back to the previous folder"),
            ("ls -la", "everything here, hidden files too"),
            ("mkdir -p a/b", "make folders, parents too"),
            ("cmd > file", "save the output; >> adds to the end"),
            ("cmd1 | cmd2", "feed one program's output to the next"),
        ])),
    ];
    if mac {
        groups.push(("macOS", own(&[
            ("open .", "this folder in Finder (open file opens a file)"),
            ("pbcopy < file", "copy a file's text; pbpaste pastes"),
            ("caffeinate", "keep the Mac awake until Ctrl+C"),
            ("say hello", "speak text aloud"),
            ("mdfind name", "search with Spotlight"),
        ])));
    }
    groups
}

/// Models the assistant can download, best first. Scores from TabShell's
/// `scripts/assist-eval.mjs` (42 cases, with recipes and typo fixes in front).
pub struct ModelInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub repo: &'static str,
    pub file: &'static str,
    pub url: &'static str,
    pub size: u64,
}

pub const MODELS: &[ModelInfo] = &[
    ModelInfo {
        id: "nl2bash",
        label: "Qwen2.5-Coder 0.5B NL2Bash",
        repo: "burhan2753y/qwen2.5-coder-0.5b-nl2bash-gguf",
        file: "qwen2.5-coder-0.5b-bash-gguf.gguf",
        url: "https://huggingface.co/burhan2753y/qwen2.5-coder-0.5b-nl2bash-gguf/resolve/e90ecc62835f8def136688757dfbd8e5a5a667fa/qwen2.5-coder-0.5b-bash-gguf.gguf",
        size: 531_067_680,
    },
    ModelInfo {
        id: "qwen-coder",
        label: "Qwen2.5-Coder 0.5B Instruct",
        repo: "Qwen/Qwen2.5-Coder-0.5B-Instruct-GGUF",
        file: "qwen2.5-coder-0.5b-instruct-q4_k_m.gguf",
        url: "https://huggingface.co/Qwen/Qwen2.5-Coder-0.5B-Instruct-GGUF/resolve/ebb2015119c907b064c512bf053e945850b5875f/qwen2.5-coder-0.5b-instruct-q4_k_m.gguf",
        size: 491_400_064,
    },
    ModelInfo {
        id: "k2-system1",
        label: "K2-Horizon 0.9B System 1",
        repo: "Lucasgitguy-co/K2-Horizon-0.9B-System1-GGUF",
        file: "K2-Horizon-0.9B-System1-Q4_K_M.gguf",
        url: "https://huggingface.co/Lucasgitguy-co/K2-Horizon-0.9B-System1-GGUF/resolve/f367c4d90a942e9f092e390650de76fc37bf2426/K2-Horizon-0.9B-System1-Q4_K_M.gguf",
        size: 666_194_336,
    },
];

pub fn model(id: &str) -> &'static ModelInfo {
    MODELS.iter().find(|m| m.id == id).unwrap_or(&MODELS[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAC: System = System::Mac;
    const APT: System = System::Apt;
    fn none(_: &str) -> bool {
        false
    }

    fn command(a: Option<Answer>) -> String {
        match a {
            Some(Answer::Command { cmd, .. }) => cmd,
            other => panic!("expected a command, got {other:?}"),
        }
    }

    #[test]
    fn distance_counts_a_swap_as_one() {
        assert_eq!(distance("gti", "git"), 1);
        assert_eq!(distance("stauts", "status"), 1);
        assert_eq!(distance("brew", "brew"), 0);
    }

    #[test]
    fn install_brew() {
        assert!(command(recipe("install brew", MAC)).contains("Homebrew/install/HEAD/install.sh"));
        assert!(command(recipe("Please instal homebrew?", MAC)).contains("Homebrew"));
        assert!(command(recipe("how do i install brew", APT)).contains("Homebrew"));
    }

    #[test]
    fn install_recipes_use_the_package_manager_and_names() {
        assert_eq!(command(recipe("install ripgrep", MAC)), "brew install ripgrep");
        assert_eq!(command(recipe("installl htop", APT)), "sudo apt install -y htop");
        assert_eq!(command(recipe("install the_silver_searcher", APT)), "sudo apt install -y silversearcher-ag");
        assert_eq!(command(recipe("install tldr", MAC)), "brew install tlrc");
        assert_eq!(command(recipe("install fish", MAC)), "brew install fish");
        assert_eq!(command(recipe("install fish", System::Pacman)), "sudo pacman -S --noconfirm fish");
    }

    #[test]
    fn shell_and_framework_recipes() {
        assert_eq!(command(recipe("use zsh", MAC)), "exec zsh -l");
        assert_eq!(command(recipe("switch to the fish shell", MAC)), "exec fish -l");
        assert_eq!(command(recipe("install oh my fish", MAC)), format!("brew install fish && {OMF_INSTALL}"));
        assert_eq!(command(recipe("install oh-my-fish", APT)), OMF_INSTALL);
        assert!(command(recipe("install oh my zsh", MAC)).contains("ohmyzsh"));
        assert_eq!(command(recipe("update everything", MAC)), "brew update && brew upgrade");
        assert_eq!(command(recipe("upgrade", APT)), "sudo apt update && sudo apt upgrade -y");
    }

    #[test]
    fn unknown_requests_go_to_the_model() {
        assert_eq!(recipe("find files bigger than 100MB", MAC), None);
        assert_eq!(quick_answer("count the lines in notes.txt", MAC, &none), None);
        assert_eq!(quick_answer("show me the files", MAC, &none), None);
    }

    #[test]
    fn typo_fixes() {
        assert_eq!(fix_typos("gti status", &none).as_deref(), Some("git status"));
        assert_eq!(fix_typos("git stauts", &none).as_deref(), Some("git status"));
        assert_eq!(fix_typos("gti psuh origin main", &none).as_deref(), Some("git push origin main"));
        assert_eq!(fix_typos("sudo apt-get instal curl", &none).as_deref(), Some("sudo apt-get install curl"));
        assert_eq!(fix_typos("lss -la", &none).as_deref(), Some("ls -la"));
        assert_eq!(fix_typos("brwe install fish", &none).as_deref(), Some("brew install fish"));
        assert_eq!(fix_typos("ls -la", &none), None);
        assert_eq!(fix_typos("git -C dir status", &none), None);
        assert_eq!(fix_typos("xq", &none), None);
        assert_eq!(fix_typos("mytool --help", &|c| c == "mytool"), None, "installed commands stay");
        assert_eq!(fix_typos("gti status", &|c| c == "gti"), None, "even when one looks like a typo");
    }

    #[test]
    fn sentences_skip_the_typo_fixer() {
        assert!(looks_like_sentence("count the lines in notes.txt"));
        assert!(!looks_like_sentence("gti status"));
    }

    #[test]
    fn model_replies() {
        let c = |t: &str| match parse_reply(t) {
            Some(Answer::Command { cmd, .. }) => Some(cmd),
            _ => None,
        };
        assert_eq!(c("CMD: ls -lh\n").as_deref(), Some("ls -lh"));
        assert_eq!(c("```\n$ df -h\n```").as_deref(), Some("df -h"));
        assert_eq!(c("`du -sh .`").as_deref(), Some("du -sh ."));
        assert_eq!(c("CMD: du -sh /path/to/directory").as_deref(), Some("du -sh ."));
        assert_eq!(c("grep -r TODO /path/to/folder/ | less").as_deref(), Some("grep -r TODO . | less"));
        assert_eq!(parse_reply("NO: Homebrew can't run here"), Some(Answer::No("Homebrew can't run here".into())));
        assert_eq!(parse_reply(""), None);
        assert_eq!(parse_reply(&"x".repeat(600)), None);
    }

    #[test]
    fn messages_name_the_system_and_end_with_the_request() {
        let m = messages("install python", MAC, "zsh");
        assert_eq!(m[0].0, "system");
        assert!(m[0].1.contains("macOS") && m[0].1.contains("Shell: zsh") && m[0].1.contains("brew"));
        assert_eq!(m.last().unwrap(), &("user", "install python".to_string()));
        assert!(m.iter().any(|(_, c)| c == "CMD: brew install python"));
        assert!(messages("x", APT, "bash").iter().any(|(_, c)| c == "CMD: sudo apt install -y python3"));
    }

    #[test]
    fn danger_warnings() {
        assert!(dangers("ls -la").is_empty());
        assert!(dangers("sudo apt install htop").iter().any(|w| w.contains("sudo")));
        assert!(dangers("rm -rf build").iter().any(|w| w.contains("rm")));
        assert!(!dangers("rm --recursive x").is_empty());
        assert!(!dangers("dd if=/dev/zero of=/dev/disk2").is_empty());
        assert!(!dangers("mkfs.ext4 /dev/sdb1").is_empty());
        assert!(dangers("curl -fsSL https://x.sh | sh").iter().any(|w| w.contains("internet")));
        assert!(dangers(BREW_INSTALL).iter().any(|w| w.contains("internet")));
        assert!(dangers(OMF_INSTALL).iter().any(|w| w.contains("internet")));
        assert!(!dangers("echo hi > /dev/disk2").is_empty());
        assert!(!dangers("chmod -R 777 /").is_empty());
        assert!(dangers(":(){ :|:& };:").iter().any(|w| w.contains("fork")));
        assert!(dangers("rm notes.txt").is_empty(), "a plain rm of one file is normal");
    }

    fn osc(line: &str) -> Vec<u8> {
        let b64 = {
            const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut out = String::new();
            for chunk in line.as_bytes().chunks(3) {
                let n = chunk.iter().enumerate().fold(0u32, |a, (i, &b)| a | (b as u32) << (16 - 8 * i));
                for i in 0..4 {
                    out.push(if i <= chunk.len() { T[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
                }
            }
            out
        };
        format!("\x1b]7777;assist;{b64}\x07").into_bytes()
    }

    #[test]
    fn osc_scanner_finds_hooks_across_chunks() {
        let mut s = OscScanner::default();
        let mut data = b"prompt% ".to_vec();
        data.extend(osc("gti status"));
        data.extend(b"zsh: command not found: gti\r\n");
        assert_eq!(s.feed(&data), vec!["gti status".to_string()]);
        // Byte by byte, and with ST instead of BEL.
        let mut st = osc("brwe install fish\n");
        st.pop();
        st.extend(b"\x1b\\");
        let mut got = vec![];
        for b in st {
            got.extend(s.feed(&[b]));
        }
        assert_eq!(got, vec!["brwe install fish".to_string()]);
        // Other OSC sequences and junk are ignored.
        assert!(s.feed(b"\x1b]0;title\x07\x1b]7777;ready;zsh\x07\x1b]7777;assist;!!!\x07").is_empty());
        assert_eq!(s.feed(&osc("café")), vec!["café".to_string()]);
    }

    #[test]
    fn every_extra_installs_somehow_and_models_are_pinned() {
        for e in EXTRAS {
            assert!(e.command(MAC).is_some() || e.command(APT).is_some(), "{}", e.name);
            assert!(e.url.starts_with("https://"), "{}", e.name);
        }
        for m in MODELS {
            assert!(m.url.contains("/resolve/") && m.url.ends_with(m.file), "{}", m.id);
        }
        assert_eq!(model("nope").id, "nl2bash");
    }

    #[test]
    fn cheatsheet_has_the_ask_shortcut() {
        let groups = cheatsheet();
        assert!(groups[0].1.iter().any(|(_, what)| what.contains("plain English")));
    }
}
