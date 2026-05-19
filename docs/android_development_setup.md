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

## Fixed APK Signing

To allow continuous over-the-air upgrades without requiring users to uninstall and reinstall, the APK must be signed with a consistent keystore. Without fixed signing, each GitHub Actions run may produce an APK with a different debug certificate, making it impossible to install over a previously installed version.

### How to Generate a Release Keystore

```bash
keytool -genkey -v \
  -keystore writer-release.jks \
  -keyalg RSA \
  -keysize 2048 \
  -validity 10000 \
  -alias writer-release
```

Follow the prompts to set:
- Keystore password
- Key password (can be the same as keystore password)

**Important:** Keep this keystore file safe. If lost, future APKs cannot upgrade over previously installed versions.

### How to Base64 Encode the Keystore

```bash
base64 -w 0 writer-release.jks > writer-release.jks.b64
```

### How to Configure GitHub Secrets

Go to your GitHub repository **Settings > Secrets and variables > Actions** and add:

| Secret Name | Value |
|---|---|
| `ANDROID_KEYSTORE_BASE64` | Contents of `writer-release.jks.b64` |
| `ANDROID_KEYSTORE_PASSWORD` | Your keystore password |
| `ANDROID_KEY_ALIAS` | Your key alias (e.g. `writer-release`) |
| `ANDROID_KEY_PASSWORD` | Your key password |

### Local Development with Fixed Signing

Set environment variables before building:

```bash
export WRITER_ANDROID_KEYSTORE_PATH=/path/to/writer-release.jks
export WRITER_ANDROID_KEYSTORE_PASSWORD=your-password
export WRITER_ANDROID_KEY_ALIAS=writer-release
export WRITER_ANDROID_KEY_PASSWORD=your-key-password
./tools/build_android.sh
```

If these environment variables are not set, the build falls back to the default debug signing.

### Important Note on First Upgrade

If you have previously installed a version signed with a random debug certificate, you **must uninstall it first** before installing the first fixed-signing version. From the first fixed-signing version onward, all future builds can be installed as upgrades without data loss.
