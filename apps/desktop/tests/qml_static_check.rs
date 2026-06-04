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
                if !content.contains("contentHeight: Math.max(editorArea.implicitHeight, editorArea.emptyContentMinimumHeight, availableHeight)") {
                    eprintln!("{}: ScrollView missing editor contentHeight guard", file_name);
                    has_errors = true;
                }
                if content.contains("SmoothCursor {") || content.contains("id: cursorOverlay") {
                    eprintln!("{}: Stable editor mode must not instantiate SmoothCursor overlay", file_name);
                    has_errors = true;
                }
                if !content.contains("cursorVisible: activeFocus && enabled") {
                    eprintln!("{}: Stable editor mode must use the native Qt cursor", file_name);
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
                if !content.contains("Rectangle {")
                    || !content.contains("id: cursorRect")
                    || !content.contains("Item {")
                    || !content.contains("id: typingLayer")
                {
                    eprintln!(
                        "{}: SmoothCursor must split cursorRect from typingLayer",
                        file_name
                    );
                    has_errors = true;
                }
                if !content.contains("targetTextArea.leftPadding")
                    || !content.contains("targetTextArea.topPadding")
                    || !content.contains("targetTextArea.bottomPadding")
                {
                    eprintln!(
                        "{}: SmoothCursor must position against TextArea padding",
                        file_name
                    );
                    has_errors = true;
                }
                if !content.contains("targetTextArea.mapToItem(overlayItem || parent") {
                    eprintln!("{}: SmoothCursor must map TextArea cursor coordinates into overlay coordinates", file_name);
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
    let editor_controller = fs::read_to_string(manifest_dir.join("qml/EditorController.qml")).unwrap();
    let writing_workspace = fs::read_to_string(manifest_dir.join("qml/WritingWorkspace.qml")).unwrap();
    let design_tokens = fs::read_to_string(manifest_dir.join("qml/DesignTokens.qml")).unwrap();

    for token in [
        ["set", "PlainText"].concat(),
        ["set", "_plain", "_text"].concat(),
        ["apply", "_current", "_text", "_color"].concat(),
    ] {
        assert!(
            !document_handler.contains(&token),
            "DocumentHandler must not own content loading or per-cursor text color refresh: {token}"
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
        editor_controller.contains("text_color: \"#E2E2E5\"")
            && editor_controller.contains("theme_color_probe")
            && editor_controller.contains("colorToHex(editorText)=")
            && editor_controller.contains("docHandler.text_color="),
        "EditorController must keep the temporary hardcoded color bisection and log the QML color chain"
    );

    assert!(
        design_tokens.contains("property color editorText: onSurface"),
        "DesignTokens.editorText must remain the semantic editor foreground token"
    );
    assert!(
        writing_workspace.contains("color: dt ? dt.editorText : \"#E2E2E5\""),
        "WritingWorkspace TextArea must read editor text color from DesignTokens"
    );
    assert!(
        writing_workspace.contains("textFormat: TextEdit.PlainText"),
        "WritingWorkspace TextArea must own plain text display in PlainText mode"
    );
    assert!(
        editor_controller.contains("targetTextArea.text = content")
            && editor_controller.contains("targetTextArea.text = plain")
            && editor_controller.contains("docHandler.apply_format()"),
        "EditorController must load and format text through TextArea.text, then apply block formatting"
    );
}
