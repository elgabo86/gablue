#!/usr/bin/bash
# Build de la version Rust. cpal exige des en-têtes ALSA absents de Kinoite :
# on les prend dans brew et on lie le libasound du système (celui qui parle à
# PipeWire) via un alsa.pc généré.
set -euo pipefail
cd "$(dirname "$0")"
ROOT="$PWD"

BREW="$(brew --prefix)"
mkdir -p .build/alsa/lib .build/alsa/pkgconfig
ln -sf /usr/lib64/libasound.so.2 .build/alsa/lib/libasound.so

cat > .build/alsa/pkgconfig/alsa.pc <<EOF
prefix=$BREW
libdir=$ROOT/.build/alsa/lib
includedir=$BREW/include
Name: alsa
Description: ALSA runtime
Version: 1.2.0
Libs: -L\${libdir} -lasound
Cflags: -I\${includedir}
EOF

export PKG_CONFIG_PATH="$ROOT/.build/alsa/pkgconfig:${PKG_CONFIG_PATH:-}"
cargo build --release "$@"
TARGET="$(cargo metadata --format-version 1 --no-deps | sed -E 's/.*"target_directory":"([^"]*)".*/\1/')"
echo "binaire : $TARGET/release/retroplayer"
