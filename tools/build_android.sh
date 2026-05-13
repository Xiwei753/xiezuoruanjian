#!/bin/bash

DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
WORKSPACE_ROOT="$( cd "$DIR/.." && pwd )"

echo "Building Writer Core JNI bindings for Android..."

if ! command -v cargo-ndk &> /dev/null; then
    echo "Error: cargo-ndk is not installed."
    echo "To build the Rust JNI library, please install it via: cargo install cargo-ndk"
    echo "Ensure ANDROID_NDK_HOME is set properly."
    echo ""
    echo "Aborting build to prevent missing libwriter_core_jni.so."
    exit 1
fi

echo "Using cargo-ndk to compile for x86_64 and arm64-v8a..."
cd "$WORKSPACE_ROOT/bindings/android"

cargo ndk -t arm64-v8a -t x86_64 -o "$WORKSPACE_ROOT/apps/android/app/src/main/jniLibs" build --release

if [ $? -eq 0 ]; then
    echo "Rust JNI library successfully built and copied to jniLibs."
else
    echo "Failed to build Rust JNI library."
    exit 1
fi

cd "$WORKSPACE_ROOT/apps/android"

echo "Building Android Native Application..."
./gradlew assembleDebug

if [ $? -eq 0 ]; then
    echo "Android Native Build Successful."
    APK_PATH="app/build/outputs/apk/debug/app-debug.apk"
    echo "APK located at: apps/android/$APK_PATH"

    echo "Verifying JNI .so files in the APK..."
    if unzip -l "$APK_PATH" | grep -q "lib/arm64-v8a/libwriter_core_jni.so"; then
        echo "Check passed: libwriter_core_jni.so found in arm64-v8a"
    else
        echo "Error: libwriter_core_jni.so NOT found in arm64-v8a inside the APK!"
        exit 1
    fi
else
    echo "Android Native Build Failed."
    exit 1
fi
