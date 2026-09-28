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
fn test_source_rect_zero() {
    let sr = SourceRect::zero();
    assert_eq!(sr.x, 0.0);
    assert_eq!(sr.w, 0.0);
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

/// Issue #724 评论 5751268664 缺口1: cluster 相对 inserted 子范围分类 + clipped source rect。
fn make_cluster(byte_start: usize, byte_end: usize, x: f64, w: f64) -> LineClusterSnapshot {
    LineClusterSnapshot {
        byte_start,
        byte_end,
        source_rect: SourceRect {
            x,
            y: 0.0,
            w,
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
    }
}

#[test]
fn test_relate_to_inserted_range_disjoint() {
    let c = make_cluster(0, 3, 0.0, 30.0);
    assert!(c.relate_to_inserted_range(5, 8).is_none());
}

#[test]
fn test_relate_to_inserted_range_inside() {
    let c = make_cluster(5, 8, 50.0, 30.0);
    match c.relate_to_inserted_range(0, 10) {
        Some(ClusterInsertRelation::Inside) => {}
        other => panic!("expected Inside, got {:?}", other),
    }
}

#[test]
fn test_relate_to_inserted_range_partial_left_clip() {
    // cluster [0, 10), inserted [5, 15) → 交集 [5, 10)，左半被裁掉
    let c = make_cluster(0, 10, 0.0, 100.0);
    match c.relate_to_inserted_range(5, 15) {
        Some(ClusterInsertRelation::Partial {
            clipped_byte_start,
            clipped_byte_end,
        }) => {
            assert_eq!(clipped_byte_start, 5);
            assert_eq!(clipped_byte_end, 10);
        }
        other => panic!("expected Partial, got {:?}", other),
    }
}

#[test]
fn test_relate_to_inserted_range_partial_right_clip() {
    // cluster [5, 15), inserted [0, 10) → 交集 [5, 10)，右半被裁掉
    let c = make_cluster(5, 15, 50.0, 100.0);
    match c.relate_to_inserted_range(0, 10) {
        Some(ClusterInsertRelation::Partial {
            clipped_byte_start,
            clipped_byte_end,
        }) => {
            assert_eq!(clipped_byte_start, 5);
            assert_eq!(clipped_byte_end, 10);
        }
        other => panic!("expected Partial, got {:?}", other),
    }
}

#[test]
fn test_relate_to_inserted_range_partial_middle_clip() {
    // cluster [0, 20), inserted [5, 15) → 交集 [5, 15)，左右各裁掉 1/4
    let c = make_cluster(0, 20, 0.0, 100.0);
    match c.relate_to_inserted_range(5, 15) {
        Some(ClusterInsertRelation::Partial {
            clipped_byte_start,
            clipped_byte_end,
        }) => {
            assert_eq!(clipped_byte_start, 5);
            assert_eq!(clipped_byte_end, 15);
        }
        other => panic!("expected Partial, got {:?}", other),
    }
}
