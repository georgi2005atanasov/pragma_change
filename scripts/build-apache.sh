#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
APXS=${APXS:-apxs}
BUILD=${APACHE_BUILD_DIR:-build/apache}
RUST_LIB=${PRAGMA_RUST_LIB:-target/release/libpragma_ffi.a}
if [ -z "${PRAGMA_RUST_LIB:-}" ]; then
  cargo build --locked --release -p pragma-ffi -p pragma-engine
fi
mkdir -p "$BUILD"
cp apache/mod_pragmachange.c include/pragmachange.h "$BUILD/"
RUST_LIB=$(realpath "$RUST_LIB")
cd "$BUILD"
"$APXS" -c -Wc,'-std=c11 -Wall -Wextra -Werror -D_GNU_SOURCE' -Wl,'-Wl,--exclude-libs=ALL' mod_pragmachange.c "$RUST_LIB" -laprutil-1 -ldl -lpthread -lm
cp .libs/mod_pragmachange.so mod_pragmachange.so
