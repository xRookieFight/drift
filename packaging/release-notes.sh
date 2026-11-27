#!/usr/bin/env bash
# Prints the CHANGELOG.md section for one version, so the GitHub Release body matches the changelog.
set -euo pipefail
version="${1:?usage: release-notes.sh <version>}"
awk -v v="$version" '
  $0 ~ "^## \\[" v "\\]" { on = 1; next }
  on && /^## \[/ { exit }
  on { print }
' CHANGELOG.md
