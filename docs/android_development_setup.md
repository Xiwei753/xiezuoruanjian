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

## Testing Priorities
This is an early Android adaptation. Please prioritize testing:
- App startup
- Project management (creation, listing)
- Chapter editing
- Auto-saving functionality
- Keyboard appearance and TextField scrolling behavior (ensure it's not obscured)
