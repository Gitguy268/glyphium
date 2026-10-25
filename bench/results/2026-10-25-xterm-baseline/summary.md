# xterm.js baseline (TabShell renderer path)

2026-10-25T12:54:39.515Z · chromium 153.0.8010.12 (Playwright 1.63.0, headless) · ANGLE (Apple, ANGLE Metal Renderer: Apple M4, Unspecified Version) · macOS 27.0.1 (26A434), Darwin 27.0.0

20 measured trials after 3 warmups, randomized order, 4 KiB chunks, terminal 120x40. Throughput = fixture bytes / time until xterm's final write callback (parse + buffer update); "presented" adds two animation frames. Frame interval = requestAnimationFrame gaps during the write, which shows main-thread blocking.

| Fixture | Mode | Renderer | MB/s p50 [95% CI] | presented ms p50 | frame p50 / p95 / p99 ms | state hashes |
|---|---|---|---|---|---|---|
| ascii-flood | tabshell | webgl | 25.0 [23.0–27.1] | 332 | 16.7 / 16.7 / 16.7 | 1 (b7559c64) |
| ascii-flood | bytes | webgl | 26.5 [24.4–28.8] | 319 | 16.7 / 16.7 / 16.7 | 1 (b7559c64) |
| sgr-flood | tabshell | webgl | 3.4 [3.1–3.6] | 2984 | 83.3 / 116.7 / 133.3 | 1 (1b404493) |
| sgr-flood | bytes | webgl | 3.6 [3.5–4.0] | 2869 | 83.3 / 116.7 / 133.3 | 1 (1b404493) |
| unicode-mix | tabshell | webgl | 16.5 [12.6–17.5] | 169 | 16.7 / 33.3 / 118.5 | 1 (c4761e99) |
| unicode-mix | bytes | webgl | 17.6 [14.0–18.5] | 161 | 16.7 / 33.3 / 66.7 | 1 (957ceab3) |
| long-lines | tabshell | webgl | 24.8 [23.0–27.2] | 341 | 16.7 / 16.7 / 16.7 | 1 (a5b0587c) |
| long-lines | bytes | webgl | 30.3 [28.6–32.0] | 288 | 16.7 / 16.7 / 16.7 | 1 (a5b0587c) |
| tui-redraw | tabshell | webgl | 20.4 [20.0–21.0] | 567 | 16.7 / 16.7 / 33.3 | 1 (b2b7bf27) |
| tui-redraw | bytes | webgl | 22.2 [20.6–24.2] | 532 | 16.7 / 16.7 / 16.7 | 1 (b2b7bf27) |

Every trial of a fixture/mode pair ended in the same state.

Byte and string writes end in different states for: unicode-mix. Known cause for unicode-mix: xterm.js 6.0.0 drops U+200D (ZWJ) when a Uint8Array write ends after the 2nd of its 3 UTF-8 bytes (see docs/PROGRESS.md). The "tabshell" string path is the correct one.
