#!/bin/bash
DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$DIR/../apps/android"

echo "Building Android Native..."
./gradlew assembleDebug
if [ $? -eq 0 ]; then
    echo "Android Native Build Successful."
    echo "APK located at: apps/android/app/build/outputs/apk/debug/app-debug.apk"
else
    echo "Android Native Build Failed."
    exit 1
fi
