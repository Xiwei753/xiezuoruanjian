use super::*;

fn record(text: &str, context: &str) -> SyncedCorrectionRecord {
    SyncedCorrectionRecord {
        schema_version: SCHEMA_VERSION,
        project_id: "project-a".to_string(),
        chapter_id: "chapter-1".to_string(),
        mode: MODE_MODEL.to_string(),
        scanner_stable: "stable-a".to_string(),
        model_identity: "model-a".to_string(),
        lexicon_identity: "lexicon-a".to_string(),
        rules_identity: "rule-a".to_string(),
        text_sha256: text.to_string(),
        context_sha256: context.to_string(),
        suggestions: Vec::new(),
        checked: true,
    }
}

#[test]
fn record_key_is_stable_and_isolated() {
    let empty_payload = payload_digest(&[]);
    let base = record_key("p", "c", MODE_MODEL, "s", "t", "ctx", &empty_payload);
    assert_eq!(base, record_key("p", "c", MODE_MODEL, "s", "t", "ctx", &empty_payload));
    assert_eq!(base.len(), 64);
    assert!(base
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    for other in [
        record_key("p2", "c", MODE_MODEL, "s", "t", "ctx", &empty_payload),
        record_key("p", "c2", MODE_MODEL, "s", "t", "ctx", &empty_payload),
        record_key("p", "c", MODE_RULES, "s", "t", "ctx", &empty_payload),
        record_key("p", "c", MODE_MODEL, "s2", "t", "ctx", &empty_payload),
        record_key("p", "c", MODE_MODEL, "s", "t2", "ctx", &empty_payload),
        record_key("p", "c", MODE_MODEL, "s", "t", "ctx2", &empty_payload),
    ] {
        assert_ne!(base, other);
    }
}

#[test]
fn write_is_idempotent_and_empty_suggestions_round_trip() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let record = record("t", "ctx");
    assert!(write_record(root, &record).unwrap());
    let path = root
        .join("correction/results/chapter-1")
        .join(format!("{}.json", record.record_key()));
    assert!(
        path.is_file(),
        "layout must stay correction/results/<chapter>/<key>.json"
    );
    let first = std::fs::read_to_string(&path).unwrap();
    assert!(
        !write_record(root, &record).unwrap(),
        "equivalent record must not rewrite"
    );
    assert_eq!(first, std::fs::read_to_string(&path).unwrap());
    let loaded = load_records(root, "project-a", "chapter-1", MODE_MODEL, "stable-a")
        .unwrap()
        .records;
    assert_eq!(loaded.len(), 1);
    assert!(loaded[0].suggestions.is_empty() && loaded[0].checked);
    assert!(loaded[0].text_sha256 == "t" && loaded[0].context_sha256 == "ctx");
}

#[test]
fn load_keeps_valid_records_and_reports_bad_files() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let dir = root.join("correction/results/chapter-1");
    std::fs::create_dir_all(&dir).unwrap();
    let broken_path = dir.join("broken.json");
    std::fs::write(&broken_path, "{not json").unwrap();
    let mut wrong_mode = record("t", "ctx");
    wrong_mode.mode = MODE_RULES.to_string();
    std::fs::write(
        dir.join("mode.json"),
        serde_json::to_string(&wrong_mode).unwrap(),
    )
    .unwrap();
    let mut wrong_version = record("t2", "ctx");
    wrong_version.schema_version = SCHEMA_VERSION + 1;
    std::fs::write(
        dir.join("version.json"),
        serde_json::to_string(&wrong_version).unwrap(),
    )
    .unwrap();
    let only_bad = load_records(root, "project-a", "chapter-1", MODE_MODEL, "stable-a").unwrap();
    assert!(only_bad.records.is_empty());
    assert_eq!(only_bad.failures.len(), 1);
    assert_eq!(only_bad.failures[0].path, broken_path);
    assert!(write_record(root, &record("t3", "ctx")).unwrap());
    // 一个坏文件只报告它自己：同章有效记录仍要读出，不能丢掉整章。
    let loaded = load_records(root, "project-a", "chapter-1", MODE_MODEL, "stable-a").unwrap();
    assert_eq!(loaded.failures.len(), 1);
    assert_eq!(loaded.failures[0].path, broken_path);
    assert_eq!(loaded.records.len(), 1);
    assert_eq!(loaded.records[0].text_sha256, "t3");
    // 身份不匹配仍是正常缓存失效，静默跳过，不算损坏。
    assert!(load_records(root, "project-a", "chapter-1", MODE_RULES, "stable-a")
        .unwrap()
        .records
        .is_empty());
    assert!(load_records(root, "project-a", "chapter-1", MODE_MODEL, "stable-b")
        .unwrap()
        .records
        .is_empty());
    // 非法章节 id 不产生目录，按“没有可用记录”返回空；目录整体不可读仍是 Err。
    let invalid = load_records(root, "project-a", "", MODE_MODEL, "stable-a").unwrap();
    assert!(invalid.records.is_empty() && invalid.failures.is_empty());
    let missing = load_records(root, "project-a", "missing-chapter", MODE_MODEL, "stable-a")
        .expect("missing chapter dir is an empty result");
    assert!(missing.records.is_empty());
}
