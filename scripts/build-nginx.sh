#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
VERSION=1.28.0
mkdir -p "$ROOT/build"
if [ ! -d "$ROOT/build/nginx-$VERSION" ]; then
  curl -fsSL "https://nginx.org/download/nginx-$VERSION.tar.gz" -o "$ROOT/build/nginx-$VERSION.tar.gz"
  (cd "$ROOT/build" && sha256sum -c "$ROOT/nginx/source.sha256")
  tar -xzf "$ROOT/build/nginx-$VERSION.tar.gz" -C "$ROOT/build"
fi
cargo build --manifest-path "$ROOT/Cargo.toml" -p pragma-ffi --release
export PRAGMA_RUST_LIB="$ROOT/target/release/libpragma_ffi.a"
cd "$ROOT/build/nginx-$VERSION"
./configure --prefix="$ROOT/build/nginx" --with-compat --with-debug --without-http_rewrite_module --without-http_gzip_module --add-dynamic-module="$ROOT/nginx"
make -j"${BUILD_JOBS:-2}"
make install
mkdir -p "$ROOT/build/nginx/modules"
cp objs/ngx_http_pragmachange_module.so "$ROOT/build/nginx/modules/"
