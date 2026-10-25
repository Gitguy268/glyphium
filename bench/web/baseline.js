// xterm.js baseline with TabShell's pinned versions and options (src/main.js in ~/tabshell).
// The runner calls window.bench.run(...) once per trial; each trial gets a fresh Terminal.
import { Terminal } from "/node_modules/@xterm/xterm/lib/xterm.mjs";
import { WebglAddon } from "/node_modules/@xterm/addon-webgl/lib/addon-webgl.mjs";

const COLS = 120, ROWS = 40;
const fixtures = new Map();

async function fixture(name) {
  if (!fixtures.has(name)) fixtures.set(name, new Uint8Array(await (await fetch(`/fixtures/${name}.bin`)).arrayBuffer()));
  return fixtures.get(name);
}

function makeTerm() {
  const term = new Terminal({
    cols: COLS, rows: ROWS,
    cursorBlink: true,
    scrollback: 5000,
    fontFamily: 'ui-monospace, "SF Mono", Menlo, Consolas, monospace',
    fontSize: 14,
    theme: { background: "#0d1117", foreground: "#e6edf3", cursor: "#5fd68a", selectionBackground: "#264f78" },
  });
  term.open(document.getElementById("term"));
  let renderer = "dom";
  try {
    const webgl = new WebglAddon();
    term.loadAddon(webgl);
    renderer = "webgl";
  } catch { /* DOM renderer */ }
  return { term, renderer };
}

const frame = () => new Promise(r => requestAnimationFrame(r));

async function stateHash(term) {
  const buf = term.buffer.active;
  const lines = [];
  for (let i = 0; i < buf.length; i++) lines.push(buf.getLine(i).translateToString(true));
  lines.push(`cursor ${buf.cursorX},${buf.cursorY} type ${buf.type}`);
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(lines.join("\n")));
  return [...new Uint8Array(digest)].map(b => b.toString(16).padStart(2, "0")).join("");
}

// mode "tabshell": stream-decode to strings like src/vm.js and src/docker.js.
// mode "bytes": hand Uint8Array chunks straight to term.write.
async function run({ name, mode, chunk = 4096 }) {
  const bytes = await fixture(name);
  const { term, renderer } = makeTerm();
  await frame();
  const dec = new TextDecoder();
  const intervals = [];
  let last = performance.now(), sampling = true;
  (function tick(t) { if (!sampling) return; intervals.push(t - last); last = t; requestAnimationFrame(tick); })(last);

  const t0 = performance.now();
  await new Promise(resolve => {
    for (let off = 0; off < bytes.length; off += chunk) {
      const part = bytes.subarray(off, off + chunk);
      const data = mode === "bytes" ? part : dec.decode(part, { stream: true });
      const final = off + chunk >= bytes.length;
      term.write(data, final ? resolve : undefined);
    }
  });
  const parsed = performance.now() - t0;
  await frame(); await frame();
  const presented = performance.now() - t0;
  sampling = false;

  const hash = await stateHash(term);
  term.dispose();
  return { name, mode, chunk, renderer, bytes: bytes.length, parsedMs: parsed, presentedMs: presented, frameIntervals: intervals.slice(1), hash };
}

function gpuInfo() {
  const gl = document.createElement("canvas").getContext("webgl2");
  const ext = gl?.getExtension("WEBGL_debug_renderer_info");
  return { webgl2: !!gl, vendor: ext ? gl.getParameter(ext.UNMASKED_VENDOR_WEBGL) : null, renderer: ext ? gl.getParameter(ext.UNMASKED_RENDERER_WEBGL) : null };
}

window.bench = { run, gpuInfo, ready: true };
