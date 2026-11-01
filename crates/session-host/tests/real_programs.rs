//! Runs real programs on a real PTY and checks what ends up on the terminal screen. Programs
//! missing from this machine are skipped with a message, so a skip is visible in `--nocapture`.
#![cfg(unix)]

use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use glyphium_session_host::{Dimensions, Profile, Session, SessionEvent};
use glyphium_terminal_core::{Frame, Terminal};

const DIMS: Dimensions = Dimensions { cols: 80, rows: 24, pixel_width: 800, pixel_height: 480 };

struct Harness {
    session: Session,
    events: Receiver<SessionEvent>,
    term: Terminal,
    frame: Frame,
    exit: Option<Option<u32>>,
    total: usize,
}

impl Harness {
    fn run(command: &str, args: &[&str]) -> Self {
        let profile = Profile { command: Some(command.into()), args: args.iter().map(|a| a.to_string()).collect(), ..Default::default() };
        let (session, events) = Session::open(&profile, DIMS, 7, || {}).expect("spawn");
        Self { session, events, term: Terminal::new(DIMS.cols, DIMS.rows, 5000), frame: Frame::default(), exit: None, total: 0 }
    }

    fn sh(script: &str) -> Self {
        Self::run("/bin/sh", &["-c", script])
    }

    /// Feeds output to the terminal and replies to the program until `done` holds.
    fn pump_until(&mut self, what: &str, mut done: impl FnMut(&mut Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if done(self) {
                return;
            }
            if Instant::now() > deadline {
                panic!("timed out waiting for {what}; screen:\n{}", self.screen());
            }
            match self.events.recv_timeout(Duration::from_millis(50)) {
                Ok(SessionEvent::Output { generation, bytes }) => {
                    assert_eq!(generation, 7);
                    self.total += bytes.len();
                    self.term.write(&bytes);
                    let replies = self.term.take_replies();
                    self.session.write(replies);
                }
                Ok(SessionEvent::Exit { generation, code }) => {
                    assert_eq!(generation, 7);
                    self.exit = Some(code);
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    assert!(self.exit.is_some(), "events ended without an exit");
                    assert!(done(self), "session ended before {what}; screen:\n{}", self.screen());
                    return;
                }
            }
        }
    }

    fn screen(&mut self) -> String {
        self.term.update_frame(&mut self.frame);
        let lines: Vec<String> = self.frame.rows.iter().map(|r| r.cells.iter().map(|c| if c.text.is_empty() && c.width != 0 { " " } else { c.text.as_str() }).collect::<String>().trim_end().to_string()).collect();
        lines.join("\n")
    }

    fn wait_for(&mut self, text: &str) {
        let t = text.to_string();
        self.pump_until(text, move |h| h.screen().contains(&t));
    }

    fn wait_exit(&mut self) -> Option<u32> {
        self.pump_until("exit", |h| h.exit.is_some());
        self.exit.unwrap()
    }

    fn send(&self, bytes: &[u8]) {
        self.session.write(bytes.to_vec());
    }
}

fn have(program: &str) -> bool {
    let found = std::process::Command::new("/bin/sh").args(["-c", &format!("command -v {program}")]).output().is_ok_and(|o| o.status.success());
    if !found {
        eprintln!("SKIPPED: {program} is not installed");
    }
    found
}

#[test]
fn output_and_exit_status() {
    let mut h = Harness::sh("printf 'hello from sh\\n'; exit 3");
    h.wait_for("hello from sh");
    assert_eq!(h.wait_exit(), Some(3));
}

#[test]
fn output_written_just_before_exit_is_not_lost() {
    let mut h = Harness::sh("seq 1 20000; exit 0");
    assert_eq!(h.wait_exit(), Some(0));
    assert!(h.screen().contains("20000"), "{}", h.screen());
}

#[test]
fn flood_arrives_complete() {
    // 200 MB would be slow in debug; 30 MB is well past the queue's 4 MiB bound.
    let mut h = Harness::sh("head -c 30000000 /dev/zero | tr '\\0' x; printf '\\nDONE'");
    h.wait_for("DONE");
    assert_eq!(h.wait_exit(), Some(0));
    assert!(h.total >= 30_000_000, "got {} bytes", h.total);
}

#[test]
fn replies_reach_the_program() {
    // The shell asks for the cursor position and prints what the terminal answered.
    let script = r#"stty -echo -icanon min 0 time 20; printf '\033[5;10H\033[6n'; IFS= read -r -d R r 2>/dev/null || r=$(dd bs=1 count=6 2>/dev/null); printf '\r\ngot:%s\n' "$(printf %s "$r" | tr -d '\033')""#;
    let mut h = Harness::run("/bin/bash", &["-c", script]);
    h.wait_for("got:[5;10");
}

#[test]
fn resize_reaches_the_program() {
    let mut h = Harness::sh("stty size; read x; stty size; sleep 0.2");
    h.wait_for("24 80");
    h.session.resize(Dimensions { cols: 100, rows: 30, pixel_width: 1000, pixel_height: 600 }).unwrap();
    h.send(b"go\n");
    h.wait_for("30 100");
}

#[test]
fn large_paste_while_program_is_busy_keeps_order() {
    // 1 MB of numbered lines to a program that starts reading only after a pause: write()
    // must not block, and the last line read must be the last line sent.
    let mut h = Harness::sh("stty -echo -icanon; printf 'READY\\n'; sleep 0.3; head -c 1000000 | tail -c 12; printf 'END'");
    h.wait_for("READY");
    let t = Instant::now();
    for i in 0..10_000 {
        h.send(format!("{i:>99}\n").as_bytes());
    }
    assert!(t.elapsed() < Duration::from_millis(250), "write blocked for {:?}", t.elapsed());
    h.wait_for("END");
    assert!(h.screen().contains("9999"), "{}", h.screen());
    assert_eq!(h.wait_exit(), Some(0));
}

#[test]
fn close_kills_the_program() {
    let mut h = Harness::sh("echo started; exec sleep 600");
    h.wait_for("started");
    h.session.close();
    assert!(h.wait_exit() != Some(0));
}

#[test]
fn stale_generation_is_visible_on_every_event() {
    let (s, events) = Session::open(&Profile { command: Some("/bin/sh".into()), args: vec!["-c".into(), "echo x".into()], ..Default::default() }, DIMS, 42, || {}).unwrap();
    let _keep = s;
    for e in events.iter().take(2) {
        assert_eq!(e.generation(), 42);
    }
}

#[test]
fn less_pages_and_quits() {
    if !have("less") {
        return;
    }
    let mut h = Harness::sh("seq 1 500 | less");
    h.wait_for("23");
    h.send(b" ");
    h.wait_for("46");
    h.send(b"q");
    assert_eq!(h.wait_exit(), Some(0));
}

#[test]
fn vim_edits_and_restores_the_screen() {
    if !have("vim") {
        return;
    }
    let file = format!("/tmp/glyphium-vim-{}.txt", std::process::id());
    let mut h = Harness::sh(&format!("echo before-vim; vim -u NONE -N {file}; cat {file}; rm -f {file}"));
    h.wait_for("~");
    assert!(!h.screen().contains("before-vim"), "alternate screen hides the shell");
    h.send(b"ihello from vim\x1b");
    h.wait_for("hello from vim");
    h.send(b":wq\r");
    assert_eq!(h.wait_exit(), Some(0));
    let screen = h.screen();
    assert!(screen.contains("before-vim") && screen.contains("hello from vim"), "{screen}");
}

#[test]
fn tmux_draws_inside_the_terminal() {
    if !have("tmux") {
        return;
    }
    let sock = format!("glyphium-test-{}", std::process::id());
    let mut h = Harness::run("tmux", &["-L", &sock, "-f", "/dev/null", "new-session", "sh -c 'echo inside-tmux; read x'"]);
    h.wait_for("inside-tmux");
    h.wait_for("[0]"); // tmux's status line
    h.send(b"\r");
    assert_eq!(h.wait_exit(), Some(0));
}

#[test]
fn htop_draws_and_quits() {
    if !have("htop") {
        return;
    }
    let mut h = Harness::run("htop", &[]);
    h.wait_for("PID");
    h.send(b"q");
    assert_eq!(h.wait_exit(), Some(0));
}
