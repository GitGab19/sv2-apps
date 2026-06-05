#!/usr/bin/env bash

# WARNING: Implemented with GPT-5
#
# Extracts key names from a TOML-like file.
#
# Behavior:
#   - Ignores comments and blank lines
#   - Tracks the current [section]
#   - For lines containing '=', treats the left-hand side as a key
#   - Outputs keys as "section.key" or "key" if no section is active
#   - Sorts and removes duplicates
#
# Limitations:
#   - Not a full TOML parser
#   - Does not handle quoted keys, inline tables, arrays, multiline strings,
#     escapes, or nested sections
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
