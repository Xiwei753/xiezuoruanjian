#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
src="$root/apps/linux/src"

search() {
  local pattern="$1"
  if command -v rg >/dev/null 2>&1; then
    rg "$pattern" "$src"
  else
    grep -R -n -E "$pattern" "$src"
  fi
}

if search 'writer_core::facade::WriterCore'; then
  echo "Linux code must not import writer_core::facade::WriterCore" >&2
  exit 1
fi

if search 'WriterCore::new\(';
then
  echo "Linux code must not construct WriterCore directly" >&2
  exit 1
fi

if search 'core_facade'; then
  echo "Linux code must not use core_facade" >&2
  exit 1
fi

echo "Linux API boundary check passed"
