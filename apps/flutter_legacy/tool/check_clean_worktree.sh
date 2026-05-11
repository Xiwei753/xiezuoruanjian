#!/bin/bash
DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$DIR/.."

set -e

echo "Running flutter pub get..."
flutter pub get

echo "Checking dart formatting..."
dart format --set-exit-if-changed .

echo "Running flutter analyze..."
flutter analyze

echo "Running flutter test..."
flutter test

echo "Checking for a clean git worktree..."
STATUS=$(git status --short)
if [ -n "$STATUS" ]; then
    echo "Error: The worktree is not clean. The following files are untracked or modified:"
    echo "$STATUS"
    exit 1
else
    echo "Worktree is perfectly clean."
fi

echo "All checks passed successfully."
