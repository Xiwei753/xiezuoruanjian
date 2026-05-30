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
    let forbidden_colors = ["#000000", "#111111", "#1a1c1e", "#1A1C1E", "black"];

    for entry in fs::read_dir(qml_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) == Some("qml") {
            let content = fs::read_to_string(&path).unwrap();
            let file_name = path.file_name().unwrap().to_str().unwrap();

            for (line_idx, line) in content.lines().enumerate() {
                let line_num = line_idx + 1;
                let trimmed = line.trim();

                // Ignore comments
                if trimmed.starts_with("//") {
                    continue;
                }

                // 1. Check for emojis
                for emoji in &forbidden_emojis {
                    if trimmed.contains(emoji) {
                        eprintln!("{}:{}: Found forbidden emoji '{}'", file_name, line_num, emoji);
                        has_errors = true;
                    }
                }

                // 2. Check for hardcoded dark colors
                if trimmed.contains("color:") || trimmed.contains("color :") || trimmed.contains("color=") {
                    let lower = trimmed.to_lowercase();
                    for color in &forbidden_colors {
                        let c = color.to_lowercase();
                        if lower.contains(&format!("\"{}\"", c)) || lower.contains(&format!("'{}'", c)) {
                            eprintln!("{}:{}: Found hardcoded dark color '{}'", file_name, line_num, color);
                            has_errors = true;
                        }
                    }
                }

                // 3. Check for Chinese characters not in qsTr (Heuristic)
                let has_chinese = trimmed.chars().any(|c| c >= '\u{4E00}' && c <= '\u{9FFF}');
                if has_chinese && !trimmed.contains("qsTr") {
                    // There are exceptions like property names or string concatenation not properly formatted, but we try our best.
                    // Also ignore console.log or backend.log
                    if !trimmed.contains("console.") && !trimmed.contains("debugLog") && !trimmed.contains("window.debugLog") && !trimmed.contains("logger") && !trimmed.contains("log_") {
                        eprintln!("{}:{}: Found Chinese text without qsTr(): {}", file_name, line_num, trimmed);
                        has_errors = true;
                    }
                }
                // 4. Check for syntax errors like qsTr("..."))
                let re_text = regex::Regex::new(r#"text:\s*qsTr\([^)]*\)\)"#).unwrap();
                let re_title = regex::Regex::new(r#"title:\s*qsTr\([^)]*\)\)"#).unwrap();
                let re_return = regex::Regex::new(r#"return\s+qsTr\([^)]*\)\)\s*;"#).unwrap();
                let re_arg = regex::Regex::new(r#"qsTr\([^)]*\)\)\.arg"#).unwrap();
                let re_assign = regex::Regex::new(r#"=\s*qsTr\([^)]*\)\)\s*;"#).unwrap();
                
                if re_text.is_match(trimmed) || re_title.is_match(trimmed) || re_return.is_match(trimmed) || re_arg.is_match(trimmed) || re_assign.is_match(trimmed) {
                    eprintln!("{}:{}: Found potential syntax error double parenthesis in qsTr: {}", file_name, line_num, trimmed);
                    has_errors = true;
                }
            }
        }
    }

    assert!(!has_errors, "QML static checks failed. See stderr for details.");
}
