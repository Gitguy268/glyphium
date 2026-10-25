// xterm.js baseline (TabShell's renderer path) in headless Chromium with a
// real GPU through ANGLE/Metal. Usage: node run-baseline.mjs [trials=20] [warmups=3]
// Writes bench/results/<date>-xterm-baseline/{env.json,raw.json,summary.md}.
import { chromium } from "playwright";
import { execSync } from "node:child_process";
import { mkdir, writeFile, readFile } from "node:fs/promises";
import { cpus, totalmem, release } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { serve } from "./serve.mjs";
import { describe, median, shuffle } from "./stats.mjs";

const trials = Number(process.argv[2] ?? 20), warmups = Number(process.argv[3] ?? 3);
const here = fileURLToPath(new URL(".", import.meta.url));
const repo = join(here, "../..");
const FIXTURES = ["ascii-flood", "sgr-flood", "unicode-mix", "long-lines", "tui-redraw"];
const MODES = ["tabshell", "bytes"];

const sh = cmd => { try { return execSync(cmd, { encoding: "utf8" }).trim(); } catch { return null; } };

const server = await serve();
const url = `http://127.0.0.1:${server.address().port}/baseline.html`;
const browser = await chromium.launch({ args: ["--enable-unsafe-webgpu", "--use-angle=metal", "--disable-renderer-backgrounding", "--disable-background-timer-throttling"] });
const page = await browser.newPage({ viewport: { width: 1280, height: 860 }, deviceScaleFactor: 1 });
await page.goto(url);
await page.waitForFunction(() => window.bench?.ready);

const env = {
  date: new Date().toISOString(),
  glyphiumCommit: sh(`git -C ${repo} rev-parse HEAD`),
  tabshellHead: sh("git -C ~/tabshell rev-parse HEAD"),
  xterm: "6.0.0", webglAddon: "0.19.0",
  browser: `chromium ${browser.version()} (Playwright 1.63.0, headless)`,
  gpu: await page.evaluate(() => window.bench.gpuInfo()),
  os: `${sh("sw_vers -productName")} ${sh("sw_vers -productVersion")} (${sh("sw_vers -buildVersion")}), Darwin ${release()}`,
  cpu: cpus()[0].model, cores: cpus().length, memGiB: Math.round(totalmem() / 2 ** 30),
  power: sh("pmset -g batt | head -1"),
  viewport: "1280x860 @1x, terminal 120x40, font 14px, scrollback 5000",
  fixtures: Object.fromEntries((await readFile(join(repo, "fixtures/MANIFEST.txt"), "utf8")).trim().split("\n").map(l => l.split(/\s+/).reverse())),
  trials, warmups, chunk: 4096,
};
console.log(env.browser, "|", env.gpu.renderer);

const raw = [];
const cases = FIXTURES.flatMap(name => MODES.map(mode => ({ name, mode })));
for (let round = 0; round < warmups + trials; round++) {
  for (const c of shuffle(cases, round + 1)) {
    const r = await page.evaluate(args => window.bench.run(args), c);
    if (round >= warmups) raw.push({ round, ...r });
  }
  process.stdout.write(`round ${round + 1}/${warmups + trials}\r`);
}
await browser.close();
server.close();

// Correctness before speed: every trial of a fixture must end in the same screen state.
const lines = ["# xterm.js baseline (TabShell renderer path)", "", `${env.date} · ${env.browser} · ${env.gpu.renderer} · ${env.os}`, "",
  `${trials} measured trials after ${warmups} warmups, randomized order, 4 KiB chunks, terminal 120x40. Throughput = fixture bytes / time until xterm's final write callback (parse + buffer update); "presented" adds two animation frames. Frame interval = requestAnimationFrame gaps during the write, which shows main-thread blocking.`, "",
  "| Fixture | Mode | Renderer | MB/s p50 [95% CI] | presented ms p50 | frame p50 / p95 / p99 ms | state hashes |", "|---|---|---|---|---|---|---|"];
let consistent = true;
const crossModeDiffs = new Set();
for (const { name, mode } of cases) {
  const rs = raw.filter(r => r.name === name && r.mode === mode);
  const mbps = rs.map(r => r.bytes / 1e6 / (r.parsedMs / 1000));
  const d = describe(mbps), f = describe(rs.flatMap(r => r.frameIntervals));
  const hashes = new Set(rs.map(r => r.hash));
  const crossMode = new Set(raw.filter(r => r.name === name).map(r => r.hash));
  if (hashes.size !== 1) consistent = false;
  if (crossMode.size !== 1) crossModeDiffs.add(name);
  lines.push(`| ${name} | ${mode} | ${rs[0].renderer} | ${d.p50.toFixed(1)} [${d.ci95[0].toFixed(1)}–${d.ci95[1].toFixed(1)}] | ${median(rs.map(r => r.presentedMs)).toFixed(0)} | ${f.p50.toFixed(1)} / ${f.p95.toFixed(1)} / ${f.p99.toFixed(1)} | ${hashes.size === 1 ? "1 (" + rs[0].hash.slice(0, 8) + ")" : hashes.size + " DIFFER"} |`);
}
lines.push("", consistent ? "Every trial of a fixture/mode pair ended in the same state." : "**State hashes differ between trials of the same fixture and mode; throughput numbers are not valid until this is explained.**");
if (crossModeDiffs.size) lines.push("", `Byte and string writes end in different states for: ${[...crossModeDiffs].join(", ")}. Known cause for unicode-mix: xterm.js 6.0.0 drops U+200D (ZWJ) when a Uint8Array write ends after the 2nd of its 3 UTF-8 bytes (see docs/PROGRESS.md). The "tabshell" string path is the correct one.`);

const out = join(repo, "bench/results", `${env.date.slice(0, 10)}-xterm-baseline`);
await mkdir(out, { recursive: true });
await writeFile(join(out, "env.json"), JSON.stringify(env, null, 2));
await writeFile(join(out, "raw.json"), JSON.stringify(raw));
await writeFile(join(out, "summary.md"), lines.join("\n") + "\n");
console.log("\n" + lines.join("\n"));
