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

                // After TextArea fallback removal, WritingWorkspace must use SujianEditorItem exclusively
                if content.contains("TextArea {") {
                    eprintln!(
                        "{}: WritingWorkspace.qml must not contain TextArea {{ after fallback removal",
                        file_name
                    );
                    has_errors = true;
                }
                if content.contains("useSujianEditorItem") {
                    eprintln!(
                        "{}: WritingWorkspace.qml must not contain useSujianEditorItem after fallback removal",
                        file_name
                    );
                    has_errors = true;
                }
                if !content.contains("SujianEditorItem {") {
                    eprintln!(
                        "{}: WritingWorkspace.qml must contain SujianEditorItem {{",
                        file_name
                    );
                    has_errors = true;
                }
                if !content.contains("id: sujianEditor") {
                    eprintln!(
                        "{}: WritingWorkspace.qml must contain id: sujianEditor",
                        file_name
                    );
                    has_errors = true;
                }
                if !content.contains("targetEditorItem: sujianEditor") {
                    eprintln!(
                        "{}: WritingWorkspace.qml must contain targetEditorItem: sujianEditor",
                        file_name
                    );
                    has_errors = true;
                }
                if content.contains("visible: !root.useSujianEditorItem") {
                    eprintln!(
                        "{}: WritingWorkspace.qml must not contain visible: !root.useSujianEditorItem after fallback removal",
                        file_name
                    );
                    has_errors = true;
                }
                if content.contains("root.useSujianEditorItem") {
                    eprintln!(
                        "{}: WritingWorkspace.qml must not contain root.useSujianEditorItem after fallback removal",
                        file_name
                    );
                    has_errors = true;
                }
                if !content.contains("contentHeight:")
                    || !content.contains("sujianEditor.content_height")
                {
                    eprintln!(
                        "{}: ScrollView missing SujianEditorItem contentHeight guard",
                        file_name
                    );
                    has_errors = true;
                }
                if !content.contains("EditorWheelScroller {")
                    || !content.contains("id: editorWheelScroller")
                    || !content.contains("editorItem: sujianEditor")
                {
                    eprintln!(
                        "{}: Stable editor mode must delegate wheel physics to EditorWheelScroller",
                        file_name
                    );
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

                // SmoothCursor was the TextArea fallback cursor overlay.
                // After fallback removal, WritingWorkspace no longer needs SmoothCursor.
                // SujianEditorItem has its own cursor rectangle (sujianCursorRect).
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
                    eprintln!(
                        "{}: Editor wheel scrolling must use velocity-integrated kinetic scrolling",
                        file_name
                    );
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

            // 回归检查：SettingsDialog ScrollView 必须显式绑定 contentHeight
            if file_name == "SettingsDialog.qml" {
                // ScrollView 必须显式绑定 contentHeight: settingsColumn.implicitHeight
                if !content.contains("contentHeight: settingsColumn.implicitHeight") {
                    eprintln!("{}: SettingsDialog ScrollView 必须绑定 contentHeight: settingsColumn.implicitHeight", file_name);
                    has_errors = true;
                }
                // SmoothWheelScroller 不能是 ScrollView 的直接子项，应作为外部 overlay
                if let Some(sv_start) = content.find("ScrollView {") {
                    // 查找匹配的右大括号
                    let after_sv = &content[sv_start..];
                    let mut brace_count = 0;
                    let mut sv_end = 0;
                    let mut found_smooth_in_sv = false;
                    for (i, c) in after_sv.chars().enumerate() {
                        if c == '{' {
                            brace_count += 1;
                        } else if c == '}' {
                            brace_count -= 1;
                            if brace_count == 0 {
                                sv_end = i;
                                break;
                            }
                        }
                    }
                    if sv_end > 0 {
                        let sv_block = &after_sv[..sv_end];
                        if sv_block.contains("SmoothWheelScroller") {
                            found_smooth_in_sv = true;
                        }
                        if found_smooth_in_sv {
                            eprintln!("{}: SmoothWheelScroller 不能是 ScrollView 的直接子项——应移到外部作为 overlay", file_name);
                            has_errors = true;
                        }
                    }
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
    let main_rs = fs::read_to_string(manifest_dir.join("src/main.rs")).unwrap();
    let sujian_editor_item =
        fs::read_to_string(manifest_dir.join("src/sujian_editor_item/mod.rs")).unwrap();
    let sujian_rendering =
        fs::read_to_string(manifest_dir.join("src/sujian_editor_item/rendering.rs")).unwrap();
    let editor_controller =
        fs::read_to_string(manifest_dir.join("qml/EditorController.qml")).unwrap();
    let writing_workspace =
        fs::read_to_string(manifest_dir.join("qml/WritingWorkspace.qml")).unwrap();
    let design_tokens = fs::read_to_string(manifest_dir.join("qml/DesignTokens.qml")).unwrap();

    assert!(
        main_rs.contains("mod sujian_editor_item;") && main_rs.contains("SujianEditorItem"),
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

    // document_handler.rs has been deleted — verify it is not referenced
    assert!(
        !main_rs.contains("mod document_handler"),
        "main.rs must not contain 'mod document_handler' after fallback removal"
    );
    assert!(
        !main_rs.contains("DocumentHandler"),
        "main.rs must not reference DocumentHandler after fallback removal"
    );
    assert!(
        !main_rs.contains("EditorPage.qml"),
        "main.rs qrc must not include EditorPage.qml after fallback removal"
    );
    assert!(
        !main_rs.contains("SmoothCursor.qml"),
        "main.rs qrc must not include SmoothCursor.qml after fallback removal"
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
            && editor_controller.contains("readEditorItemPlainText")
            && editor_controller.contains("targetEditorItem.set_plain_text(content)")
            && editor_controller.contains("targetEditorItem.reload_plain_text(plain)"),
        "EditorController must prefer SujianEditorItem for load/format/save plain text"
    );
    // After DocumentHandler removal, EditorController no longer owns text_color binding.
    // Color binding is now in WritingWorkspace.qml on SujianEditorItem directly.
    assert!(
        writing_workspace.contains("text_color: editorController.colorToHex"),
        "WritingWorkspace must bind SujianEditorItem text_color via editorController.colorToHex"
    );
    assert!(
        editor_controller.contains("colorToHex"),
        "EditorController must keep colorToHex utility for SujianEditorItem color binding"
    );

    assert!(
        design_tokens.contains("property color onSurface: isDark ? \"#E2E2E5\" : \"#1A1C1E\"")
            && design_tokens
                .contains("property color textPrimary: isDark ? \"#E2E2E5\" : \"#1A1C1E\"")
            && design_tokens.contains("property color editorText: textPrimary"),
        "DesignTokens.editorText must remain the semantic editor foreground token"
    );
    assert!(
        writing_workspace.contains("SujianEditorItem {")
            && writing_workspace.contains("text_color: editorController.colorToHex"),
        "WritingWorkspace must mount SujianEditorItem as the sole editor with semantic color binding"
    );
    // Reverse checks after TextArea fallback removal
    assert!(
        !writing_workspace.contains("TextArea {"),
        "WritingWorkspace must not contain TextArea {{ after fallback removal"
    );
    assert!(
        !writing_workspace.contains("useSujianEditorItem"),
        "WritingWorkspace must not contain useSujianEditorItem after fallback removal"
    );
    assert!(
        !editor_controller.contains("targetTextArea"),
        "EditorController must not reference targetTextArea after TextArea fallback removal"
    );
    assert!(
        !editor_controller.contains("DocumentHandler"),
        "EditorController must not reference DocumentHandler after TextArea fallback removal"
    );
    assert!(
        !editor_controller.contains("useSelfRenderedEditor"),
        "EditorController must not reference useSelfRenderedEditor after fallback removal"
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
}

#[test]
fn test_no_mindmap_in_api_udl() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or(manifest_dir);
    let udl_path = workspace_root.join("core/writer_core/src/api.udl");
    if !udl_path.exists() {
        return;
    }
    let content = fs::read_to_string(&udl_path).unwrap();
    assert!(
        !content.contains("MindMap"),
        "api.udl must not contain MindMap types or methods after route consolidation"
    );
}

#[test]
fn test_no_mindmap_in_qml() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let qml_dir = manifest_dir.join("qml");
    if !qml_dir.exists() {
        return;
    }
    for entry in fs::read_dir(&qml_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("qml") {
            let content = fs::read_to_string(&path).unwrap();
            let file_name = path.file_name().unwrap().to_str().unwrap();
            assert!(
                !content.contains("mind_map") && !content.contains("mindMap"),
                "QML file {} must not reference mind_map/mindMap after route consolidation",
                file_name
            );
        }
    }
}

#[test]
fn test_no_mindmap_in_starmap_backend() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let backend_path = manifest_dir.join("src/backend/starmap_backend.rs");
    if !backend_path.exists() {
        return;
    }
    let content = fs::read_to_string(&backend_path).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("//")
            || trimmed.starts_with("#[path")
            || trimmed.starts_with("mod mind_map")
        {
            continue;
        }
        assert!(
            !trimmed.contains("mind_map_operations"),
            "starmap_backend.rs:{} must not reference mind_map_operations in non-comment code",
            idx + 1
        );
    }
}

#[test]
fn test_no_mindmap_in_android正式代码() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or(manifest_dir);
    let android_data =
        workspace_root.join("apps/android/app/src/main/kotlin/com/xiwei/sujian/data");
    let android_model =
        workspace_root.join("apps/android/app/src/main/kotlin/com/xiwei/sujian/model");

    if android_data.exists() {
        for entry in fs::read_dir(&android_data).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.file_name().unwrap().to_str() == Some("MindMapBridge.kt") {
                panic!("MindMapBridge.kt must be deleted after route consolidation");
            }
        }
    }
    if android_model.exists() {
        for entry in fs::read_dir(&android_model).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.file_name().unwrap().to_str() == Some("MindMapModels.kt") {
                panic!("MindMapModels.kt must be deleted after route consolidation");
            }
        }
    }
}

#[test]
fn test_sujian_editor_item_no_request_repaint() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mod_rs = fs::read_to_string(manifest_dir.join("src/sujian_editor_item/mod.rs")).unwrap();
    let rendering_rs =
        fs::read_to_string(manifest_dir.join("src/sujian_editor_item/rendering.rs")).unwrap();

    assert!(
        !mod_rs.contains("fn request_repaint("),
        "sujian_editor_item/mod.rs must not define fn request_repaint — use request_static_repaint or request_frame_update instead"
    );

    for (line_idx, line) in rendering_rs.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            continue;
        }
        if trimmed.contains("request_frame_update")
            && trimmed.contains("render_dirty")
            && trimmed.contains("= true")
        {
            panic!(
                "rendering.rs:{}: request_frame_update path must not set render_dirty = true — animation frames must not invalidate the static texture",
                line_idx + 1
            );
        }
    }

    for (line_idx, line) in mod_rs.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            continue;
        }
        if trimmed.contains("request_frame_update")
            && trimmed.contains("render_dirty")
            && trimmed.contains("= true")
        {
            panic!(
                "sujian_editor_item/mod.rs:{}: request_frame_update path must not set render_dirty = true — animation frames must not invalidate the static texture",
                line_idx + 1
            );
        }
    }
}

#[test]
fn test_desktop_no_envelope_json_calls() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src_dir = manifest_dir.join("src");

    let mut violations = Vec::new();

    fn check_rs_file(path: &Path, prefix: &str, violations: &mut Vec<String>) {
        let content = fs::read_to_string(path).unwrap();
        let file_name = path.file_name().unwrap().to_str().unwrap();
        for (line_idx, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") {
                continue;
            }
            if trimmed.contains("envelope_json(") {
                violations.push(format!(
                    "{}{}:{}: Found forbidden envelope_json call: {}",
                    prefix,
                    file_name,
                    line_idx + 1,
                    trimmed
                ));
            }
        }
    }

    if src_dir.exists() {
        for entry in fs::read_dir(&src_dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            check_rs_file(&path, "", &mut violations);
        }

        let backend_dir = src_dir.join("backend");
        if backend_dir.exists() {
            for entry in fs::read_dir(&backend_dir).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                    continue;
                }
                check_rs_file(&path, "backend/", &mut violations);
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Desktop must not call envelope_json — use typed DTO API + ResultEnvelope instead:\n{}",
        violations.join("\n")
    );
}

#[test]
fn test_no_textarea_fallback_in_writing_workspace() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let writing_workspace =
        fs::read_to_string(manifest_dir.join("qml/WritingWorkspace.qml")).unwrap();
    let editor_controller =
        fs::read_to_string(manifest_dir.join("qml/EditorController.qml")).unwrap();
    let main_qml = fs::read_to_string(manifest_dir.join("qml/main.qml")).unwrap();
    let app_backend = fs::read_to_string(manifest_dir.join("src/backend/app_backend.rs")).unwrap();

    assert!(
        !writing_workspace.contains("TextArea {"),
        "WritingWorkspace.qml must not contain TextArea {{ after fallback removal"
    );
    assert!(
        !writing_workspace.contains("useSujianEditorItem"),
        "WritingWorkspace.qml must not contain useSujianEditorItem after fallback removal"
    );
    assert!(
        !editor_controller.contains("targetTextArea"),
        "EditorController.qml must not contain targetTextArea after fallback removal"
    );
    assert!(
        !editor_controller.contains("DocumentHandler"),
        "EditorController.qml must not contain DocumentHandler after fallback removal"
    );
    assert!(
        !editor_controller.contains("useSelfRenderedEditor"),
        "EditorController.qml must not contain useSelfRenderedEditor after fallback removal"
    );
    assert!(
        !main_qml.contains("sujian_editor_item_enabled"),
        "main.qml must not contain sujian_editor_item_enabled after fallback removal"
    );
    assert!(
        !app_backend.contains("SUJIAN_DESKTOP_USE_SUJIAN_EDITOR"),
        "app_backend.rs must not contain SUJIAN_DESKTOP_USE_SUJIAN_EDITOR after fallback removal"
    );
}

#[test]
fn test_no_textarea_document_handler_fallback_in_qml() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let qml_dir = manifest_dir.join("qml");
    if !qml_dir.exists() {
        return;
    }

    let mut has_errors = false;

    for entry in fs::read_dir(&qml_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("qml") {
            continue;
        }
        let content = fs::read_to_string(&path).unwrap();
        let file_name = path.file_name().unwrap().to_str().unwrap();

        // No QML file should reference TextArea / DocumentHandler FALLBACK
        for (idx, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") {
                continue;
            }
            if trimmed.contains("TextArea / DocumentHandler FALLBACK")
                || trimmed.contains("TextArea FALLBACK")
                || trimmed.contains("DocumentHandler FALLBACK")
            {
                eprintln!(
                    "{}:{}: Found forbidden TextArea/DocumentHandler FALLBACK reference: {}",
                    file_name,
                    idx + 1,
                    trimmed
                );
                has_errors = true;
            }
        }
    }

    assert!(
        !has_errors,
        "QML files must not contain TextArea / DocumentHandler FALLBACK references"
    );
}

#[test]
fn test_no_fallback_files_in_qrc() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let main_rs = fs::read_to_string(manifest_dir.join("src/main.rs")).unwrap();

    assert!(
        !main_rs.contains("EditorPage.qml"),
        "main.rs qrc must not include EditorPage.qml — it has been deleted as a fallback file"
    );
    assert!(
        !main_rs.contains("SmoothCursor.qml"),
        "main.rs qrc must not include SmoothCursor.qml — it has been deleted as a fallback file"
    );
    assert!(
        !main_rs.contains("mod document_handler"),
        "main.rs must not declare mod document_handler — it has been deleted as a fallback file"
    );
}
