//! layout_snapshot.rs 的单元测试。
//!
//! 按仓库结构门禁（`tools/check_source_structure.py` 的
//! production-test-bloat 规则）从生产文件拆出，测试逻辑保持不变。
use super::*;

#[test]
fn test_layout_revision_monotonic() {
    let r1 = LayoutRevision::next();
    let r2 = LayoutRevision::next();
    assert!(r2 > r1);
}

#[test]
fn test_shaping_identity_same() {
    let a = ShapingIdentity {
        text_content_hash: 42,
        raw_font_fingerprint: "Arial:w50:s16".into(),
        glyph_indexes_hash: 100,
        cluster_glyph_count: 2,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let b = a.clone();
    assert!(a.is_same_shaping(&b));
}

#[test]
fn test_shaping_identity_different() {
    let a = ShapingIdentity {
        text_content_hash: 42,
        raw_font_fingerprint: "Arial:w50:s16".into(),
        glyph_indexes_hash: 100,
        cluster_glyph_count: 2,
        direction_rtl: false,
        format_fingerprint: 0,
    };
    let mut b = a.clone();
    b.glyph_indexes_hash = 200;
    assert!(!a.is_same_shaping(&b));
}

/// Issue #826: `PreparedLineSnapshot::stub_for_tests` 造出的行几何自洽。
///
/// 前沿遮罩与 Reflow 都按 `visual_line_top/bottom` 定位行，所以 stub 的
/// top/bottom 与 `document_origin_y` 必须一致。
#[test]
fn test_stub_for_tests_geometry_is_self_consistent() {
    let line = PreparedLineSnapshot::stub_for_tests(
        3,
        40.0,
        0,
        vec![LineClusterSnapshot {
            byte_start: 0,
            byte_end: 1,
            source_rect: SourceRect {
                x: 8.0,
                y: 0.0,
                w: 10.0,
                h: 20.0,
            },
            shaping_identity: ShapingIdentity {
                text_content_hash: 0,
                raw_font_fingerprint: String::new(),
                glyph_indexes_hash: 0,
                cluster_glyph_count: 0,
                direction_rtl: false,
                format_fingerprint: 0,
            },
        }],
    );
    assert_eq!(line.byte_start, 0);
    assert_eq!(line.byte_end, 1);
    assert_eq!(line.visual_x, 8.0);
    assert_eq!(line.visual_line_top, 40.0);
    assert_eq!(line.visual_line_bottom, 60.0);
    assert_eq!(line.document_origin_y, 40.0);
}
