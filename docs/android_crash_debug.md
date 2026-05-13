# Android Crash Debugging Guide

To effectively capture and debug Android startup and JNI crashes (such as `UnsatisfiedLinkError`, `FATAL EXCEPTION`, `NoSuchMethodError`, `JNI DETECTED ERROR`, or `ClassNotFoundException`), follow these steps:

1. Clear the current logcat buffer:
   ```
   adb logcat -c
   ```

2. Start a continuous logcat stream, filtering for relevant tags and errors:
   ```
   adb logcat -v time | grep -i "xiwei\|writer\|AndroidRuntime\|FATAL"
   ```

3. Launch the application on your device or emulator to reproduce the crash.

4. If you miss the crash and need to dump recent logs:
   ```
   adb logcat -d -t 300
   ```

Key things to look for:
- **FATAL EXCEPTION:** General Kotlin/Java runtime crashes.
- **UnsatisfiedLinkError:** Native library (`libwriter_core_jni.so`) is missing or failed to load.
- **NoSuchMethodError:** JNI bindings do not match the expected Kotlin `external fun` signature.
- **JNI DETECTED ERROR:** Invalid JNI operations, such as passing null to a function that expects non-null.
- **ClassNotFoundException:** Missing Kotlin classes or misconfigured ProGuard/R8 rules.
