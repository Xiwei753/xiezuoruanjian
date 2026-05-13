# Android Crash Debugging Guide

If the application crashes on startup, it might be due to native JNI loading issues. Follow these steps to capture the logs:

1. Connect your device or emulator.
2. Clear existing logcat:
   ```bash
   adb logcat -c
   ```
3. Start the logcat stream and filter by your app (e.g. `xiwei`):
   ```bash
   adb logcat -v time | grep -i xiwei
   ```
4. Launch the application on your device.
5. If it crashes, stop the logcat stream (Ctrl+C). Or export the recent lines:
   ```bash
   adb logcat -d -t 300 > crash_log.txt
   ```

Look for `UnsatisfiedLinkError` or `java.lang.Error` to identify JNI loading or linking failures.
