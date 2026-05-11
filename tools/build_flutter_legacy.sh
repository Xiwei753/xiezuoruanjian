#!/bin/bash
DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$DIR/.."

echo "Building Flutter Legacy..."
cd apps/flutter_legacy
flutter pub get
# flutter build apk
