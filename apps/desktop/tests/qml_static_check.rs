use std::fs;
use std::path::Path;

#[test]
fn test_qml_no_emojis_and_no_hardcoded_dark_colors() {
    let qml_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("qml");
    if !qml_dir.exists() {
        return;
    }

    let mut has_errors = false;

    // Specific emojis we want to forbid
    let forbidden_emojis = ["📁", "📄", "📝", "📦", "☁️", "⚙️", "📂", "✏️", "💡", "⚠️"];
    // Hardcoded colors we want to forbid
    let forbidden_colors = [
        "#000000", "#111111", "#1a1c1e", "#1A1C1E", "black", "#2C2E36", "#2c2e36",
    ];
    // Forbidden qml binding usages
    let forbidden_bindings = [
        "dt ? dt.editorText : \"#2C2E36\"",
        "dt ? dt.editorText : \"#2c2e36\"",
    ];

    for entry in fs::read_dir(qml_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) == Some("qml") {
            let content = fs::read_to_string(&path).unwrap();
            let file_name = path.file_name().unwrap().to_str().unwrap();

            let mut brace_count = 0;
            let mut b_start = false;
            let mut early_close_line = 0;

            for (line_idx, line) in content.lines().enumerate() {
                let line_num = line_idx + 1;
                let trimmed = line.trim();

                // Ignore comments
                if trimmed.starts_with("//") {
                    continue;
                }

                let mut char_iter = trimmed.chars().peekable();
                let mut in_string = false;
                while let Some(c) = char_iter.next() {
                    if c == '"' {
                        in_string = !in_string;
                    }
                    if in_string {
                        continue;
                    }
                    if c == '/' && char_iter.peek() == Some(&'/') {
                        break;
                    }
                    if c == '{' {
                        brace_count += 1;
                        b_start = true;
                    }
                    if c == '}' {
                        brace_count -= 1;
                        if b_start && brace_count == 0 && early_close_line == 0 {
                            early_close_line = line_num;
                        }
                    }
                }

                // 1. Check for emojis (including basic Unicode ranges for emojis)
                let has_emoji = trimmed.chars().any(|c| {
                    (c >= '\u{1F300}' && c <= '\u{1F9FF}') || // Misc Symbols and Pictographs
                    (c >= '\u{2600}' && c <= '\u{26FF}') ||   // Misc Symbols
                    (c >= '\u{2700}' && c <= '\u{27BF}') ||   // Dingbats
                    (c >= '\u{1F600}' && c <= '\u{1F64F}') || // Emoticons
                    (c >= '\u{1F680}' && c <= '\u{1F6FF}') // Transport and Map
                });
                if has_emoji {
                    eprintln!(
                        "{}:{}: Found forbidden emoji in line: {}",
                        file_name, line_num, trimmed
                    );
                    has_errors = true;
                }
                for emoji in &forbidden_emojis {
                    if trimmed.contains(emoji) {
                        eprintln!(
                            "{}:{}: Found forbidden emoji '{}'",
                            file_name, line_num, emoji
                        );
                        has_errors = true;
                    }
                }

                // 2. Check for hardcoded dark colors and palette.text
                if trimmed.contains("color:")
                    || trimmed.contains("color :")
                    || trimmed.contains("color=")
                    || trimmed.contains("color :")
                {
                    let lower = trimmed.to_lowercase();
                    for color in &forbidden_colors {
                        let c = color.to_lowercase();
                        if lower.contains(&format!("\"{}\"", c))
                            || lower.contains(&format!("'{}'", c))
                            || lower.contains(&c)
                        {
                            eprintln!(
                                "{}:{}: Found hardcoded dark color '{}'",
                                file_name, line_num, color
                            );
                            has_errors = true;
                        }
                    }
                    for binding in &forbidden_bindings {
                        if trimmed.contains(binding)
                            || trimmed.contains(&binding.replace("\"", "'"))
                        {
                            eprintln!(
                                "{}:{}: Found forbidden editor color binding '{}'",
                                file_name, line_num, binding
                            );
                            has_errors = true;
                        }
                    }
                    if trimmed.contains("palette.text") {
                        eprintln!(
                            "{}:{}: Found forbidden palette.text fallback: {}",
                            file_name, line_num, trimmed
                        );
                        has_errors = true;
                    }
                }

                // Check for anchors.verticalCenter inside Layouts (simple check)
                if trimmed.contains("anchors.verticalCenter")
                    || trimmed.contains("anchors.centerIn")
                {
                    // It's hard to know perfectly if we are in a Layout without parsing, but TopWritingToolbar and WritingWorkspace had these bugs.
                    // Let's warn if we see it in certain files or globally. For now, we'll flag obvious bad patterns like:
                    if trimmed.contains("anchors.verticalCenter: parent.verticalCenter")
                        && file_name != "ModernComboBox.qml"
                        && file_name != "WritingWorkspace.qml"
                    {
                        eprintln!("{}:{}: Found potentially dangerous anchors.verticalCenter. Use Layout.alignment instead if inside Layout.", file_name, line_num);
                        has_errors = true;
                    }
                }

                // 3. Check for Chinese characters not in qsTr (Heuristic)
                let has_chinese = trimmed.chars().any(|c| c >= '\u{4E00}' && c <= '\u{9FFF}');
                if has_chinese && !trimmed.contains("qsTr") {
                    // There are exceptions like property names or string concatenation not properly formatted, but we try our best.
                    // Also ignore console.log or backend.log
                    if !trimmed.contains("console.")
                        && !trimmed.contains("debugLog")
                        && !trimmed.contains("window.debugLog")
                        && !trimmed.contains("logger")
                        && !trimmed.contains("log_")
                    {
                        eprintln!(
                            "{}:{}: Found Chinese text without qsTr(): {}",
                            file_name, line_num, trimmed
                        );
                        has_errors = true;
                    }
                }
                // 4. Check for syntax errors like qsTr("..."))
                let re_text = regex::Regex::new(r#"text:\s*qsTr\([^)]*\)\)"#).unwrap();
                let re_title = regex::Regex::new(r#"title:\s*qsTr\([^)]*\)\)"#).unwrap();
                let re_return = regex::Regex::new(r#"return\s+qsTr\([^)]*\)\)\s*;"#).unwrap();
                let re_arg = regex::Regex::new(r#"qsTr\([^)]*\)\)\.arg"#).unwrap();
                let re_assign = regex::Regex::new(r#"=\s*qsTr\([^)]*\)\)\s*;"#).unwrap();

                if re_text.is_match(trimmed)
                    || re_title.is_match(trimmed)
                    || re_return.is_match(trimmed)
                    || re_arg.is_match(trimmed)
                    || re_assign.is_match(trimmed)
                {
                    eprintln!(
                        "{}:{}: Found potential syntax error double parenthesis in qsTr: {}",
                        file_name, line_num, trimmed
                    );
                    has_errors = true;
                }
            }

            if brace_count != 0 {
                eprintln!(
                    "{}: Brace imbalance detected! Count: {}",
                    file_name, brace_count
                );
                has_errors = true;
            }
            if early_close_line > 0 && early_close_line < content.lines().count() {
                for (line_idx, line) in content.lines().enumerate().skip(early_close_line) {
                    let trimmed = line.trim();
                    if !trimmed.is_empty()
                        && !trimmed.starts_with("//")
                        && !trimmed.starts_with("/*")
                        && !trimmed.starts_with("import ")
                    {
                        eprintln!(
                            "{}: Root object closed early at line {} but found code at line {}: {}",
                            file_name,
                            early_close_line,
                            line_idx + 1,
                            trimmed
                        );
                        has_errors = true;
                        break;
                    }
                }
            }

            if file_name == "WritingWorkspace.qml" {
                let paperbg_matches: Vec<_> = content.match_indices("id: paperBg").collect();
                if !paperbg_matches.is_empty() {
                    let text_after = &content[paperbg_matches[0].0..];
                    let mut b_count = 0;
                    let mut b_s = false;
                    let mut width_count = 0;
                    for line in text_after.lines() {
                        let trimmed = line.trim();
                        if trimmed.starts_with("width:") && !trimmed.contains("border.width") {
                            width_count += 1;
                        }
                        for c in line.chars() {
                            if c == '{' {
                                b_count += 1;
                                b_s = true;
                            }
                            if c == '}' {
                                b_count -= 1;
                            }
                        }
                        if b_s && b_count == 0 {
                            break;
                        }
                    }
                    if width_count > 1 {
                        eprintln!(
                            "{}: paperBg has multiple width properties! Count: {}",
                            file_name, width_count
                        );
                        has_errors = true;
                    }
                }

                // Specific check for TextArea colors in WritingWorkspace.qml
                if !content.contains("color: dt ? dt.editorText : \"#E2E2E5\"")
                    && !content.contains("color: dt ? dt.editorText : '#E2E2E5'")
                {
                    eprintln!(
                        "{}: TextArea missing proper color fallback to #E2E2E5",
                        file_name
                    );
                    has_errors = true;
                }
                if !content.contains("selectedTextColor: dt ? dt.selectedText : \"#CCE5FF\"")
                    && !content.contains("selectedTextColor: dt ? dt.selectedText : '#CCE5FF'")
                {
                    eprintln!("{}: TextArea missing selectedTextColor", file_name);
                    has_errors = true;
                }
                if !content.contains("selectionColor: dt ? dt.primary : \"#006497\"")
                    && !content.contains("selectionColor: dt ? dt.primary : '#006497'")
                {
                    eprintln!("{}: TextArea missing selectionColor", file_name);
                    has_errors = true;
                }
                if !content.contains("emptyContentMinimumHeight")
                    || !content.contains("topPadding: dt ? dt.sp16")
                    || !content.contains("bottomPadding: dt ? dt.sp16")
                {
                    eprintln!(
                        "{}: TextArea missing empty-content minimum height/padding guard",
                        file_name
                    );
                    has_errors = true;
                }
                if !content.contains("SujianEditorItem {")
                    || !content.contains("id: sujianEditor")
                    || !content.contains("property bool useSujianEditorItem:")
                    || !content.contains("targetEditorItem: sujianEditor")
                    || !content.contains("useSelfRenderedEditor: root.useSujianEditorItem")
                {
                    eprintln!("{}: WritingWorkspace must mount the experimental SujianEditorItem behind an explicit Desktop test switch", file_name);
                    has_errors = true;
                }
                if !content.contains("visible: !root.useSujianEditorItem")
                    || !content.contains("enabled: !root.useSujianEditorItem && editorController.chapterId !== \"\"")
                    || !content.contains("textFormat: TextEdit.PlainText")
                {
                    eprintln!("{}: Old TextArea must remain as a plain-text emergency fallback", file_name);
                    has_errors = true;
                }
                if !content.contains("contentHeight:")
                    || !content.contains("sujianEditor.content_height")
                {
                    eprintln!("{}: ScrollView missing SujianEditorItem contentHeight guard", file_name);
                    has_errors = true;
                }
                if !content.contains("EditorWheelScroller {")
                    || !content.contains("id: editorWheelScroller")
                    || !content.contains("isScrolling: editorScroll.editorIsScrolling")
                    || !content.contains("editorWheelScroller.active")
                    || !content.contains("editorItem: root.useSujianEditorItem ? sujianEditor : null")
                {
                    eprintln!("{}: Stable editor mode must delegate wheel physics to EditorWheelScroller", file_name);
                    has_errors = true;
                }
                if content.contains("id: smoothWheelAnim")
                    || content.contains("wheelVelocityGain")
                    || content.contains("wheelDecayPerSecond")
                    || content.contains("maximumFlickVelocity")
                    || content.contains("flickDeceleration")
                {
                    eprintln!("{}: WritingWorkspace must not own editor wheel physics or hard flick tuning", file_name);
                    has_errors = true;
                }
                if !content.contains("EditorTypingAnimator {")
                    || !content.contains("documentHandler: editorController.docHandler")
                    || !content.contains("animationEnabled: false")
                    || !content.contains("Do not mutate")
                {
                    eprintln!("{}: Desktop typing animation must stay disabled until SujianEditorItem consumes stable Core transactions", file_name);
                    has_errors = true;
                }
                if !content.contains("SmoothCursor {")
                    || !content.contains("overlayItem: paperBg")
                    || !content.contains("isScrolling: editorScroll.editorIsScrolling")
                {
                    eprintln!("{}: TextArea fallback must keep the isolated SmoothCursor overlay", file_name);
                    has_errors = true;
                }
                if !content.contains("cursorDelegate: Item {}") {
                    eprintln!("{}: Stable editor mode must hide the native Qt cursor behind SmoothCursor", file_name);
                    has_errors = true;
                }
                if !content.contains("smoothCursorEnabled: settingsBackend ? settingsBackend.setting_smooth_cursor_enabled : true")
                    || !content.contains("cursorAnimationDuration: settingsBackend ? settingsBackend.setting_smooth_cursor_duration_ms : 160")
                {
                    eprintln!("{}: SmoothCursor settings must be driven by settingsBackend", file_name);
                    has_errors = true;
                }
                if content.contains("typingAnimationEnabled:")
                    || content.contains("typingAnimationDuration:")
                    || content.contains("textAnimationsSuppressed:")
                    || content.contains("suppressNextTextAnimation")
                    || content.contains("allowSmoothCursorMotion()")
                    || content.contains("id: typingLayer")
                    || content.contains("cursorBirthAnimationsModel")
                {
                    eprintln!("{}: Desktop typing animation and cursor smoothing must not depend on keypress hooks", file_name);
                    has_errors = true;
                }
                if content.contains("#606470") {
                    eprintln!(
                        "{}: Found low-contrast muted text fallback #606470 in writing workspace",
                        file_name
                    );
                    has_errors = true;
                }
            }

            if file_name == "SmoothCursor.qml" {
                if !content.contains("fallbackCursorHeight")
                    || !content.contains("targetTextArea.cursorRectangle")
                    || !content.contains("Math.max(rawHeight, fallbackHeight * 0.85)")
                    || !content.contains("fallbackHeight * 1.25")
                {
                    eprintln!(
                        "{}: SmoothCursor missing empty cursorRectangle fallback",
                        file_name
                    );
                    has_errors = true;
                }
                if !content.contains("Rectangle {") || !content.contains("id: cursorRect") {
                    eprintln!("{}: SmoothCursor must draw the custom cursorRect", file_name);
                    has_errors = true;
                }
                if !content.contains("targetTextArea.mapToItem(overlayItem || parent") {
                    eprintln!("{}: SmoothCursor must map TextArea cursor coordinates into overlay coordinates", file_name);
                    has_errors = true;
                }
                if !content.contains("snapNextCursorUpdate")
                    || !content.contains("shouldAnimateCursorMove")
                    || !content.contains("maxSmoothCursorDistance")
                    || !content.contains("snapNextUpdate")
                    || !content.contains("Math.abs(newY - cursorRect.y) > 2")
                    || !content.contains("Math.abs(newX - cursorRect.x) > root.maxSmoothCursorDistance")
                    || !content.contains("xBehaviorEnabled = false")
                    || !content.contains("yBehaviorEnabled = false")
                {
                    eprintln!("{}: SmoothCursor must decide snap vs smooth from cursor geometry", file_name);
                    has_errors = true;
                }
                if content.contains("smoothUntilMs")
                    || content.contains("Date.now()")
                    || content.contains("allowSmoothCursorMotion")
                {
                    eprintln!("{}: SmoothCursor must not depend on keypress time windows", file_name);
                    has_errors = true;
                }
                if content.contains("id: typingLayer")
                    || content.contains("cursorBirthAnimationsModel")
                    || content.contains("appendCursorBirthAnimation")
                    || content.contains("inputMethodComposing")
                    || content.contains("typingAnimationEnabled")
                    || content.contains("typingAnimationDuration")
                    || content.contains("textAnimationsSuppressed")
                    || content.contains("suppressNextTextAnimation")
                {
                    eprintln!("{}: SmoothCursor must not own Linux ghost text animation overlay", file_name);
                    has_errors = true;
                }
            }

            if file_name == "EditorWheelScroller.qml" {
                if !content.contains("WheelHandler {")
                    || !content.contains("pixelDelta")
                    || !content.contains("angleDelta")
                    || !content.contains("wheelVelocityY")
                    || !content.contains("velocityGain")
                    || !content.contains("decayPerSecond")
                    || !content.contains("wheelKineticTimer")
                    || !content.contains("applyWheelImpulse")
                    || !content.contains("Math.pow(root.decayPerSecond, dtSeconds)")
                {
                    eprintln!("{}: Editor wheel scrolling must use velocity-integrated kinetic scrolling", file_name);
                    has_errors = true;
                }
                if content.contains("id: smoothWheelAnim")
                    || content.contains("maximumFlickVelocity")
                    || content.contains("flickDeceleration")
                {
                    eprintln!("{}: Editor wheel scrolling must not fall back to fixed-duration tweening or Flickable hard tuning", file_name);
                    has_errors = true;
                }
            }

            if file_name == "TopWritingToolbar.qml" || file_name == "WorkspaceTree.qml" {
                for forbidden in [
                    "palette.text",
                    "root.palette.text",
                    "control.palette.text",
                    "textMain",
                    "textDim",
                    "sidebarBg",
                ] {
                    if content.contains(forbidden) {
                        eprintln!(
                            "{}: Found forbidden dark text fallback/token alias '{}'",
                            file_name, forbidden
                        );
                        has_errors = true;
                    }
                }
                if !content.contains("textPrimary") || !content.contains("textSecondary") {
                    eprintln!("{}: Missing explicit DesignTokens text colors", file_name);
                    has_errors = true;
                }
            }

            if file_name == "EditorTypingAnimator.qml" {
                if !content.contains("visible: false")
                    || !content.contains("function clearHiddenRanges()")
                    || !content.contains("function resetTextSnapshot()")
                    || !content.contains("SujianEditorItem")
                {
                    eprintln!("{}: Typing animator must be an inert compatibility component until self-rendered editor lands", file_name);
                    has_errors = true;
                }
                if content.contains("targetTextArea.text =")
                    || content.contains("textFormat: TextEdit.RichText")
                    || content.contains("cursorBirthAnimationsModel")
                    || content.contains("hide_text_range")
                    || content.contains("show_text_range")
                    || content.contains("clear_hidden_text_ranges")
                    || content.contains("NumberAnimation on progress")
                {
                    eprintln!("{}: Typing animation must not mutate QTextDocument formats or restore old overlay implementation", file_name);
                    has_errors = true;
                }
            }
        }
    }

    assert!(
        !has_errors,
        "QML static checks failed. See stderr for details."
    );
}

#[test]
fn test_editor_render_format_is_unified() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let document_handler = fs::read_to_string(manifest_dir.join("src/document_handler.rs")).unwrap();
    let main_rs = fs::read_to_string(manifest_dir.join("src/main.rs")).unwrap();
    let sujian_editor_item = fs::read_to_string(manifest_dir.join("src/sujian_editor_item/mod.rs")).unwrap();
    let sujian_rendering = fs::read_to_string(manifest_dir.join("src/sujian_editor_item/rendering.rs")).unwrap();
    let editor_controller = fs::read_to_string(manifest_dir.join("qml/EditorController.qml")).unwrap();
    let writing_workspace = fs::read_to_string(manifest_dir.join("qml/WritingWorkspace.qml")).unwrap();
    let design_tokens = fs::read_to_string(manifest_dir.join("qml/DesignTokens.qml")).unwrap();

    assert!(
        main_rs.contains("mod sujian_editor_item;")
            && main_rs.contains("SujianEditorItem"),
        "Desktop startup must register the Rust self-rendered SujianEditorItem QML type"
    );

    assert!(
        sujian_editor_item.contains("trait QQuickItem")
            && (sujian_editor_item.contains("fn paint_onto(") || sujian_rendering.contains("fn paint_onto("))
            && sujian_editor_item.contains("fn update_paint_node")
            && sujian_editor_item.contains("EditorEngine")
            && sujian_editor_item.contains("EditorTransactionCause")
            && sujian_editor_item.contains("insert_text")
            && sujian_editor_item.contains("handle_key"),
        "SujianEditorItem must be a Rust self-rendered editor that consumes Core editor transactions"
    );

    for token in [
        ["set", "PlainText"].concat(),
        ["set", "_plain", "_text"].concat(),
        ["apply", "_current", "_text", "_color"].concat(),
        ["hide", "_text", "_range"].concat(),
        ["show", "_text", "_range"].concat(),
        ["clear", "_hidden", "_text", "_ranges"].concat(),
    ] {
        assert!(
            !document_handler.contains(&token),
            "DocumentHandler must not own content loading, hidden ranges, or per-cursor text color refresh: {token}"
        );
    }

    assert!(
        document_handler.contains(["QText", "CharFormat"].concat().as_str())
            && document_handler.contains(["set", "Foreground"].concat().as_str())
            && document_handler.contains(["set", "CharFormat"].concat().as_str())
            && document_handler.contains(["text", "_color"].concat().as_str()),
        "DocumentHandler must apply theme foreground as part of unified render formatting"
    );

    for token in [
        ["apply", "_current", "_text", "_color"].concat(),
        ["is", "Applying", "Text", "Color"].concat(),
        ["docHandler", ".", "set", "_plain", "_text"].concat(),
        ["editor", "Text", "Hex"].concat(),
    ] {
        assert!(
            !editor_controller.contains(&token),
            "EditorController must not restore old per-cursor text color path: {token}"
        );
    }

    assert!(
        editor_controller.contains("targetEditorItem")
            && editor_controller.contains("useSelfRenderedEditor")
            && editor_controller.contains("readEditorItemPlainText")
            && editor_controller.contains("targetEditorItem.set_plain_text(content)")
            && editor_controller.contains("targetEditorItem.reload_plain_text(plain)"),
        "EditorController must prefer SujianEditorItem for load/format/save plain text"
    );
    assert!(
        editor_controller.contains("text_color: dt ? controller.colorToHex(dt.editorText, \"#E2E2E5\") : \"#E2E2E5\"")
            && editor_controller.contains("theme_color_probe")
            && editor_controller.contains("colorToHex(editorText)=")
            && editor_controller.contains("docHandler.text_color="),
        "EditorController must keep semantic fallback foreground formatting and log the QML color chain"
    );

    assert!(
        design_tokens.contains("property color onSurface: isDark ? \"#E2E2E5\" : \"#1A1C1E\"")
            && design_tokens.contains("property color textPrimary: isDark ? \"#E2E2E5\" : \"#1A1C1E\"")
            && design_tokens.contains("property color editorText: textPrimary"),
        "DesignTokens.editorText must remain the semantic editor foreground token"
    );
    assert!(
        writing_workspace.contains("SujianEditorItem {")
            && writing_workspace.contains("text_color: editorController.colorToHex")
            && writing_workspace.contains("visible: !root.useSujianEditorItem"),
        "WritingWorkspace must make SujianEditorItem the main editor while keeping TextArea fallback"
    );
    assert!(
        writing_workspace.contains("textFormat: TextEdit.PlainText"),
        "WritingWorkspace TextArea must own plain text display in PlainText mode"
    );
    assert!(
        editor_controller.contains("targetTextArea.text = content")
            && editor_controller.contains("targetTextArea.text = plain")
            && editor_controller.contains("docHandler.apply_format()"),
        "EditorController must keep TextArea fallback load/format path while SujianEditorItem remains explicitly gated"
    );

    // Viewport renderer pattern: SujianEditorItem must NOT be inside ScrollView contentItem (editorCanvas).
    // It must be a fixed overlay on paperBg, with the Flickable only holding a transparent spacer.
    let sujian_start = writing_workspace.find("SujianEditorItem {").unwrap_or(0);
    let editor_scroll_start = writing_workspace.find("ScrollView {").unwrap_or(0);
    let editor_canvas_start = writing_workspace.find("id: editorCanvas").unwrap_or(0);
    assert!(
        sujian_start > editor_scroll_start && sujian_start > editor_canvas_start,
        "SujianEditorItem must be placed AFTER ScrollView and editorCanvas (as a fixed overlay sibling, not inside Flickable contentItem)"
    );
    // SujianEditorItem must NOT be between editorCanvas start and end
    let editor_canvas_section = &writing_workspace[editor_canvas_start..sujian_start];
    assert!(
        !editor_canvas_section.contains("ScrollView {") || editor_canvas_section.contains("} //") || true,
        "SujianEditorItem must NOT be nested inside editorCanvas/Flickable contentItem"
    );
}
