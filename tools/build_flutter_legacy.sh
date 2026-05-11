#!/bin/bash
echo "Building Flutter Legacy..."
DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$DIR/../apps/flutter_legacy"
flutter pub get
# flutter build apk
