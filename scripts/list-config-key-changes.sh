#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/toml-config-keys.sh"

if [[ "$#" -ne 2 ]]; then
  echo "usage: $0 <before-ref> <after-ref>" >&2
  exit 2
fi

BEFORE_REF="$1"
AFTER_REF="$2"

is_watched_config_file() {
  case "$1" in
    miner-apps/jd-client/config-examples/*.toml) return 0 ;;
    miner-apps/translator/config-examples/*.toml) return 0 ;;
    *) return 1 ;;
  esac
}

extract_keys_from_ref() {
  local ref="$1"
  local file="$2"
  local tmp_file

  if ! git cat-file -e "$ref:$file" 2>/dev/null; then
    return 0
  fi

  tmp_file="$(mktemp)"
  git show "$ref:$file" > "$tmp_file"
  extract_toml_keys "$tmp_file"
  rm -f "$tmp_file"
}

git diff --name-only "$BEFORE_REF" "$AFTER_REF" -- \
  miner-apps/jd-client/config-examples \
  miner-apps/translator/config-examples |
while IFS= read -r file; do
  if ! is_watched_config_file "$file"; then
    continue
  fi

  if ! diff -q \
    <(extract_keys_from_ref "$BEFORE_REF" "$file") \
    <(extract_keys_from_ref "$AFTER_REF" "$file") >/dev/null; then
    echo "$file"
  fi
done
