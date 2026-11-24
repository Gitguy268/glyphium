//! The assistant's language model: downloaded on request into the settings folder, then run on
//! a worker thread with llama.cpp (the `assist-model` feature). Results come back through a
//! callback that wakes the event loop, so the window never waits on the model.
//!
//! `GLYPHIUM_ASSIST_STUB=<reply>` replaces the model with a fixed reply, for tests.

use std::path::PathBuf;
use std::sync::mpsc::{Sender, channel};
use std::thread;
use std::time::Duration;

use crate::assist::ModelInfo;

#[derive(Debug)]
pub enum Event {
    /// The model's raw reply to request `id`, or why it failed.
    Answer { id: u64, reply: Result<String, String> },
    /// Download progress, 0 to 1.
    Progress(f32),
    Downloaded,
    DownloadFailed(String),
}

struct Job {
    id: u64,
    path: PathBuf,
    messages: Vec<(&'static str, String)>,
}

type Notify = std::sync::Arc<dyn Fn(Event) + Send + Sync>;

pub struct Assistant {
    notify: Notify,
    worker: Option<Sender<Job>>,
    downloading: bool,
    stub: Option<String>,
}

/// Where downloaded models live: `models/` in the settings folder.
pub fn models_dir() -> Option<PathBuf> {
    crate::config::dir().map(|d| d.join("models"))
}

pub fn model_path(m: &ModelInfo) -> Option<PathBuf> {
    models_dir().map(|d| d.join(m.file))
}

impl Assistant {
    pub fn new(notify: impl Fn(Event) + Send + Sync + 'static) -> Self {
        Self { notify: std::sync::Arc::new(notify), worker: None, downloading: false, stub: std::env::var("GLYPHIUM_ASSIST_STUB").ok() }
    }

    /// True when the model can answer now: downloaded in full (or stubbed).
    pub fn ready(&self, m: &ModelInfo) -> bool {
        self.stub.is_some() || model_path(m).and_then(|p| std::fs::metadata(p).ok()).is_some_and(|md| md.len() == m.size)
    }

    pub fn downloading(&self) -> bool {
        self.downloading
    }

    /// Asks the model; the answer arrives as [`Event::Answer`] with the same `id`.
    pub fn ask(&mut self, id: u64, m: &ModelInfo, messages: Vec<(&'static str, String)>) {
        if let Some(reply) = &self.stub {
            (self.notify)(Event::Answer { id, reply: Ok(reply.clone()) });
            return;
        }
        let Some(path) = model_path(m) else {
            (self.notify)(Event::Answer { id, reply: Err("no settings folder to keep the model in".into()) });
            return;
        };
        let job = Job { id, path, messages };
        if let Some(tx) = &self.worker {
            match tx.send(job) {
                Ok(()) => return,
                Err(e) => {
                    self.worker = None;
                    return self.ask(id, m, e.0.messages);
                }
            }
        }
        let (tx, rx) = channel::<Job>();
        let notify = self.notify.clone();
        let spawned = thread::Builder::new().name("glyphium-assist".into()).spawn(move || worker(rx, notify));
        if spawned.is_ok() {
            let _ = tx.send(job);
            self.worker = Some(tx);
        } else {
            (self.notify)(Event::Answer { id, reply: Err("could not start the model thread".into()) });
        }
    }

    /// Downloads the model with curl into a temporary file, renamed once complete and the right size.
    pub fn download(&mut self, m: &'static ModelInfo) {
        if self.downloading {
            return;
        }
        let Some(path) = model_path(m) else {
            (self.notify)(Event::DownloadFailed("no settings folder to keep the model in".into()));
            return;
        };
        self.downloading = true;
        let notify = self.notify.clone();
        let part = path.with_extension("part");
        let spawned = thread::Builder::new().name("glyphium-download".into()).spawn(move || {
            let result = download(m, &part, &path, &notify);
            notify(match result {
                Ok(()) => Event::Downloaded,
                Err(e) => Event::DownloadFailed(e),
            });
        });
        if spawned.is_err() {
            self.downloading = false;
        }
    }

    /// Called when [`Event::Downloaded`] or [`Event::DownloadFailed`] arrives.
    pub fn download_finished(&mut self) {
        self.downloading = false;
    }
}

fn download(m: &ModelInfo, part: &std::path::Path, path: &std::path::Path, notify: &Notify) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
    }
    let _ = std::fs::remove_file(part);
    let mut child = std::process::Command::new("curl")
        .args(["-fsSL", "--retry", "3", "-o"])
        .arg(part)
        .arg(m.url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("can't run curl: {e}"))?;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        let have = std::fs::metadata(part).map_or(0, |md| md.len());
        notify(Event::Progress((have as f32 / m.size as f32).min(1.0)));
        thread::sleep(Duration::from_millis(250));
    };
    if !status.success() {
        let mut err = String::new();
        if let Some(mut e) = child.stderr.take() {
            use std::io::Read;
            let _ = e.read_to_string(&mut err);
        }
        let _ = std::fs::remove_file(part);
        return Err(format!("download failed: {}", err.trim().lines().last().unwrap_or("curl error")));
    }
    let got = std::fs::metadata(part).map_or(0, |md| md.len());
    if got != m.size {
        let _ = std::fs::remove_file(part);
        return Err(format!("download was {got} bytes, expected {}", m.size));
    }
    std::fs::rename(part, path).map_err(|e| e.to_string())
}

#[cfg(feature = "assist-model")]
fn worker(rx: std::sync::mpsc::Receiver<Job>, notify: Notify) {
    use llama_cpp_2::context::params::LlamaContextParams;
    use llama_cpp_2::llama_backend::LlamaBackend;
    use llama_cpp_2::llama_batch::LlamaBatch;
    use llama_cpp_2::model::params::LlamaModelParams;
    use llama_cpp_2::model::{LlamaChatMessage, LlamaModel};
    use llama_cpp_2::sampling::LlamaSampler;

    let mut backend = match LlamaBackend::init() {
        Ok(b) => b,
        Err(e) => {
            for job in rx {
                notify(Event::Answer { id: job.id, reply: Err(format!("llama.cpp didn't start: {e}")) });
            }
            return;
        }
    };
    backend.void_logs();
    let mut loaded: Option<(PathBuf, LlamaModel)> = None;
    for job in rx {
        let reply = (|| -> Result<String, String> {
            if loaded.as_ref().is_none_or(|(p, _)| *p != job.path) {
                loaded = None;
                // CPU only: a 0.5B model answers in well under a second, and Metal's shader
                // compile would add seconds to the first question.
                let params = LlamaModelParams::default().with_n_gpu_layers(0);
                let model = LlamaModel::load_from_file(&backend, &job.path, &params).map_err(|e| format!("can't load the model: {e}"))?;
                loaded = Some((job.path.clone(), model));
            }
            let model = &loaded.as_ref().unwrap().1;
            let threads = thread::available_parallelism().map_or(4, |n| n.get().min(8)) as i32;
            let ctx_params = LlamaContextParams::default().with_n_ctx(std::num::NonZeroU32::new(1024)).with_n_threads(threads).with_n_threads_batch(threads);
            let mut ctx = model.new_context(&backend, ctx_params).map_err(|e| e.to_string())?;
            let template = model.chat_template(None).map_err(|e| e.to_string())?;
            let chat: Vec<LlamaChatMessage> = job.messages.iter().filter_map(|(role, content)| LlamaChatMessage::new(role.to_string(), content.clone()).ok()).collect();
            let prompt = model.apply_chat_template(&template, &chat, true).map_err(|e| e.to_string())?;
            let vocab = model.vocab();
            let tokens = vocab.tokenize(prompt.as_bytes(), false, true);
            if tokens.is_empty() || tokens.len() > 900 {
                return Err("the question is too long".into());
            }
            let mut batch = LlamaBatch::new(1024, 1);
            let last = tokens.len() - 1;
            for (i, t) in tokens.iter().enumerate() {
                batch.add(*t, i as i32, &[0], i == last).map_err(|e| e.to_string())?;
            }
            ctx.decode(&mut batch).map_err(|e| e.to_string())?;
            let mut sampler = LlamaSampler::greedy();
            let mut out = Vec::new();
            for pos in (tokens.len() as i32..).take(80) {
                let token = sampler.sample(&ctx, batch.n_tokens() - 1);
                sampler.accept(token);
                if vocab.is_eog(token) {
                    break;
                }
                out.extend(vocab.token_to_piece(token, false, None));
                if out.contains(&b'\n') && out.iter().any(|b| !b.is_ascii_whitespace()) {
                    break;
                }
                batch.clear();
                batch.add(token, pos, &[0], true).map_err(|e| e.to_string())?;
                ctx.decode(&mut batch).map_err(|e| e.to_string())?;
            }
            Ok(String::from_utf8_lossy(&out).into_owned())
        })();
        notify(Event::Answer { id: job.id, reply });
    }
}

#[cfg(not(feature = "assist-model"))]
fn worker(rx: std::sync::mpsc::Receiver<Job>, notify: Notify) {
    for job in rx {
        let _ = &job.messages;
        let _ = &job.path;
        notify(Event::Answer { id: job.id, reply: Err("this build of Glyphium has no model support (built without the assist-model feature)".into()) });
    }
}

#[cfg(all(test, feature = "assist-model"))]
mod tests {
    use super::*;
    use crate::assist::{self, Answer, System};

    /// Runs the real model when `GLYPHIUM_TEST_MODEL` points at a GGUF file:
    /// `GLYPHIUM_TEST_MODEL=~/models/nl2bash.gguf cargo test -p glyphium real_model -- --ignored`
    #[test]
    #[ignore]
    fn real_model_answers() {
        let Ok(path) = std::env::var("GLYPHIUM_TEST_MODEL") else { return };
        let (tx, rx) = std::sync::mpsc::channel();
        let (jobs_tx, jobs_rx) = channel();
        let tx = std::sync::Mutex::new(tx);
        let notify: Notify = std::sync::Arc::new(move |e| {
            let _ = tx.lock().unwrap().send(e);
        });
        thread::spawn(move || worker(jobs_rx, notify));
        for (id, (q, want)) in [("show disk usage of this folder", "du"), ("find files bigger than 100MB", "find"), ("copy file.txt to the clipboard", "pbcopy")].into_iter().enumerate() {
            jobs_tx.send(Job { id: id as u64, path: path.clone().into(), messages: assist::messages(q, System::Mac, "zsh") }).unwrap();
            let Event::Answer { reply, .. } = rx.recv_timeout(Duration::from_secs(60)).unwrap() else { panic!() };
            let reply = reply.unwrap();
            let Some(Answer::Command { cmd, .. }) = assist::parse_reply(&reply) else { panic!("{q}: {reply:?}") };
            eprintln!("{q} -> {cmd}");
            assert!(cmd.contains(want), "{q}: {cmd}");
        }
    }
}
