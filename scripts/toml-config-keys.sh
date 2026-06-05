#!/usr/bin/env bash

extract_toml_keys() {
  local file="$1"

  awk '
  /^[[:space:]]*#/ { next }
  /^[[:space:]]*$/ { next }

  /^\[.*\]$/ {
    section=$0
    gsub(/[\[\]]/, "", section)
    next
  }

  /=/ {
    split($0, a, "=")
    key=a[1]
    gsub(/[[:space:]]+$/, "", key)

    if (section != "")
      print section "." key
    else
      print key
  }
' "$file" | sort -u
}
