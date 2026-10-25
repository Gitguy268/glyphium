#!/bin/sh
# Builds each core separately to wasm32 (wasm-size profile: opt-level z, LTO, panic=abort),
# runs wasm-bindgen --target web into target/web-pkg/<core>/, and records sizes after
# wasm-opt -Oz, the same treatment for every core.
#
# Needs: rustup's wasm32-unknown-unknown target, wasm-bindgen 0.2.129, wasm-opt, and a
# clang with a wasm32 backend for wezterm-term's psm dependency (Apple clang has none;
# Homebrew llvm is used). The ghostty build needs .tools/ghostty/zig-out-wasm.
set -eu
cd "$(dirname "$0")/../.."
export PATH="$HOME/.cargo/bin:$PATH"
LLVM="${LLVM:-/opt/homebrew/opt/llvm/bin}"
export CC_wasm32_unknown_unknown="$LLVM/clang" AR_wasm32_unknown_unknown="$LLVM/llvm-ar"
out=target/web-pkg
mkdir -p "$out"
printf 'core\twasm_bytes\twasm_opt_Oz_bytes\tgzip9_bytes\n' > "$out/sizes.tsv"
for core in ghostty alacritty wezterm; do
  cargo build -q --profile wasm-size --target wasm32-unknown-unknown -p glyphium-bakeoff --lib \
    --no-default-features --features "$core"
  wasm-bindgen --target web --out-dir "$out/$core" \
    target/wasm32-unknown-unknown/wasm-size/glyphium_bakeoff.wasm
  bg="$out/$core/glyphium_bakeoff_bg.wasm"
  wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext \
    --enable-mutable-globals --enable-reference-types --enable-multivalue "$bg" -o "$bg.oz"
  mv "$bg.oz" "$bg"
  printf '%s\t%s\t%s\t%s\n' "$core" \
    "$(wc -c < target/wasm32-unknown-unknown/wasm-size/glyphium_bakeoff.wasm | tr -d ' ')" \
    "$(wc -c < "$bg" | tr -d ' ')" "$(gzip -9c "$bg" | wc -c | tr -d ' ')" >> "$out/sizes.tsv"
done
cat "$out/sizes.tsv"
