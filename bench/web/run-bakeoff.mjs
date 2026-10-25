// M1 Web Worker gate: every core's wasm build in a worker, in headless Chromium.
// Usage: node run-bakeoff.mjs [trials=10]. Needs bench/bakeoff/build-wasm.sh first.
// Compares checkpoint hashes with bench/results/2026-10-25-bakeoff/native-hashes-fixed-4096.json
// and writes wasm-worker.json next to it.
import { chromium } from "playwright";
import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { serve } from "./serve.mjs";
import { median } from "./stats.mjs";

const trials = Number(process.argv[2] ?? 10);
const here = fileURLToPath(new URL(".", import.meta.url));
const results = join(here, "../results/2026-10-25-bakeoff");
const native = JSON.parse(await readFile(join(results, "native-hashes-fixed-4096.json"), "utf8"));
const FIXTURES = ["ascii-flood", "sgr-flood", "unicode-mix", "long-lines", "tui-redraw", "edge-cases"];
const CORES = ["libghostty-vt", "alacritty_terminal", "wezterm-term"];

const server = await serve();
const browser = await chromium.launch({ args: ["--disable-renderer-backgrounding", "--disable-background-timer-throttling"] });
const page = await browser.newPage();
page.on("console", m => console.log("[page]", m.text()));
await page.goto(`http://127.0.0.1:${server.address().port}/bakeoff.html`);
await page.waitForFunction(() => window.bench?.ready);

const out = { browser: `chromium ${browser.version()} (Playwright, headless)`, date: new Date().toISOString(), trials, chunk: 4096, cases: [] };
for (const fixture of FIXTURES) {
  for (const core of CORES) {
    const r = await page.evaluate(a => window.bench.run(a), { core, fixture, chunk: 4096, trials });
    if (!r.ok) { console.log(fixture, core, "ERROR", r.error); out.cases.push(r); continue; }
    const want = native.fixtures[fixture][core];
    const match = JSON.stringify(want) === JSON.stringify(r.hashes);
    const p50 = median(r.ms);
    console.log(`${fixture.padEnd(12)} ${core.padEnd(19)} native-hash ${match ? "MATCH" : "DIFFER"}  ${(r.bytes / 1e6 / (p50 / 1000)).toFixed(1).padStart(7)} MB/s`);
    out.cases.push({ ...r, nativeMatch: match, medianMBs: r.bytes / 1e6 / (p50 / 1000) });
  }
}
await browser.close();
server.close();
await writeFile(join(results, "wasm-worker.json"), JSON.stringify(out, null, 2));
