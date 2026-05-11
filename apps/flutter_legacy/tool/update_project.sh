#!/bin/bash

set -euo pipefail

# Confirm current directory is git root
if [ ! -d ".git" ]; then
    echo "Error: Please run this script from the root directory of the git repository."
    exit 1
fi

# Check for flutter
if ! command -v flutter &> /dev/null; then
    echo "Error: flutter command not found."
    echo "Please install Flutter first. You can refer to docs/fedora_development_setup.md for instructions."
    exit 1
fi

# Confirm current branch is main
CURRENT_BRANCH=$(git rev-parse --abbrev-ref HEAD)
if [ "$CURRENT_BRANCH" != "main" ]; then
    echo "Error: Current branch is '$CURRENT_BRANCH'. Please checkout 'main' branch before updating."
    exit 1
fi

# Handle local changes
STATUS=$(git status --short)
if [ -n "$STATUS" ]; then
    echo "Local changes detected:"
    echo "$STATUS"
    echo "Stashing changes..."
    TIMESTAMP=$(date +"%Y%m%d_%H%M%S")
    git stash push -u -m "auto-stash before update_project.sh $TIMESTAMP"
    echo "Local changes have been stashed. They will NOT be automatically restored to avoid conflicts."
    echo "You can view your stashes using 'git stash list' and restore them manually with 'git stash pop' if needed."
fi

# Update process
echo "Fetching origin main..."
git fetch origin main

echo "Pulling latest changes (fast-forward only)..."
if ! git pull --ff-only origin main; then
    echo "Error: git pull --ff-only failed."
    echo "There might be local commits or the remote history has forked."
    echo "Please resolve the issue manually."
    exit 1
fi

echo "Running DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$DIR/.."
flutter pub get..."
DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$DIR/.."
flutter pub get

echo "Running flutter analyze..."
if ! flutter analyze; then
    echo "Error: flutter analyze failed."
    exit 1
fi

echo "Running flutter test..."
if ! flutter test; then
    echo "Error: flutter test failed."
    exit 1
fi

# Workspace clean check
FINAL_STATUS=$(git status --short)
echo "--------------------------------------------------"
if [ -z "$FINAL_STATUS" ]; then
    echo "工作区干净 (Workspace is clean)."
else
    echo "There are remaining changes in the workspace:"
    echo "$FINAL_STATUS"
fi
