# Android Development Setup

## Prerequisites
- Install Android Studio and the Android SDK.
- Accept Android licenses:
  ```bash
  flutter doctor --android-licenses
  ```

## Viewing Devices
To see available devices (e.g. connected phones or emulators), run:
```bash
flutter devices
```

## Running the App
To run the app on an Android device in debug mode, you can use Android Studio, or build the APK and deploy it manually. Ensure the Rust core is built by using our provided build script first.

## Building APKs
To build a debug APK with the Rust core included:
```bash
./tools/build_android.sh
```

If you only want to run Gradle without rebuilding the Rust core, you can use:
```bash
./tools/build_android_gradle_only.sh
```

## Downloading the APK
You can easily test the Android Native MVP without building it locally.
Every push or pull request to the `main` branch automatically builds a debug APK via GitHub Actions.

To download the latest APK:
1. Go to the **Actions** tab in the GitHub repository.
2. Select the **Android Native Debug Build** workflow on the left side.
3. Click on the most recent successful workflow run.
4. Scroll down to the **Artifacts** section at the bottom of the summary page.
5. Click on `android-native-debug-apk` to download the zip file containing the APK.

## Testing Priorities
This is an early Android adaptation. Please prioritize testing:
- App startup
- Project management (creation, listing)
- Chapter editing
- Auto-saving functionality
- Keyboard appearance and TextField scrolling behavior (ensure it's not obscured)
