// Minimal static server for the browser benchmarks: this folder, its node_modules, and the
// generated fixtures at /fixtures/. Cross-origin isolation headers are on so that
// SharedArrayBuffer and precise timers behave the same as in the Glyphium web client.
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const here = fileURLToPath(new URL(".", import.meta.url));
const roots = [
  ["/fixtures/", join(here, "../../fixtures/generated")],
  ["/pkg/", join(here, "../../target/web-pkg")],
  ["/", here],
];
const types = { ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript", ".css": "text/css", ".wasm": "application/wasm", ".json": "application/json", ".bin": "application/octet-stream" };

export function serve(port = 0) {
  const server = createServer(async (req, res) => {
    const path = decodeURIComponent(new URL(req.url, "http://x").pathname);
    const [prefix, dir] = roots.find(([p]) => path.startsWith(p));
    const file = normalize(join(dir, path.slice(prefix.length)));
    if (!file.startsWith(dir)) { res.writeHead(403).end(); return; }
    try {
      const body = await readFile(file);
      res.writeHead(200, {
        "content-type": types[extname(file)] ?? "application/octet-stream",
        "cross-origin-opener-policy": "same-origin",
        "cross-origin-embedder-policy": "require-corp",
        "cache-control": "no-store",
      }).end(body);
    } catch {
      res.writeHead(404).end();
    }
  });
  return new Promise(resolve => server.listen(port, "127.0.0.1", () => resolve(server)));
}
