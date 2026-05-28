# Plan to implement Monet color extraction and Sync

1. *Update Rust Core Settings*
   - Add `monet_color: String` to `SyncableSettings` struct in `core/writer_core/src/settings/mod.rs`. This will map to `monetColor` in JSON due to camelCase attribute.

2. *Update Kotlin Models*
   - Add `monetColor: String = ""` to `SyncableSettings` data class in `apps/android/app/src/main/kotlin/com/xiwei/writerapp/model/Models.kt`.

3. *Extract Monet Color on Android*
   - In `apps/android/app/src/main/kotlin/com/xiwei/writerapp/ui/MainActivity.kt`, fetch `android.R.color.system_accent1_500` to get the system's dynamic Monet color if the Android version is S (API 31) or above.
   - Convert it to a hex string (e.g. `"#FF5566"`).
   - Check if this color differs from the current `syncableSettings.monetColor` via `settingsRepository`. If it does, update `syncableSettings` with the new color and save it to the Rust Core.

4. *Expose to Linux Frontend*
   - In `apps/linux/src/main.rs`, add `current_setting_monet_color: String` to the `AppBackend` struct.
   - Add a corresponding `setting_monet_color` `qt_property!` (with getter, setter, and `NOTIFY settings_changed`).
   - Introduce calls to `core.load_syncable_settings()` and `core.save_syncable_settings()` within the `load_local_settings` and `save_local_settings` methods to handle the new `monet_color` property (reading into `current_setting_monet_color` and writing back out).

5. *Use Monet Color in Linux UI (QML)*
   - Modify `apps/linux/qml/DesignTokens.qml` to accept a `monetColor` property (defaulting to empty string).
   - Update the `accent` and related colors in `DesignTokens.qml` to use `monetColor` if it's a valid hex string, falling back to the default hardcoded accents otherwise.
   - In `apps/linux/qml/main.qml`, bind `designTokens.monetColor = appBackend.setting_monet_color`.

6. *Run tests*
   - Run the Rust core tests using `cargo test -p writer_core`.
   - Run the Android tests using `./gradlew test assembleDebug`.

7. *Complete pre commit steps*
   - Complete pre commit steps to ensure proper testing, verification, review, and reflection are done.

8. *Submit the change*
   - Use `submit` to push changes once all tests pass and UI checks are verified.
