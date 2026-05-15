1. **Update `LocalSettings` and `Models.kt`**
   - Add `autoIndentEnabled: Boolean` (default `true`) and `autoIndentWidth: Float` (default `2.0f`) to `apps/android/app/src/main/kotlin/com/xiwei/writerapp/model/Models.kt` (Already done).
   - Add `auto_indent_enabled: bool` and `auto_indent_width: f32` to `core/writer_core/src/settings/mod.rs` (Already done).

2. **Update `activity_settings.xml`**
   - Replace old `SeekBar` widgets with `com.google.android.material.slider.Slider` for Font Size, Line Spacing, and Auto Save Delay.
   - Add settings UI for `Auto Indent Enabled` (`MaterialSwitch`) and `Auto Indent Width` (`Slider`).
   - Add labels to show current values for sliders.

3. **Update `SettingsActivity.kt`**
   - Bind the new Material Sliders, listening to `addOnChangeListener` to update the labels with live values (e.g., "18sp", "1.6x", "2秒", "2字符").
   - Bind `autoIndentEnabled` and `autoIndentWidth` to the new UI components.
   - Save the values to `LocalSettings` when the user navigates back.

4. **Implement Custom `LeadingMarginSpan` in Android**
   - Create a `ParagraphIndentSpan` that implements `LeadingMarginSpan`. It will provide indentation only for the first line of each paragraph.
   - We need to handle this at the rendering level so that the actual text does not contain indentation spaces.

5. **Create `WriterEditText`**
   - Create a custom `EditText` called `WriterEditText` (or handle it in `EditorActivity` safely) that applies `ParagraphIndentSpan` to each paragraph when text changes.
   - Wait, `TextWatcher` can apply Spans without altering the actual string content! We can use a `TextWatcher` that only manipulates `Spannable` spans, keeping the underlying text clean.
   - Actually, a better approach is to set the Spans on the `Editable` text within `afterTextChanged` or apply a global `ParagraphIndentSpan` to the whole text if it's a `LeadingMarginSpan`. `LeadingMarginSpan` operates on paragraphs (separated by `\n`). We just need to add it to the entire `Spannable` and Android's text layout will automatically apply it to every paragraph!
   - No, `LeadingMarginSpan.Standard` applies to paragraphs, but to apply to ALL paragraphs, we just set one span over the whole text length!
   - Let's check how `LeadingMarginSpan` works. If applied to the entire text `(0, length)`, it affects every paragraph within that range. First line gets `first` margin, rest gets `rest` margin. This is exactly what we need!

6. **Update `EditorActivity.kt`**
   - Read `autoIndentEnabled` and `autoIndentWidth` from `SettingsRepository`.
   - If enabled, calculate the pixel width of `autoIndentWidth` characters (using `editorEditText.paint.measureText("一".repeat(width))`).
   - Apply a `LeadingMarginSpan.Standard(indentPx, 0)` to the `editorEditText.text`.
   - Ensure the span is reapplied or expanded as text changes. A `TextWatcher` can ensure the span covers `0..text.length`. Or we can set the span with `Spanned.SPAN_INCLUSIVE_INCLUSIVE` so it grows automatically.
   - Actually, just setting it once on `setText` and `SPAN_INCLUSIVE_INCLUSIVE` might be enough.
   - Let's test this behavior.

7. **Verify Pre-commit steps**
   - Check `tools/check_all.sh` to ensure Rust changes compile and pass tests.
   - Run Android build `./tools/build_android.sh` to ensure Kotlin changes compile.

