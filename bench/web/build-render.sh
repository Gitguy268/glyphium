#!/bin/sh
# Builds crates/terminal-wasm for the browser into target/web-pkg/terminal/.
set -eu
cd "$(dirname "$0")/../.."
export PATH="$HOME/.cargo/bin:$PATH"
cargo build -q --profile wasm-size --target wasm32-unknown-unknown -p glyphium-terminal-wasm
wasm-bindgen --target web --out-dir target/web-pkg/terminal target/wasm32-unknown-unknown/wasm-size/glyphium_terminal_wasm.wasm
bg=target/web-pkg/terminal/glyphium_terminal_wasm_bg.wasm
wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals --enable-reference-types --enable-multivalue "$bg" -o "$bg.oz" && mv "$bg.oz" "$bg"
printf 'wasm %s bytes, gzip %s bytes\n' "$(wc -c < "$bg" | tr -d ' ')" "$(gzip -9c "$bg" | wc -c | tr -d ' ')"
