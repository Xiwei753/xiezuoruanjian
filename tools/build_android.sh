#!/bin/bash

DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
WORKSPACE_ROOT="$( cd "$DIR/.." && pwd )"

# Parse arguments
RUST_FEATURES=""
GRADLE_FLAVOR="noAiDebug"
APK_VARIANT="noAiDebug"

for arg in "$@"; do
    case $arg in
        --ai)
            RUST_FEATURES="--features ai"
            GRADLE_FLAVOR="aiDebug"
            APK_VARIANT="aiDebug"
            ;;
        --no-ai)
            RUST_FEATURES=""
            GRADLE_FLAVOR="noAiDebug"
            APK_VARIANT="noAiDebug"
            ;;
        *)
            echo "Unknown argument: $arg"
            echo "Usage: $0 [--ai|--no-ai]"
            exit 1
            ;;
    esac
done

echo "Building Writer Core JNI bindings for Android..."
echo "  Rust features: ${RUST_FEATURES:-<none>}"
echo "  Gradle flavor: $GRADLE_FLAVOR"

if ! command -v cargo-ndk &> /dev/null; then
    echo "Error: cargo-ndk is not installed."
    echo "To build the Rust JNI library, please install it via: cargo install cargo-ndk"
    echo "Ensure ANDROID_NDK_HOME is set properly."
    echo ""
    echo "Aborting build to prevent missing libwriter_core_jni.so."
    exit 1
fi

echo "Using cargo-ndk to compile for arm64-v8a..."
cd "$WORKSPACE_ROOT/bindings/android"

# shellcheck disable=SC2086
cargo ndk -t arm64-v8a -o "$WORKSPACE_ROOT/apps/android/app/src/main/jniLibs" build --release $RUST_FEATURES

if [ $? -eq 0 ]; then
    echo "Rust JNI library successfully built and copied to jniLibs."
else
    echo "Failed to build Rust JNI library."
    exit 1
fi

cd "$WORKSPACE_ROOT/apps/android"

echo "Building Android Native Application ($GRADLE_FLAVOR)..."
./gradlew "assemble${GRADLE_FLAVOR^}"

if [ $? -eq 0 ]; then
    echo "Android Native Build Successful."

    # Find the APK with the custom name
    APK_DIR="app/build/outputs/apk/${APK_VARIANT%Debug}/debug"
    APK_PATH=$(find "$APK_DIR" -name "writer-android-*.apk" -type f 2>/dev/null | head -1)

    if [ -z "$APK_PATH" ]; then
        # Fallback to default name
        APK_PATH="app/build/outputs/apk/${APK_VARIANT%Debug}/debug/app-${APK_VARIANT}.apk"
    fi

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
