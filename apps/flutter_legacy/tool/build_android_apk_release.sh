#!/bin/bash
DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$DIR/.."

echo "Warning: No real signature. For local testing only."
flutter build apk --release
