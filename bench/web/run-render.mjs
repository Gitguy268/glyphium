// M1 text gate: the same terminal frame rendered natively (Metal) and in Chromium (WebGPU).
// Usage: node run-render.mjs OUT_DIR. Needs build-render.sh and the render_png example first.
import { chromium } from "playwright";
import { spawnSync } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { serve } from "./serve.mjs";

const here = fileURLToPath(new URL(".", import.meta.url));
const repo = join(here, "../..");
const out = resolve(process.argv[2] ?? join(repo, "bench/results/render"));
await mkdir(out, { recursive: true });
const nativeRun = spawnSync(join(repo, "target/release/examples/render_png"), [join(out, "native.png")], { cwd: repo, encoding: "utf8" });
if (nativeRun.status !== 0) throw new Error(nativeRun.stderr);
const native = await readFile(join(out, "native.png.rgba"));

const server = await serve();
const browser = await chromium.launch({ args: ["--enable-unsafe-webgpu", "--use-angle=metal"] });
const page = await browser.newPage();
page.on("console", m => console.log("[page]", m.text()));
page.on("pageerror", e => console.log("[err]", e.message));
await page.goto(`http://127.0.0.1:${server.address().port}/render.html`);
await page.waitForFunction(() => window.bench?.ready);
const r = await page.evaluate(() => window.bench.run());
await browser.close();
server.close();

const web = Buffer.from(r.rgba, "base64");
await writeFile(join(out, "web.png"), Buffer.from(r.png.split(",")[1], "base64"));
if (web.length !== native.length) throw new Error(`size differs: web ${web.length}, native ${native.length}`);
let differing = 0, maxDelta = 0;
for (let i = 0; i < web.length; i += 4) {
  let d = 0;
  for (let k = 0; k < 4; k++) d = Math.max(d, Math.abs(web[i + k] - native[i + k]));
  if (d) differing++;
  maxDelta = Math.max(maxDelta, d);
}
const summary = { web: r.adapter, native: nativeRun.stderr.split("\n")[0].slice(0, 120), size: [r.w, r.h], pixels: web.length / 4, differing, maxDelta };
await writeFile(join(out, "summary.json"), JSON.stringify(summary, null, 2));
console.log(summary);
