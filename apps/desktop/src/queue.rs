//! Commands waiting to run in a pane once the shell is back at its prompt.
//!
//! "Back at the prompt" means the shell owns the terminal again (the PTY's foreground process
//! group is the shell's). One command is sent per completion: after a send, the next waits
//! until that command was seen running and the shell is idle again, or, for builtins like `cd`
//! that never leave the shell, until output arrived and has been quiet for a moment. So a
//! command that reads input, such as `head` or `ssh`, can never swallow the next one.
//!
//! The command and its Enter go as two writes, Enter once the shell has echoed the text. Bash
//! 3.2's line editor redraws the line after Enter when both arrive together, which leaves the
//! command's output glued to a second copy of the command.
//!
//! Known limit: a builtin waiting for input (`read`) looks idle.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How long output must be quiet before a command that never left the shell counts as done.
pub const QUIET: Duration = Duration::from_millis(150);
/// How often waiting panes check the shell while nothing else wakes the app.
pub const POLL: Duration = Duration::from_millis(100);
/// Longest wait for the echo of a typed command before pressing Enter anyway.
const ECHO: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Ready,
    /// Command text written, Enter not yet.
    Typed { at: Instant, echoed: bool },
    Sent { saw_busy: bool, output_since: Option<Instant> },
}

#[derive(Debug)]
pub struct CommandQueue {
    items: VecDeque<String>,
    state: State,
}

impl Default for CommandQueue {
    fn default() -> Self {
        Self { items: VecDeque::new(), state: State::Ready }
    }
}

impl CommandQueue {
    pub fn push(&mut self, command: String) {
        self.items.push_back(command);
    }

    /// Removes the most recently queued command.
    pub fn pop_last(&mut self) -> Option<String> {
        self.items.pop_back()
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn items(&self) -> impl Iterator<Item = &str> {
        self.items.iter().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// True while there is anything to watch: commands waiting, or one sent and not yet done.
    pub fn active(&self) -> bool {
        !self.items.is_empty() || self.state != State::Ready
    }

    /// Output from the pane arrived at `now`.
    pub fn saw_output(&mut self, now: Instant) {
        match &mut self.state {
            State::Sent { output_since, .. } => *output_since = Some(now),
            State::Typed { echoed, .. } => *echoed = true,
            State::Ready => {}
        }
    }

    /// When a waiting Enter is due at the latest, so the caller can wake for it.
    pub fn enter_due(&self) -> Option<Instant> {
        match self.state {
            State::Typed { at, .. } => Some(at + ECHO),
            _ => None,
        }
    }

    /// Advances with the shell's current state; returns the bytes to type, if a command is due.
    /// `idle` is `None` when the shell's state can't be read, and then nothing is ever sent.
    pub fn poll(&mut self, idle: Option<bool>, now: Instant) -> Option<Vec<u8>> {
        let idle = idle?;
        if let State::Typed { at, echoed } = self.state {
            if !echoed && now.duration_since(at) < ECHO {
                return None;
            }
            self.state = State::Sent { saw_busy: false, output_since: None };
            return Some(b"\r".to_vec());
        }
        if let State::Sent { saw_busy, output_since } = &mut self.state {
            if !idle {
                *saw_busy = true;
                return None;
            }
            let quiet = output_since.is_some_and(|t| now.duration_since(t) >= QUIET);
            if !*saw_busy && !quiet {
                return None;
            }
            self.state = State::Ready;
        }
        if !idle {
            return None;
        }
        let command = self.items.pop_front()?;
        self.state = State::Typed { at: now, echoed: false };
        Some(command.into_bytes())
    }
}

/// Shells the queue can watch: their own process owns the terminal only while at the prompt.
pub fn is_shell(program: &str) -> bool {
    let name = program.rsplit('/').next().unwrap_or(program).trim_start_matches('-');
    matches!(name, "bash" | "zsh" | "fish" | "sh" | "dash" | "ksh" | "mksh" | "tcsh" | "csh" | "nu" | "elvish" | "xonsh")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Types the next command and presses Enter after its echo; returns the text typed.
    fn send(q: &mut CommandQueue, t: Instant) -> Option<Vec<u8>> {
        let text = q.poll(Some(true), t)?;
        assert_eq!(q.poll(Some(true), t), None, "Enter waits for the echo");
        q.saw_output(t);
        assert_eq!(q.poll(Some(true), t), Some(b"\r".to_vec()));
        Some(text)
    }

    fn queue(items: &[&str]) -> CommandQueue {
        let mut q = CommandQueue::default();
        for i in items {
            q.push(i.to_string());
        }
        q
    }

    #[test]
    fn idle_shell_runs_the_first_command_at_once() {
        let mut q = queue(&["echo hi"]);
        assert_eq!(send(&mut q, Instant::now()), Some(b"echo hi".to_vec()));
        assert_eq!(q.len(), 0);
        assert!(q.active(), "still watching the sent command");
    }

    #[test]
    fn busy_shell_waits() {
        let mut q = queue(&["make"]);
        assert_eq!(q.poll(Some(false), Instant::now()), None);
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn next_waits_for_busy_then_idle() {
        let t = Instant::now();
        let mut q = queue(&["head -n1", "echo second"]);
        assert!(send(&mut q, t).is_some());
        // The shell has not forked yet: still idle, no output. Must not send the next one.
        assert_eq!(q.poll(Some(true), t + Duration::from_millis(5)), None);
        assert_eq!(q.poll(Some(false), t + Duration::from_millis(10)), None);
        // head is waiting on input for a long time: still busy.
        assert_eq!(q.poll(Some(false), t + Duration::from_secs(30)), None);
        assert_eq!(q.poll(Some(true), t + Duration::from_secs(31)), Some(b"echo second".to_vec()));
    }

    #[test]
    fn builtin_completes_after_quiet_output() {
        let t = Instant::now();
        let mut q = queue(&["cd /tmp", "pwd"]);
        assert!(send(&mut q, t).is_some());
        q.saw_output(t + Duration::from_millis(2));
        assert_eq!(q.poll(Some(true), t + Duration::from_millis(50)), None, "not quiet long enough");
        q.saw_output(t + Duration::from_millis(60));
        assert_eq!(q.poll(Some(true), t + Duration::from_millis(100)), None);
        assert_eq!(q.poll(Some(true), t + Duration::from_millis(60) + QUIET), Some(b"pwd".to_vec()));
    }

    #[test]
    fn silent_idle_never_counts_as_done() {
        let t = Instant::now();
        let mut q = queue(&["a", "b"]);
        assert!(send(&mut q, t).is_some());
        assert_eq!(q.poll(Some(true), t + Duration::from_secs(10)), None, "no echo, no busy: wait");
    }

    #[test]
    fn unknown_state_sends_nothing() {
        let mut q = queue(&["a"]);
        assert_eq!(q.poll(None, Instant::now()), None);
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn last_send_finishing_makes_it_inactive() {
        let t = Instant::now();
        let mut q = queue(&["sleep 1"]);
        send(&mut q, t);
        q.poll(Some(false), t + Duration::from_millis(10));
        assert_eq!(q.poll(Some(true), t + Duration::from_secs(1)), None);
        assert!(!q.active());
    }

    #[test]
    fn enter_goes_without_echo_after_a_moment() {
        let t = Instant::now();
        let mut q = queue(&["ls"]);
        assert_eq!(q.poll(Some(true), t), Some(b"ls".to_vec()));
        assert_eq!(q.enter_due(), Some(t + ECHO));
        assert_eq!(q.poll(Some(true), t + ECHO / 2), None);
        assert_eq!(q.poll(Some(true), t + ECHO), Some(b"\r".to_vec()));
        assert_eq!(q.enter_due(), None);
    }

    #[test]
    fn pop_and_clear() {
        let mut q = queue(&["a", "b", "c"]);
        assert_eq!(q.pop_last().as_deref(), Some("c"));
        assert_eq!(q.items().collect::<Vec<_>>(), ["a", "b"]);
        q.clear();
        assert!(!q.active());
    }

    #[test]
    fn shells() {
        assert!(is_shell("/bin/bash"));
        assert!(is_shell("-zsh"));
        assert!(is_shell("/opt/homebrew/bin/fish"));
        assert!(!is_shell("/usr/bin/vim"));
        assert!(!is_shell("bashful"));
    }
}
