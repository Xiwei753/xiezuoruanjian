#!/bin/bash
set -euo pipefail

TIMESTAMP=$(date +%s)
OUT_DIR="diagnostics_cli_$TIMESTAMP"

echo "Collecting CLI diagnostics into $OUT_DIR..."
mkdir -p "$OUT_DIR"

# 1. Environment and Tools Info
echo "Collecting tool versions..."
{
  echo "Flutter Version:"
  flutter --version || echo "Flutter not found or failed"
  echo "----------------"
  echo "Dart Version:"
  dart --version || echo "Dart not found or failed"
} > "$OUT_DIR/tools_info.txt"

# 2. Git Info
echo "Collecting git info..."
{
  echo "Git Branch:"
  git branch --show-current || echo "Not a git repository"
  echo "Git Commit:"
  git rev-parse HEAD || echo "No commit found"
  echo "Git Status:"
  git status --short || echo "No status"
} > "$OUT_DIR/git_info.txt"

# 3. Environment Variables (Input Method & Linux Desktop specifics)
echo "Collecting environment variables..."
{
  echo "XDG_SESSION_TYPE=${XDG_SESSION_TYPE:-unset}"
  echo "WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-unset}"
  echo "DISPLAY=${DISPLAY:-unset}"
  echo "GDK_BACKEND=${GDK_BACKEND:-unset}"
  echo "GTK_IM_MODULE=${GTK_IM_MODULE:-unset}"
  echo "QT_IM_MODULE=${QT_IM_MODULE:-unset}"
  echo "SDL_IM_MODULE=${SDL_IM_MODULE:-unset}"
  echo "XMODIFIERS=${XMODIFIERS:-unset}"
  echo "XDG_CURRENT_DESKTOP=${XDG_CURRENT_DESKTOP:-unset}"
} > "$OUT_DIR/env_vars.txt" 2>/dev/null || true

# 4. App Logs
# Find the workspace log file. Assuming standard location if workspace is in Documents,
# or relative to the project if debugging. We'll try common paths.
LOG_PATH="$HOME/Documents/writer_app_workspace/app-meta/logs/app.log"
echo "Collecting logs..."
if [ -f "$LOG_PATH" ]; then
  tail -n 500 "$LOG_PATH" > "$OUT_DIR/logs_tail.jsonl"
  # Optional simple sed pass to censor sensitive keys just in case
  sed -i -E 's/"(deepSeekApiKey|githubToken|password|secret|apiKey|token)":\s*"[^"]*"/"\1": "***"/gi' "$OUT_DIR/logs_tail.jsonl"
else
  echo "Log file not found at $LOG_PATH" > "$OUT_DIR/logs_tail.jsonl"
fi

echo "Done! Diagnostics collected in: $OUT_DIR"
