// Runs one bake-off core (a wasm-bindgen build from bench/bakeoff/build-wasm.sh) inside a
// Web Worker. Checkpoint hashes use the same chunk boundaries as the native runner's
// `fixed-N` chunking, so they can be compared byte for byte with native results.
const DIRS = { "libghostty-vt": "ghostty", "alacritty_terminal": "alacritty", "wezterm-term": "wezterm" };

function boundaries(len, chunk, forced) {
  const cuts = new Set(forced.filter(f => f > 0 && f < len));
  for (let at = chunk; at < len; at += chunk) cuts.add(at);
  cuts.add(len);
  return [...cuts].sort((a, b) => a - b);
}

onmessage = async ({ data: { core, fixture, chunk, trials } }) => {
  try {
    const mod = await import(`/pkg/${DIRS[core]}/glyphium_bakeoff.js`);
    await mod.default();
    const bytes = new Uint8Array(await (await fetch(`/fixtures/${fixture}.bin`)).arrayBuffer());
    const len = bytes.length;
    const checkpoints = [Math.floor(len / 4), Math.floor(len / 2), Math.floor(len * 3 / 4), len];

    // Correctness run (not timed).
    const hashes = [];
    let b = new mod.Bench(core), start = 0;
    for (const end of boundaries(len, chunk, checkpoints)) {
      b.write(bytes.subarray(start, end));
      start = end;
      if (checkpoints.includes(end)) hashes.push(b.hash());
    }
    b.free();

    // Timed runs: plain fixed-size chunks, writes only.
    const ms = [];
    for (let t = 0; t < trials; t++) {
      b = new mod.Bench(core);
      const t0 = performance.now();
      for (let at = 0; at < len; at += chunk) b.write(bytes.subarray(at, at + chunk));
      ms.push(performance.now() - t0);
      b.free();
    }
    postMessage({ ok: true, core, fixture, bytes: len, chunk, hashes, ms });
  } catch (e) {
    postMessage({ ok: false, core, fixture, error: String(e?.stack ?? e) });
  }
};
