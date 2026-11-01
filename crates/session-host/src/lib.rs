//! A local shell session on a PTY (Unix) or ConPTY (Windows).
//!
//! Output arrives on a reader thread and passes through a bounded queue, so a program that
//! floods the terminal blocks in its own write instead of growing memory here. Input goes
//! through a writer thread in order, so typing or a large paste never blocks the UI while the
//! program is busy; the UI and terminal-generated replies share that one ordered channel.
//! Every event carries the session's generation, so late output from a closed session cannot
//! enter its replacement.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, SyncSender, channel, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;

use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};

/// One read's worth of output; matches a typical PTY buffer several times over.
const READ_CHUNK: usize = 64 * 1024;
/// Queue depth in chunks: at most 4 MiB of output waits in memory.
const OUTPUT_QUEUE: usize = 64;

#[derive(Clone, Debug, Default)]
pub struct Profile {
    /// Program to run; `None` is the user's login shell.
    pub command: Option<String>,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: HashMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dimensions {
    pub cols: u16,
    pub rows: u16,
    pub pixel_width: u16,
    pub pixel_height: u16,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SessionEvent {
    Output { generation: u64, bytes: Vec<u8> },
    /// The program ended. `code` is `None` if its status could not be read.
    Exit { generation: u64, code: Option<u32> },
}

impl SessionEvent {
    pub fn generation(&self) -> u64 {
        match self {
            SessionEvent::Output { generation, .. } | SessionEvent::Exit { generation, .. } => *generation,
        }
    }
}

enum WriterMessage {
    Data(Vec<u8>),
    Stop,
}

pub struct Session {
    pub generation: u64,
    input: Sender<WriterMessage>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    pid: Option<u32>,
}

fn other(e: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::other(e.to_string())
}

fn pty_size(d: Dimensions) -> PtySize {
    PtySize { rows: d.rows, cols: d.cols, pixel_width: d.pixel_width, pixel_height: d.pixel_height }
}

impl Session {
    /// Starts the program. Events go to the returned receiver; `wake` runs after each one is
    /// queued, from a background thread, so a UI event loop can be told to look. Dropping the
    /// receiver lets the reader thread end.
    pub fn open(profile: &Profile, dims: Dimensions, generation: u64, wake: impl Fn() + Send + Sync + 'static) -> std::io::Result<(Session, Receiver<SessionEvent>)> {
        let pair = native_pty_system().openpty(pty_size(dims)).map_err(other)?;
        let mut cmd = match &profile.command {
            Some(c) => {
                let mut b = CommandBuilder::new(c);
                b.args(&profile.args);
                b
            }
            None => CommandBuilder::new_default_prog(),
        };
        if let Some(dir) = &profile.cwd {
            cmd.cwd(dir);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "Glyphium");
        for (k, v) in &profile.env {
            cmd.env(k, v);
        }
        let mut child = pair.slave.spawn_command(cmd).map_err(other)?;
        // The child holds its own copy; keeping ours would stop EOF arriving on exit.
        drop(pair.slave);
        let pid = child.process_id();
        let killer = child.clone_killer();
        let mut reader = pair.master.try_clone_reader().map_err(other)?;
        let mut writer = pair.master.take_writer().map_err(other)?;

        let (events, receiver): (SyncSender<SessionEvent>, _) = sync_channel(OUTPUT_QUEUE);
        let wake = Arc::new(wake);

        // The exit status is sent only after the reader has drained everything: output that a
        // program writes just before it exits must not be lost or arrive after the exit.
        let (drained_tx, drained_rx) = channel::<()>();
        {
            let (events, wake) = (events.clone(), wake.clone());
            thread::Builder::new().name("glyphium-pty-read".into()).spawn(move || {
                let mut buf = vec![0u8; READ_CHUNK];
                loop {
                    match reader.read(&mut buf) {
                        // EIO is how Linux reports that the child closed its side.
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if events.send(SessionEvent::Output { generation, bytes: buf[..n].to_vec() }).is_err() {
                                break;
                            }
                            wake();
                        }
                    }
                }
                let _ = drained_tx.send(());
            })?;
        }
        thread::Builder::new().name("glyphium-pty-wait".into()).spawn(move || {
            let code = child.wait().ok().map(|s| s.exit_code());
            // A grandchild can keep the PTY open past the shell's exit, so don't wait for ever.
            let _ = drained_rx.recv_timeout(std::time::Duration::from_secs(2));
            let _ = events.send(SessionEvent::Exit { generation, code });
            wake();
        })?;

        let (input, input_rx) = channel::<WriterMessage>();
        thread::Builder::new().name("glyphium-pty-write".into()).spawn(move || {
            while let Ok(WriterMessage::Data(bytes)) = input_rx.recv() {
                if writer.write_all(&bytes).is_err() || writer.flush().is_err() {
                    break;
                }
            }
        })?;

        Ok((Session { generation, input, master: Mutex::new(pair.master), killer: Mutex::new(killer), pid }, receiver))
    }

    /// Queues input for the program; never blocks. Order is preserved.
    pub fn write(&self, bytes: Vec<u8>) {
        if !bytes.is_empty() {
            let _ = self.input.send(WriterMessage::Data(bytes));
        }
    }

    pub fn resize(&self, dims: Dimensions) -> std::io::Result<()> {
        self.master.lock().unwrap().resize(pty_size(dims)).map_err(other)
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Ends the session: stops input and kills the program.
    pub fn close(&self) {
        let _ = self.input.send(WriterMessage::Stop);
        let _ = self.killer.lock().unwrap().kill();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.close();
    }
}
