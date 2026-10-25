//! Bake-off runner.
//!
//! `glyphium-bakeoff invariance` checks chunk-invariance; `dump` writes each core's final
//! screen text for cross-core diffs; `throughput` times parsing. Results go to stdout as
//! JSON. Fixtures are read from `fixtures/generated` and checked against
//! `fixtures/MANIFEST.txt` first.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Instant;

use glyphium_bakeoff::{Chunking, Core, checkpoints, run_hashes};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

type Factory = (&'static str, fn() -> Box<dyn Core>);

fn cores() -> Vec<Factory> {
    let mut v: Vec<Factory> = vec![];
    #[cfg(feature = "ghostty")]
    v.push(("libghostty-vt", || Box::new(glyphium_bakeoff::ghostty::Ghostty::new())));
    #[cfg(feature = "alacritty")]
    v.push(("alacritty_terminal", || Box::new(glyphium_bakeoff::alacritty::Alacritty::new())));
    #[cfg(feature = "wezterm")]
    v.push(("wezterm-term", || Box::new(glyphium_bakeoff::wezterm::Wezterm::new())));
    v
}

fn fixtures(only: Option<&str>) -> Vec<(String, Vec<u8>)> {
    let manifest = fs::read_to_string("fixtures/MANIFEST.txt").expect("run from the repo root");
    let mut out = vec![];
    for line in manifest.lines() {
        let (sum, file) = line.split_once("  ").expect("manifest line");
        let name = file.trim_end_matches(".bin");
        if only.is_some_and(|o| o != name) {
            continue;
        }
        let bytes = fs::read(Path::new("fixtures/generated").join(file)).expect("fixture; run glyphium-fixtures");
        let got = glyphium_bakeoff::hex(&Sha256::digest(&bytes));
        assert_eq!(got, sum, "{file} does not match MANIFEST.txt");
        out.push((name.to_string(), bytes));
    }
    out
}

fn sampled() -> Vec<Chunking> {
    let mut v: Vec<Chunking> = [1, 2, 3, 7, 13, 4096].into_iter().map(Chunking::Fixed).collect();
    v.extend((1..=3).map(|seed| Chunking::Random { seed, max: 8192 }));
    v.push(Chunking::Random { seed: 4, max: 16 });
    v
}

fn invariance(only: Option<&str>, mode2027: bool) -> Value {
    let mut report = BTreeMap::new();
    for (name, mut bytes) in fixtures(only) {
        if mode2027 {
            // Grapheme clustering (mode 2027) on from the first byte; cores without it ignore it.
            bytes.splice(0..0, *b"\x1b[?2027h");
        }
        let exhaustive = bytes.len() <= 16 * 1024;
        let mut strategies = sampled();
        if exhaustive {
            strategies.extend((1..bytes.len()).map(Chunking::SplitAt));
        }
        let mut per_core = BTreeMap::new();
        for (core_name, make) in cores() {
            let t = Instant::now();
            let reference = run_hashes(&mut *make(), &bytes, &Chunking::Whole);
            let mut failures = vec![];
            for s in &strategies {
                let got = run_hashes(&mut *make(), &bytes, s);
                if got != reference {
                    let cp = got.iter().zip(&reference).position(|(a, b)| a != b).unwrap();
                    failures.push(json!({ "chunking": s.label(), "first_bad_checkpoint": checkpoints(bytes.len())[cp] }));
                }
            }
            eprintln!("{name:<12} {core_name:<19} {} runs, {} failures, {:.1}s", strategies.len(), failures.len(), t.elapsed().as_secs_f64());
            per_core.insert(core_name, json!({
                "pass": failures.is_empty(),
                "runs": strategies.len(),
                "exhaustive_splits": exhaustive,
                "reference_hashes": reference,
                "failures": failures.iter().take(50).collect::<Vec<_>>(),
                "failure_count": failures.len(),
            }));
        }
        report.insert(name, json!({ "bytes": bytes.len(), "checkpoints": checkpoints(bytes.len()), "cores": per_core }));
    }
    json!(report)
}

fn dump(only: Option<&str>) -> Value {
    let mut report = BTreeMap::new();
    for (name, bytes) in fixtures(only) {
        let mut per_core = BTreeMap::new();
        for (core_name, make) in cores() {
            let mut core = make();
            core.write(&bytes);
            let s = core.snapshot();
            per_core.insert(core_name, json!({
                "cursor": [s.cursor.0, s.cursor.1],
                "cursor_visible": s.cursor_visible,
                "alt_screen": s.alt_screen,
                "title": s.title,
                "history_len": s.history_len,
                "replies": String::from_utf8_lossy(&s.replies).escape_debug().to_string(),
                "screen": s.screen_text(),
                "widths": s.screen.iter().map(|r| r.cells.iter().map(|c| c.width as u32).sum::<u32>()).collect::<Vec<_>>(),
            }));
        }
        report.insert(name, json!(per_core));
    }
    json!(report)
}

fn parse_chunking(s: &str) -> Chunking {
    let (kind, n) = s.rsplit_once('-').unwrap_or((s, "0"));
    let n: usize = n.parse().unwrap();
    match kind {
        "whole" => Chunking::Whole,
        "fixed" => Chunking::Fixed(n),
        "split" => Chunking::SplitAt(n),
        _ => panic!("chunking: whole | fixed-N | split-N"),
    }
}

/// States at the checkpoints for one chunking.
fn run_states(core: &mut dyn Core, bytes: &[u8], chunking: &Chunking) -> Vec<glyphium_bakeoff::State> {
    let cps = checkpoints(bytes.len());
    let mut out = vec![];
    let mut start = 0;
    for end in chunking.boundaries(bytes.len(), &cps) {
        core.write(&bytes[start..end]);
        start = end;
        if cps.contains(&end) {
            out.push(core.snapshot());
        }
    }
    out
}

/// Field-by-field difference between `whole` and another chunking, at the first checkpoint
/// where they disagree.
fn diff(only: Option<&str>, core: &str, chunking: &str) -> Value {
    let (_, bytes) = fixtures(only).into_iter().next().expect("--fixture");
    let make = cores().into_iter().find(|(n, _)| *n == core).expect("--core").1;
    let a = run_states(&mut *make(), &bytes, &Chunking::Whole);
    let b = run_states(&mut *make(), &bytes, &parse_chunking(chunking));
    let cps = checkpoints(bytes.len());
    for (i, (x, y)) in a.iter().zip(&b).enumerate() {
        if x == y {
            continue;
        }
        let mut d = serde_json::Map::new();
        d.insert("checkpoint".into(), json!(cps[i]));
        if x.cursor != y.cursor { d.insert("cursor".into(), json!([format!("{:?}", x.cursor), format!("{:?}", y.cursor)])); }
        if x.title != y.title { d.insert("title".into(), json!([x.title, y.title])); }
        if x.alt_screen != y.alt_screen { d.insert("alt_screen".into(), json!([x.alt_screen, y.alt_screen])); }
        if x.history_len != y.history_len { d.insert("history_len".into(), json!([x.history_len, y.history_len])); }
        if x.replies != y.replies {
            d.insert("replies".into(), json!([String::from_utf8_lossy(&x.replies).escape_debug().to_string(), String::from_utf8_lossy(&y.replies).escape_debug().to_string()]));
        }
        let mut rows = vec![];
        for (label, xs, ys) in [("history", &x.history, &y.history), ("screen", &x.screen, &y.screen)] {
            for (r, (p, q)) in xs.iter().zip(ys.iter()).enumerate() {
                if p != q {
                    let cells: Vec<String> = p.cells.iter().zip(q.cells.iter()).filter(|(c, e)| c != e).take(3).map(|(c, e)| format!("{c:?} != {e:?}")).collect();
                    rows.push(json!({ "where": label, "row": r, "wrapped": [p.wrapped, q.wrapped], "whole": glyphium_bakeoff::row_text(p), "chunked": glyphium_bakeoff::row_text(q), "cells": cells, "lens": [p.cells.len(), q.cells.len()] }));
                }
            }
            if xs.len() != ys.len() { rows.push(json!({ "where": label, "len": [xs.len(), ys.len()] })); }
        }
        d.insert("rows".into(), json!(rows.into_iter().take(10).collect::<Vec<_>>()));
        return Value::Object(d);
    }
    json!("identical")
}

/// Checkpoint hashes for one chunking, every fixture and core: the native side of the
/// native-vs-wasm comparison.
fn hashes(only: Option<&str>, chunking: &str) -> Value {
    let c = parse_chunking(chunking);
    let mut report = BTreeMap::new();
    for (name, bytes) in fixtures(only) {
        let per_core: BTreeMap<_, _> = cores().into_iter().map(|(n, make)| (n, run_hashes(&mut *make(), &bytes, &c))).collect();
        report.insert(name, json!(per_core));
    }
    json!({ "chunking": c.label(), "fixtures": report })
}

/// Cursor advance each core gives a cluster on an empty line: the width disagreements behind
/// the cross-core diffs. The clusters are the unicode-mix pieces plus a few edge cases.
fn probe() -> Value {
    const PIECES: &[&str] = &[
        "日本語", "😀", "🎉", "👍🏽", "👨\u{200d}👩\u{200d}👧\u{200d}👦", "🏳\u{fe0f}\u{200d}🌈", "❤\u{fe0f}", "❤", "☺\u{fe0e}",
        "e\u{301}", "a\u{308}\u{332}", "नमस्ते", "क्षत्रिय", "مرحبا", "ไทย", "─│┌┐", "½", "™", "\u{200b}", "ﬁ",
        "𝓜𝓪𝓽𝓱", "🇳🇱", "🇯", "\u{1f469}\u{200d}\u{1f467}",
    ];
    let mut out = vec![];
    for p in PIECES {
        let mut row = serde_json::Map::new();
        row.insert("cluster".into(), json!(p.escape_unicode().to_string()));
        row.insert("text".into(), json!(p));
        // Default modes, then with mode 2027 (grapheme clustering) requested first.
        for (prefix, suffix) in [("", ""), ("\x1b[?2027h", " +2027")] {
            for (name, make) in cores() {
                let mut core = make();
                core.write(format!("{prefix}\x1b[H\x1b[2J{p}").as_bytes());
                row.insert(format!("{name}{suffix}"), json!(core.snapshot().cursor.0));
            }
        }
        out.push(Value::Object(row));
    }
    json!(out)
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn throughput(only: Option<&str>, trials: usize, chunk: usize) -> Value {
    let mut report = BTreeMap::new();
    for (name, bytes) in fixtures(only) {
        let factories = cores();
        let mut secs: Vec<Vec<f64>> = vec![vec![]; factories.len()];
        // Two warmups, then trials with the core order rotated each round.
        for round in 0..trials + 2 {
            for k in 0..factories.len() {
                let i = (k + round) % factories.len();
                let mut core = (factories[i].1)();
                let t = Instant::now();
                for c in bytes.chunks(chunk) {
                    core.write(c);
                }
                let dt = t.elapsed().as_secs_f64();
                std::hint::black_box(&mut core);
                if round >= 2 {
                    secs[i].push(dt);
                }
            }
        }
        let mut per_core = BTreeMap::new();
        for (i, (core_name, _)) in factories.iter().enumerate() {
            let raw = secs[i].clone();
            let med = median(&mut secs[i]);
            per_core.insert(*core_name, json!({
                "median_mb_s": bytes.len() as f64 / med / 1e6,
                "median_s": med,
                "trials_s": raw,
            }));
            eprintln!("{name:<12} {core_name:<19} chunk {chunk:>5}  {:>8.1} MB/s", bytes.len() as f64 / med / 1e6);
        }
        report.insert(name, json!({ "bytes": bytes.len(), "chunk": chunk, "trials": trials, "cores": per_core }));
    }
    json!(report)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |f: &str| args.iter().position(|a| a == f).map(|i| args[i + 1].clone());
    // `--mode2027` (invariance only) takes no value.
    let only = flag("--fixture");
    let only = only.as_deref();
    let out = match args.first().map(String::as_str) {
        Some("invariance") => invariance(only, args.iter().any(|a| a == "--mode2027")),
        Some("dump") => dump(only),
        Some("probe") => probe(),
        Some("hashes") => hashes(only, &flag("--chunking").unwrap_or_else(|| "fixed-4096".into())),
        Some("diff") => diff(only, &flag("--core").expect("--core"), &flag("--chunking").expect("--chunking")),
        Some("throughput") => throughput(
            only,
            flag("--trials").map_or(10, |t| t.parse().unwrap()),
            flag("--chunk").map_or(4096, |c| c.parse().unwrap()),
        ),
        _ => {
            eprintln!("usage: glyphium-bakeoff invariance|dump|diff|hashes|probe|throughput [--fixture NAME] [--trials N] [--chunk BYTES]");
            std::process::exit(2);
        }
    };
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}
