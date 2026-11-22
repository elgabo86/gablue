#!/usr/bin/bash
# Lance la GUI. `zxtune123` est cherché dans $ZXTUNE, ~/.cache/gtracker/bin,
# bin/ puis PATH.
set -euo pipefail
cd "$(dirname "$0")"

if [[ -z "${ZXTUNE:-}" ]]; then
    for cand in "$HOME/.cache/gtracker/bin/zxtune123" "$PWD/bin/zxtune123"; do
        [[ -x "$cand" ]] && export ZXTUNE="$cand" && break
    done
fi
[[ -z "${ZXTUNE:-}" ]] && echo "attention : zxtune123 introuvable" >&2

TARGET="$(cargo metadata --format-version 1 --no-deps | sed -E 's/.*"target_directory":"([^"]*)".*/\1/')"
exec "$TARGET/release/gtracker" "$@"
