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
To run the app on an Android device in debug mode:
```bash
./tool/run_android_debug.sh
```

## Building APKs
To build a debug APK:
```bash
./tool/build_android_apk_debug.sh
```

To build a release APK (Note: Currently uses debug signature, for local testing only. Do not commit a real keystore or sign for production yet):
```bash
./tool/build_android_apk_release.sh
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
