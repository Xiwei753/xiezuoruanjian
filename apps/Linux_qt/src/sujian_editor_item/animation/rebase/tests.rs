
    use super::*;
    use super::super::coordinator::LinuxEditorAnimationCoordinator;
    use crate::sujian_editor_item::animated_slice::{AnimatedSlice, AnimatedSliceKind};
    use crate::sujian_editor_item::edit_motion::CursorRect;
    use crate::sujian_editor_item::layout_snapshot::{
        EditorLayoutSnapshot, LineSnapshotId, ShapingIdentity, SourceRect,
    };
    use crate::sujian_editor_item::layout_revision::LayoutRevision;
    use crate::sujian_editor_item::render_plan::{
        CoordinatedMotionFrame, CursorRenderState, SampledCaretFrame,
    };
    use crate::sujian_editor_item::animation::cursor_motion::sample_coordinated_cursor_rect_at;
    use crate::sujian_editor_item::animation::transaction_builder::build_delete_conceal_slices;
    use crate::sujian_editor_item::animation::{
        PreparedCursorVisualTrack, PreparedTextVisualTransaction, PreparedVisualUnit,
        RebaseFrame, TextVisualOperationKind, TextVisualTransactionState,
        TransactionTimeline, VisualUnitTiming,
    };
    use crate::sujian_editor_item::animation_mode::AnimationMode;
    use crate::sujian_editor_item::transaction_key::VisualTransactionKey;
    use writer_core::editor::Utf8ByteOffset;
    use std::time::Duration;

    /// 构造不带时间线的 `RebaseFrame`，用于只验证三层匹配策略的用例。
    fn rebase_frame(
        byte_start: usize,
        byte_end: usize,
        x: f64,
        y: f64,
        opacity: f64,
        shaping_identity: Option<ShapingIdentity>,
        visible_fraction: f64,
    ) -> RebaseFrame {
        RebaseFrame {
            byte_start,
            byte_end,
            x,
            y,
            opacity,
            shaping_identity,
            visible_fraction,
            sampled_at: Instant::now(),
            remaining_duration_ms: 0,
        }
    }

    /// `match_rebase_frames` 现在作用在视觉单元上（Issue #690 评论 5675007226 步骤 3）。
    fn wrap_units(slices: Vec<AnimatedSlice>) -> Vec<PreparedVisualUnit> {
        slices
            .into_iter()
            .map(|s| PreparedVisualUnit::wrap(s, 100))
            .collect()
    }

    fn make_test_snapshot(
        virtual_text: &str,
        line_clusters: Vec<(usize, usize, f64, f64, ShapingIdentity)>,
    ) -> EditorLayoutSnapshot {
        use crate::editor::layout::{CaretAffinity, LayoutSnapshot, VisualLine};
        use crate::sujian_editor_item::layout_snapshot::{
            LineClusterSnapshot, PreparedLineSnapshot,
        };
        let clusters: Vec<LineClusterSnapshot> = line_clusters
            .iter()
            .map(|(bs, be, x, _y, sid)| LineClusterSnapshot {
                byte_start: *bs,
                byte_end: *be,
                source_rect: SourceRect {
                    x: *x,
                    y: 0.0,
                    w: (*be - *bs) as f64 * 10.0,
                    h: 20.0,
                },
                shaping_identity: sid.clone(),
            })
            .collect();
        let line = PreparedLineSnapshot {
            id: LineSnapshotId::new(1, 0, 0),
            image: None,
            clusters,
            document_origin_y: 0.0,
            dpr: 1.0,
            byte_start: line_clusters.first().map(|c| c.0).unwrap_or(0),
            byte_end: line_clusters.last().map(|c| c.1).unwrap_or(0),
            visual_x: 0.0,
            visual_line_id: 0,
            visual_line_top: 0.0,
            visual_line_bottom: 20.0,
            cache_slot: 0,
            qtextline_idx: 0,
            // Issue #724 评论 5752140048 问题 4a: 测试用段落起始偏移 0。
            paragraph_document_byte_start: 0,
        };
        let layout_snapshot = LayoutSnapshot {
            text_revision: 0,
            text_ptr: 0,
            text_len: virtual_text.len(),
            width: 800.0,
            font_size: 16.0,
            font_family: "sans-serif".to_string(),
            line_spacing: 1.5,
            text_indent: 0.0,
            padding: 0.0,
            lines: vec![VisualLine {
                id: 0,
                byte_start: line.byte_start,
                byte_end: line.byte_end,
                qchar_start: 0,
                qchar_end: 0,
                hard_break: false,
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 20.0,
                para_text: virtual_text.to_string(),
                para_start: 0,
                qtextline_idx: 0,
                para_qchar_start: 0,
                para_qchar_end: 0,
                line_wrap_width: 800.0,
                line_indent_x: 0.0,
                para_indent: 0.0,
                x_end_trailing: 800.0,
                qt_ascent: 16.0,
                qt_descent: 4.0,
                cache_slot: 0,
            }],
            layout_generation: 0,
        };
        EditorLayoutSnapshot::new(
            layout_snapshot,
            vec![line],
            None,
            None,
            CaretAffinity::Downstream,
        )
        .with_virtual_text(virtual_text.to_string())
    }

    fn reveal_slice(byte_start: usize, byte_end: usize, x: f64, w: f64) -> AnimatedSlice {
        AnimatedSlice::insert_reveal(
            VisualTransactionKey::new(1, 1),
            LineSnapshotId::new(1, 0, 0),
            SourceRect {
                x: 0.0,
                y: 0.0,
                w,
                h: 20.0,
            },
            SourceRect {
                x,
                y: 0.0,
                w,
                h: 20.0,
            },
            x,
            0.0,
            byte_start,
            byte_end,
            None,
            None,
        )
    }

    fn reflow_slice(byte_start: usize, byte_end: usize, from_x: f64, to_x: f64) -> AnimatedSlice {
        AnimatedSlice::reflow_move(
            VisualTransactionKey::new(1, 1),
            LineSnapshotId::new(1, 0, 0),
            SourceRect {
                x: 0.0,
                y: 0.0,
                w: 30.0,
                h: 20.0,
            },
            SourceRect {
                x: from_x,
                y: 0.0,
                w: 30.0,
                h: 20.0,
            },
            LineSnapshotId::new(1, 0, 0),
            SourceRect {
                x: 0.0,
                y: 0.0,
                w: 30.0,
                h: 20.0,
            },
            SourceRect {
                x: to_x,
                y: 0.0,
                w: 30.0,
                h: 20.0,
            },
            byte_start,
            byte_end,
            None,
        )
    }

    fn caret(x: f64) -> CursorRect {
        CursorRect {
            x,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        }
    }

    /// 已经演进了 `elapsed_ms` 的视觉单元（单元自己的时间线，与事务 timeline 无关）。
    fn elapsed_unit(
        slice: AnimatedSlice,
        elapsed_ms: u64,
        duration_ms: u64,
        now: Instant,
    ) -> PreparedVisualUnit {
        let mut unit = PreparedVisualUnit::wrap(slice, duration_ms);
        // Issue #727 约束 2: 通过 VisualUnitTiming 设置 started_at / start_fraction。
        // CaretDriven unit 无 started_at，通过 start_fraction 模拟已吐/吞比例。
        // Timed unit 通过 started_at 设置独立时间线。
        let fraction = if duration_ms > 0 {
            (elapsed_ms as f64 / duration_ms as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        match &mut unit.timing {
            VisualUnitTiming::Timed { started_at, .. } => {
                *started_at = Some(now - Duration::from_millis(elapsed_ms));
            }
            VisualUnitTiming::CaretDriven { .. } => {
                // Issue #727 约束 2: CaretDriven unit 的 visible_fraction 从 caret track
                // progress 推导（start + (target - start) * ease_out_quad(progress)），
                // 不需要通过 start_fraction 模拟已演进状态。
                // start_fraction 保持 fresh unit 的初始值（0 for InsertReveal, 1 for DeleteConceal）。
                // 测试中 caret track 的 started_at 由 rendering_tx 设置，反映已演进状态。
            }
        }
        unit
    }

    /// 手工装配一笔处于 Rendering 的正文事务。事务 timeline 从 `tx_elapsed_ms` 起算，
    /// 与单元各自的 `elapsed_ms` 故意取不同值，用来验证两者不再互相顶替。
    /// Issue #727 约束 3: 自动创建 `cursor_visual_track`，使 CaretDriven unit 可以
    /// 从 caret track progress 推导可见比例。`started_at` 与事务 timeline 同步。
    fn rendering_tx(
        key: VisualTransactionKey,
        operation_kind: TextVisualOperationKind,
        units: Vec<PreparedVisualUnit>,
        old_cursor: CursorRect,
        new_cursor: CursorRect,
        now: Instant,
        tx_elapsed_ms: u64,
    ) -> PreparedTextVisualTransaction {
        let mut timeline = TransactionTimeline::new(100);
        timeline.rendering_started_at = Some(now - Duration::from_millis(tx_elapsed_ms));
        // Issue #727 约束 3: CaretDriven unit 依赖 caret motion track。
        // 测试辅助函数自动创建 cursor_visual_track，使 CaretDriven unit 可以生成 glyph。
        let cursor_visual_track = Some(PreparedCursorVisualTrack {
            from: old_cursor.clone(),
            to: new_cursor.clone(),
            from_visual_line_id: None,
            to_visual_line_id: None,
            from_line_top: 0.0,
            from_line_bottom: 20.0,
            to_line_top: 0.0,
            to_line_bottom: 20.0,
            started_at: Some(now - Duration::from_millis(tx_elapsed_ms)),
            duration_ms: 100,
            pause_start: None,
        });
        PreparedTextVisualTransaction {
            key,
            state: TextVisualTransactionState::Rendering,
            operation_kind,
            timeline,
            units,
            old_cursor_rect: Some(old_cursor),
            new_cursor_rect: Some(new_cursor),
            cursor_visual_track,
            cancel_reason: None,
            texture_prepared: true,
            old_snapshot: None,
            new_snapshot: None,
            cursor_owner_epoch: 0,
            caret_motion_retired: false,
            visual_affected_byte_range_old: None,
            visual_affected_byte_range_new: None,
            layout_basis_revision: LayoutRevision::initial(),
        }
    }

#[test]
    fn test_rebase_uses_offset_map_and_shaping_identity() {
        use writer_core::editor::{OffsetMapEntry, OffsetMapKind};
        let sid_a = ShapingIdentity {
            text_content_hash: 1,
            raw_font_fingerprint: "font_a".to_string(),
            glyph_indexes_hash: 10,
            cluster_glyph_count: 3,
            direction_rtl: false,
            format_fingerprint: 100,
        };
        let sid_b = ShapingIdentity {
            text_content_hash: 2,
            raw_font_fingerprint: "font_b".to_string(),
            glyph_indexes_hash: 20,
            cluster_glyph_count: 2,
            direction_rtl: false,
            format_fingerprint: 200,
        };

        let rebase_frames: Vec<RebaseFrame> = vec![
            rebase_frame(10, 20, 100.0, 200.0, 0.5, Some(sid_a.clone()), 0.0),
            rebase_frame(30, 40, 150.0, 250.0, 0.7, Some(sid_b.clone()), 0.0),
        ];

        let slices = vec![
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 1),
                LineSnapshotId::new(1, 0, 0),
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                50,
                60,
                Some(sid_a.clone()),
                None,
            ),
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 2),
                LineSnapshotId::new(1, 0, 1),
                SourceRect {
                    x: 0.0,
                    y: 20.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 20.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                70,
                80,
                Some(sid_b.clone()),
                None,
            ),
        ];

        let offset_map = OffsetMap {
            entries: vec![OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::default(),
                new_byte_offset: Utf8ByteOffset::default(),
                length: 200,
                kind: OffsetMapKind::Identity,
            }],
        };
        let mut units = wrap_units(slices);
        match_rebase_frames(&rebase_frames, &mut units, &offset_map);

        assert!((units[0].slice.from_document_rect.x - 0.0).abs() < 0.01);
        assert!((units[1].slice.from_document_rect.x - 0.0).abs() < 0.01);
    }

#[test]
    fn test_rebase_tier3_closest_position_match_with_duplicate_shaping() {
        use writer_core::editor::{OffsetMapEntry, OffsetMapKind};
        let sid_dup = ShapingIdentity {
            text_content_hash: 99,
            raw_font_fingerprint: "font_x".to_string(),
            glyph_indexes_hash: 50,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 500,
        };

        let rebase_frames: Vec<RebaseFrame> = vec![rebase_frame(
            10,
            30,
            10.0,
            100.0,
            0.3,
            Some(sid_dup.clone()),
            0.3,
        )];

        let slices = vec![
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 1),
                LineSnapshotId::new(1, 0, 0),
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                11,
                15,
                Some(sid_dup.clone()),
                None,
            ),
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 2),
                LineSnapshotId::new(1, 0, 1),
                SourceRect {
                    x: 0.0,
                    y: 20.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 20.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                18,
                22,
                Some(sid_dup.clone()),
                None,
            ),
        ];

        let offset_map = OffsetMap {
            entries: vec![OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::default(),
                new_byte_offset: Utf8ByteOffset::default(),
                length: 200,
                kind: OffsetMapKind::Identity,
            }],
        };
        let mut units = wrap_units(slices);
        match_rebase_frames(&rebase_frames, &mut units, &offset_map);

        let mapped_center = 20i64;
        let center_0 = (11 + 15) as i64 / 2;
        let center_1 = (18 + 22) as i64 / 2;
        let dist_0 = (center_0 - mapped_center).abs();
        let dist_1 = (center_1 - mapped_center).abs();
        assert!(
            dist_1 < dist_0,
            "test setup: slice 1 (dist={}) should be closer than slice 0 (dist={})",
            dist_1,
            dist_0
        );
        assert!(
            (units[1].slice.start_fraction - 0.3).abs() < 0.01,
            "slice 1 (center={}, abs dist={}) should match rebase frame, got start_fraction={}",
            center_1,
            dist_1,
            units[1].slice.start_fraction
        );
        assert!(
            (units[0].slice.start_fraction - 0.0).abs() < 0.01,
            "slice 0 (center={}, abs dist={}) should NOT be matched, got start_fraction={}",
            center_0,
            dist_0,
            units[0].slice.start_fraction
        );
    }

#[test]
    fn test_rebase_tier3_absolute_distance_not_signed() {
        use writer_core::editor::{OffsetMapEntry, OffsetMapKind};
        let sid_dup = ShapingIdentity {
            text_content_hash: 99,
            raw_font_fingerprint: "font_x".to_string(),
            glyph_indexes_hash: 50,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 500,
        };

        let rebase_frames: Vec<RebaseFrame> = vec![rebase_frame(
            10,
            30,
            10.0,
            100.0,
            0.3,
            Some(sid_dup.clone()),
            0.3,
        )];

        let slices = vec![
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 1),
                LineSnapshotId::new(1, 0, 0),
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                11,
                15,
                Some(sid_dup.clone()),
                None,
            ),
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 2),
                LineSnapshotId::new(1, 0, 1),
                SourceRect {
                    x: 0.0,
                    y: 20.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                22,
                26,
                Some(sid_dup.clone()),
                None,
            ),
        ];

        let offset_map = OffsetMap {
            entries: vec![OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::default(),
                new_byte_offset: Utf8ByteOffset::default(),
                length: 200,
                kind: OffsetMapKind::Identity,
            }],
        };
        let mut units = wrap_units(slices);
        match_rebase_frames(&rebase_frames, &mut units, &offset_map);

        let mapped_center = 20i64;
        let center_0 = (11 + 15) as i64 / 2;
        let center_1 = (22 + 26) as i64 / 2;
        let signed_0 = center_0 - mapped_center;
        let signed_1 = center_1 - mapped_center;
        let abs_0 = (center_0 - mapped_center).abs();
        let abs_1 = (center_1 - mapped_center).abs();
        assert!(
            signed_0 < signed_1,
            "test setup: slice 0 signed diff ({}) should be more negative than slice 1 ({})",
            signed_0,
            signed_1
        );
        assert!(
            abs_1 < abs_0,
            "test setup: slice 1 abs dist ({}) should be less than slice 0 ({})",
            abs_1,
            abs_0
        );
        assert!((units[1].slice.start_fraction - 0.3).abs() < 0.01,
            "slice 1 (abs dist={}) should be chosen over slice 0 (abs dist={}, signed={}), got start_fraction={}",
            abs_1, abs_0, signed_0, units[1].slice.start_fraction);
    }

#[test]
    fn test_rebase_tier1_consumed_prevents_reuse() {
        let sid_a = ShapingIdentity {
            text_content_hash: 1,
            raw_font_fingerprint: "font_a".to_string(),
            glyph_indexes_hash: 10,
            cluster_glyph_count: 3,
            direction_rtl: false,
            format_fingerprint: 100,
        };

        let rebase_frames: Vec<RebaseFrame> = vec![
            rebase_frame(50, 60, 10.0, 100.0, 0.3, Some(sid_a.clone()), 0.3),
            rebase_frame(50, 60, 20.0, 200.0, 0.5, Some(sid_a.clone()), 0.5),
        ];

        let slices = vec![AnimatedSlice::insert_reveal(
            VisualTransactionKey::new(1, 1),
            LineSnapshotId::new(1, 0, 0),
            SourceRect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 20.0,
            },
            SourceRect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 20.0,
            },
            0.0,
            0.0,
            50,
            60,
            Some(sid_a.clone()),
            None,
        )];

        let offset_map = OffsetMap {
            entries: Vec::new(),
        };
        let mut units = wrap_units(slices);
        match_rebase_frames(&rebase_frames, &mut units, &offset_map);

        assert!(
            (units[0].slice.start_fraction - 0.3).abs() < 0.01,
            "slice 0 should get first rebase frame start_fraction 0.3 (not second 0.5), got {}",
            units[0].slice.start_fraction
        );
    }

#[test]
    fn test_rebase_tier3_consumed_prevents_reuse() {
        use writer_core::editor::{OffsetMapEntry, OffsetMapKind};
        let sid_dup = ShapingIdentity {
            text_content_hash: 99,
            raw_font_fingerprint: "font_x".to_string(),
            glyph_indexes_hash: 50,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 500,
        };

        let rebase_frames: Vec<RebaseFrame> = vec![
            rebase_frame(10, 30, 10.0, 100.0, 0.3, Some(sid_dup.clone()), 0.3),
            rebase_frame(10, 30, 20.0, 200.0, 0.5, Some(sid_dup.clone()), 0.5),
        ];

        let slices = vec![
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 1),
                LineSnapshotId::new(1, 0, 0),
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                11,
                15,
                Some(sid_dup.clone()),
                None,
            ),
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 2),
                LineSnapshotId::new(1, 0, 1),
                SourceRect {
                    x: 0.0,
                    y: 20.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                18,
                22,
                Some(sid_dup.clone()),
                None,
            ),
        ];

        let offset_map = OffsetMap {
            entries: vec![OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::default(),
                new_byte_offset: Utf8ByteOffset::default(),
                length: 200,
                kind: OffsetMapKind::Identity,
            }],
        };
        let mut units = wrap_units(slices);
        match_rebase_frames(&rebase_frames, &mut units, &offset_map);

        let mapped_center = 20i64;
        let center_0 = (11 + 15) as i64 / 2;
        let center_1 = (18 + 22) as i64 / 2;
        let dist_0 = (center_0 - mapped_center).abs();
        let dist_1 = (center_1 - mapped_center).abs();
        assert!(dist_1 < dist_0, "test setup: slice 1 should be closer");
        assert!(
            (units[1].slice.start_fraction - 0.3).abs() < 0.01,
            "slice 1 should get first rebase frame (start_fraction=0.3), got start_fraction={}",
            units[1].slice.start_fraction
        );
        assert!(
            (units[0].slice.start_fraction - 0.5).abs() < 0.01,
            "slice 0 should get second rebase frame (start_fraction=0.5), not reuse slice 1's frame, got start_fraction={}",
            units[0].slice.start_fraction
        );
    }

#[test]
    fn test_rebase_tier2_consumed_prevents_reuse() {
        use writer_core::editor::{OffsetMapEntry, OffsetMapKind};
        let sid_a = ShapingIdentity {
            text_content_hash: 1,
            raw_font_fingerprint: "font_a".to_string(),
            glyph_indexes_hash: 10,
            cluster_glyph_count: 3,
            direction_rtl: false,
            format_fingerprint: 100,
        };

        let rebase_frames: Vec<RebaseFrame> = vec![
            rebase_frame(50, 70, 10.0, 100.0, 0.3, Some(sid_a.clone()), 0.3),
            rebase_frame(50, 70, 20.0, 200.0, 0.5, Some(sid_a.clone()), 0.5),
        ];

        let slices = vec![AnimatedSlice::insert_reveal(
            VisualTransactionKey::new(1, 1),
            LineSnapshotId::new(1, 0, 0),
            SourceRect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 20.0,
            },
            SourceRect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 20.0,
            },
            0.0,
            0.0,
            150,
            170,
            Some(sid_a.clone()),
            None,
        )];

        let offset_map = OffsetMap {
            entries: vec![OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::default(),
                new_byte_offset: Utf8ByteOffset::unchecked(100),
                length: 200,
                kind: OffsetMapKind::Identity,
            }],
        };
        let mut units = wrap_units(slices);
        match_rebase_frames(&rebase_frames, &mut units, &offset_map);

        assert!(
            (units[0].slice.start_fraction - 0.3).abs() < 0.01,
            "slice 0 should get first rebase frame via tier2 (start_fraction=0.3), got start_fraction={}",
            units[0].slice.start_fraction
        );
    }

#[test]
    fn test_rebase_tier1_consumed_prevents_tier3_reuse() {
        use writer_core::editor::{OffsetMapEntry, OffsetMapKind};
        let sid_dup = ShapingIdentity {
            text_content_hash: 99,
            raw_font_fingerprint: "font_x".to_string(),
            glyph_indexes_hash: 50,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 500,
        };

        let rebase_frames: Vec<RebaseFrame> = vec![
            rebase_frame(50, 60, 10.0, 100.0, 0.3, Some(sid_dup.clone()), 0.3),
            rebase_frame(40, 80, 20.0, 200.0, 0.5, Some(sid_dup.clone()), 0.5),
        ];

        let slices = vec![AnimatedSlice::insert_reveal(
            VisualTransactionKey::new(1, 1),
            LineSnapshotId::new(1, 0, 0),
            SourceRect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 20.0,
            },
            SourceRect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 20.0,
            },
            0.0,
            0.0,
            50,
            60,
            Some(sid_dup.clone()),
            None,
        )];

        let offset_map = OffsetMap {
            entries: vec![OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::default(),
                new_byte_offset: Utf8ByteOffset::default(),
                length: 200,
                kind: OffsetMapKind::Identity,
            }],
        };
        let mut units = wrap_units(slices);
        match_rebase_frames(&rebase_frames, &mut units, &offset_map);

        assert!(
            (units[0].slice.start_fraction - 0.3).abs() < 0.01,
            "slice 0 should get first rebase frame start_fraction 0.3 (not second 0.5), got {}",
            units[0].slice.start_fraction
        );
    }

#[test]
    fn test_rebase_tier3_tiebreak_by_byte_start_then_index() {
        use writer_core::editor::{OffsetMapEntry, OffsetMapKind};
        let sid_dup = ShapingIdentity {
            text_content_hash: 99,
            raw_font_fingerprint: "font_x".to_string(),
            glyph_indexes_hash: 50,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 500,
        };

        let rebase_frames: Vec<RebaseFrame> = vec![rebase_frame(
            10,
            30,
            10.0,
            100.0,
            0.3,
            Some(sid_dup.clone()),
            0.3,
        )];

        let slices = vec![
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 1),
                LineSnapshotId::new(1, 0, 0),
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                11,
                15,
                Some(sid_dup.clone()),
                None,
            ),
            AnimatedSlice::insert_reveal(
                VisualTransactionKey::new(1, 2),
                LineSnapshotId::new(1, 0, 1),
                SourceRect {
                    x: 0.0,
                    y: 20.0,
                    w: 100.0,
                    h: 20.0,
                },
                SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 20.0,
                },
                0.0,
                0.0,
                25,
                29,
                Some(sid_dup.clone()),
                None,
            ),
        ];

        let offset_map = OffsetMap {
            entries: vec![OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::default(),
                new_byte_offset: Utf8ByteOffset::default(),
                length: 200,
                kind: OffsetMapKind::Identity,
            }],
        };
        let mut units = wrap_units(slices);
        match_rebase_frames(&rebase_frames, &mut units, &offset_map);

        let mapped_center = 20i64;
        let center_0 = (11 + 15) as i64 / 2;
        let center_1 = (25 + 29) as i64 / 2;
        let dist_0 = (center_0 - mapped_center).abs();
        let dist_1 = (center_1 - mapped_center).abs();
        assert_eq!(
            dist_0, dist_1,
            "test setup: both slices should have equal distance"
        );
        assert!(
            (units[0].slice.start_fraction - 0.3).abs() < 0.01,
            "slice 0 (lower byte_start) should win tiebreak, got start_fraction={}",
            units[0].slice.start_fraction
        );
        assert!(
            (units[1].slice.start_fraction - 0.0).abs() < 0.01,
            "slice 1 should not be matched, got start_fraction={}",
            units[1].slice.start_fraction
        );
    }

#[test]
    fn issue690_collect_rebase_frames_uses_per_unit_progress() {
        let now = Instant::now();
        let mut tx = rendering_tx(
            VisualTransactionKey::new(1, 1),
            TextVisualOperationKind::Insert,
            vec![
                // CaretDriven unit（InsertReveal）：可见比例从 caret track progress 推导。
                // caret track progress = 0.5 → ease_out_quad(0.5) = 0.75
                elapsed_unit(reveal_slice(0, 3, 100.0, 60.0), 50, 100, now),
                // Timed unit（ReflowMove）：有自己的时间线，elapsed 500ms > duration 100ms
                // → progress >= 1.0 → is_finished() = true → 已播完不交棒。
                // Issue #727 约束 2: CaretDriven unit 共享 caret track，不能独立"已播完"，
                // 所以用 Timed unit 验证"已播完的单元不交棒"。
                elapsed_unit(reflow_slice(3, 6, 160.0, 220.0), 500, 100, now),
            ],
            caret(100.0),
            caret(220.0),
            now,
            50,
        );
        // 事务级 progress = 0.5（eased 0.75）。CaretDriven unit 的可见比例从 caret track
        // progress 推导：start + (target - start) * ease_out_quad(0.5) = 0.75。
        tx.timeline.rendering_started_at = Some(now - Duration::from_millis(50));

        let frames = tx.collect_rebase_frames(now);
        assert_eq!(
            frames.len(),
            1,
            "已播完的单元不应再交棒，got {:?}",
            frames.iter().map(|f| f.byte_start).collect::<Vec<_>>()
        );
        let frame = &frames[0];
        assert!(
            (frame.visible_fraction - 0.75).abs() < 1e-6,
            "交棒帧必须按单元自己的 progress 计算可见比例（期望 0.75，按事务 progress 会得 0.19）",
        );
        assert_eq!((frame.byte_start, frame.byte_end), (0, 3));
        assert!((frame.x - 100.0).abs() < 1e-6);
        // Issue #690 评论 5679744253 问题 1: 采集时计算剩余时长，不再沿用旧起始时间。
        // 旧单元演了 50ms，总时长 100ms，剩余 50ms。
        assert_eq!(frame.remaining_duration_ms, 50);
        assert_eq!(frame.sampled_at, now);
        // 采集到的比例必须与文字帧同一个几何结果（右边界 100 + 60*0.75 = 145）
        let edge = reveal_slice(0, 3, 100.0, 60.0).compute_frame(frame.visible_fraction);
        assert!(
            (edge.x + edge.w - 145.0).abs() < 1e-6,
            "可见比例应还原出同一帧的文字右边界，got {}",
            edge.x + edge.w
        );
    }

#[test]
    fn issue690_match_rebase_frames_continues_unit_timeline() {
        let now = Instant::now();
        let old_unit = elapsed_unit(reveal_slice(0, 3, 100.0, 60.0), 50, 100, now);
        // Issue #727 约束 2: CaretDriven unit 的 visible_fraction 从 caret track progress 推导。
        // start_fraction=0, target_fraction=1, progress=0.5 → visible = 0 + (1-0)*ease_out_quad(0.5) = 0.75
        let visible_fraction = 0.0 + (1.0 - 0.0) * AnimatedSlice::ease_out_quad(0.5);
        let frame = old_unit.slice.compute_frame(visible_fraction);
        // Issue #690 评论 5679744253 问题 1: RebaseFrame 携带 sampled_at 和
        // remaining_duration_ms，retarget 时从当前帧重新起段。
        // 旧单元演了 50ms，总时长 100ms，剩余 50ms。
        let frames = vec![RebaseFrame {
            byte_start: old_unit.slice.byte_start,
            byte_end: old_unit.slice.byte_end,
            x: frame.x,
            y: frame.y,
            opacity: frame.opacity,
            shaping_identity: None,
            visible_fraction,
            sampled_at: now,
            remaining_duration_ms: 50,
        }];

        let mut units = wrap_units(vec![reveal_slice(0, 3, 100.0, 60.0)]);
        assert!(
            units[0].current_visible_fraction(now) < 1e-9,
            "交棒前新单元从 0 起步"
        );

        let offset_map = OffsetMap::build("abc", "abc");
        match_rebase_frames(&frames, &mut units, &offset_map);

        let unit = &units[0];
        // Issue #727 约束 2: CaretDriven unit 的 start_fraction 是 rebase 交棒时的载体。
        // 交棒后 start_fraction = visible_fraction = 0.75。
        let (start_fraction, started_at_is_none) = match &unit.timing {
            VisualUnitTiming::CaretDriven { start_fraction, .. } => (*start_fraction, true),
            VisualUnitTiming::Timed {
                start_fraction,
                started_at,
                ..
            } => (*start_fraction, started_at.is_none()),
        };
        assert!(
            (start_fraction - 0.75).abs() < 1e-6,
            "Reveal 单元交棒后应从已显示比例继续，got {}",
            start_fraction
        );
        // Issue #727 约束 2: CaretDriven unit 没有 duration_ms / started_at。
        // remaining_duration_ms 由 caret track 管理，不由 unit 自己的时间线决定。
        assert!(
            started_at_is_none,
            "CaretDriven unit 无独立时间线，started_at 不适用"
        );
        // Issue #690 评论 5679744253 问题 1: retarget 时从当前帧重新起段，
        // started_at 留 None，等进入 Rendering 再启动，progress 从 0 开始。
        // Issue #690 评论 5683759796: 原来写 Some(sampled_at) 会让 rebased 文字 unit
        // 从旧事务交棒时刻提前计时，与等 Rendering 才启动的 caret track 错拍；
        // 改成 None 后跟 fresh unit、caret track 一样由 build_text_animation_plan_with_sample
        // 在 Prepared→Rendering 时用同一个 sample.frame_now 启动。
        assert!(
            started_at_is_none,
            "Issue #690 评论 5683759796: rebase 后 started_at 应为 None（等 Rendering 再启动）"
        );
        let progress = unit.progress(now);
        assert!(
            progress.abs() < 1e-9,
            "retarget 时从当前帧重新起段，progress 从 0 开始，got {}",
            progress
        );
        // 可见比例连续：start_fraction=0.75 + (1-0.75)*ease_out_quad(0) = 0.75
        let visible = unit.current_visible_fraction(now);
        assert!(
            (visible - 0.75).abs() < 1e-6,
            "retarget 后可见比例应连续（0.75），不重复吃进度，got {}",
            visible
        );
    }

#[test]
    fn issue690_take_rebase_frames_carries_frames_and_cancels_old_transaction() {
        let now = Instant::now();
        let old_key = VisualTransactionKey::new(7, 7);
        let mut coord = LinuxEditorAnimationCoordinator::new();
        coord.prepared_queue.enqueue(rendering_tx(
            old_key,
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reveal_slice(0, 3, 100.0, 60.0), 50, 100, now)],
            caret(100.0),
            caret(160.0),
            now,
            50,
        ));

        let (frames, _) =
            coord.take_rebase_frames(&[old_key], "rebased_by_insert", now, None, "abc", 0);
        assert_eq!(frames.len(), 1, "旧事务的未播完单元要全部交棒");
        assert!((frames[0].visible_fraction - 0.75).abs() < 1e-6);
        assert!(
            coord.prepared_queue.is_empty(),
            "交棒后旧事务必须取消，snapshot/纹理资源归新事务所有"
        );
        let (no_frames, _) =
            coord.take_rebase_frames(&[], "rebased_by_insert", now, None, "abc", 0);
        assert!(no_frames.is_empty(), "无冲突事务时不产生交棒帧");
    }

    /// Issue #690 评论 5675007226 步骤 3: 未被新编辑覆盖的单元继续自己的时间线。
    #[test]
    fn issue690_take_rebase_frames_keeps_transaction_when_units_are_untouched() {
        let now = Instant::now();
        let old_key = VisualTransactionKey::new(11, 11);
        let mut coord = LinuxEditorAnimationCoordinator::new();
        coord.prepared_queue.enqueue(rendering_tx(
            old_key,
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reveal_slice(0, 3, 100.0, 60.0), 50, 100, now)],
            caret(100.0),
            caret(160.0),
            now,
            50,
        ));

        // 在 "abc" 末尾插入 "d"：old 坐标里只是位置 3 这一个点，前面的单元没被覆盖。
        let offset_map = OffsetMap::build("abc", "abcd");
        let (frames, _) = coord.take_rebase_frames(
            &[old_key],
            "rebased_by_insert",
            now,
            Some((&[(3, 3)], &offset_map)),
            "abc",
            0,
        );

        assert!(frames.is_empty(), "未覆盖的单元不该交棒，旧事务自己播完");
        assert_eq!(
            coord.prepared_queue.active_transactions().len(),
            1,
            "旧事务要留在队列里，继续持有自己的 snapshot 与静态隐藏区"
        );
        let unit = &coord.prepared_queue.active_transactions()[0].units[0];
        let start_fraction = unit.timing.start_fraction();
        assert!(
            start_fraction.abs() < 1e-9,
            "保留的单元起点不能被改写，got {}",
            start_fraction
        );
        // Issue #727 约束 2: CaretDriven unit 的 visible_fraction 从 caret track progress 推导。
        // 保留的单元的 caret track progress = 50/100 = 0.5
        // → visible = 0 + (1-0)*ease_out_quad(0.5) = 0.75
        let tx_ref = &coord.prepared_queue.active_transactions()[0];
        let caret_progress = tx_ref
            .cursor_visual_track
            .as_ref()
            .map(|track| track.progress(now))
            .unwrap_or(0.0);
        let visible = start_fraction
            + (unit.timing.target_fraction() - start_fraction)
                * AnimatedSlice::ease_out_quad(caret_progress);
        assert!(
            (visible - 0.75).abs() < 1e-6,
            "保留的单元沿 caret track 继续，不因新事务 id 归零重播，got {}",
            visible
        );
    }

#[test]
    fn issue690_take_rebase_frames_cancels_when_edit_covers_playing_unit() {
        let now = Instant::now();
        let old_key = VisualTransactionKey::new(12, 12);
        let mut coord = LinuxEditorAnimationCoordinator::new();
        coord.prepared_queue.enqueue(rendering_tx(
            old_key,
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reveal_slice(0, 3, 100.0, 60.0), 50, 100, now)],
            caret(100.0),
            caret(160.0),
            now,
            50,
        ));

        let offset_map = OffsetMap::build("abc", "ab");
        let (frames, _) = coord.take_rebase_frames(
            &[old_key],
            "rebased_by_delete",
            now,
            Some((&[(2, 3)], &offset_map)),
            "abc",
            0,
        );

        assert_eq!(frames.len(), 1, "被编辑覆盖的单元必须交棒给新事务");
        assert!(coord.prepared_queue.is_empty(), "覆盖后旧事务结束生命期");
    }

#[test]
    fn issue690_take_rebase_frames_cancels_when_unit_offsets_shift() {
        let now = Instant::now();
        let old_key = VisualTransactionKey::new(13, 13);
        let mut coord = LinuxEditorAnimationCoordinator::new();
        coord.prepared_queue.enqueue(rendering_tx(
            old_key,
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reveal_slice(0, 3, 100.0, 60.0), 50, 100, now)],
            caret(100.0),
            caret(160.0),
            now,
            50,
        ));

        // 在开头插入：old 单元 0..3 在新文档里变成 1..4，几何位置变了必须重排。
        let offset_map = OffsetMap::build("abc", "xabc");
        let (frames, _) = coord.take_rebase_frames(
            &[old_key],
            "rebased_by_insert",
            now,
            Some((&[(0, 0)], &offset_map)),
            "abc",
            0,
        );

        assert_eq!(frames.len(), 1, "偏移被平移的单元仍属被影响范围，要交棒");
        assert!(coord.prepared_queue.is_empty());
    }

#[test]
    fn issue690_take_rebase_frames_cancels_finished_transaction_without_frames() {
        let now = Instant::now();
        let old_key = VisualTransactionKey::new(14, 14);
        let mut coord = LinuxEditorAnimationCoordinator::new();
        coord.prepared_queue.enqueue(rendering_tx(
            old_key,
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reveal_slice(0, 3, 100.0, 60.0), 500, 100, now)],
            caret(100.0),
            caret(160.0),
            now,
            500,
        ));

        let offset_map = OffsetMap::build("abc", "abcd");
        let (frames, _) = coord.take_rebase_frames(
            &[old_key],
            "rebased_by_insert",
            now,
            Some((&[(3, 3)], &offset_map)),
            "abc",
            0,
        );

        assert!(frames.is_empty(), "已播完的单元是稳定终态，不该再交棒");
        assert!(
            coord.prepared_queue.is_empty(),
            "全部单元播完的事务没有保留价值，交给新事务接管资源"
        );
    }

    /// Issue #710 评论 5733833897: 一次新编辑同时撞上多笔旧事务时，
    /// 队列必须处理全部冲突，不能只处理第一笔。
    ///
    /// 场景：
    /// - current_old_text = "aaa\nbbb"（A 段 "aaa"，换行，B 段 "bbb"）
    /// - current_new_text = "aaabbb"（删了换行）
    /// - offset_map = OffsetMap::build("aaa\nbbb", "aaabbb")（current-old → current-new）
    /// - changed_old_ranges = [(3, 4)]（换行符位置）
    /// - tx1: unit byte range (0,3)（A 段 "aaa"，旧事务 new 坐标系）
    ///   tx1.new_text = "aaa\nbbb"（tx1 之后文本没变直到当前编辑）
    ///   per_tx_map = OffsetMap::build("aaa\nbbb", "aaa\nbbb") = identity
    ///   unit (0,3) 映射到 current-old (0,3)，不在 changed_old_ranges (3,4) 内
    ///   offset_map(0,3) = (0,3) == (0,3) → untouched ✓（tx1 留在队列）
    /// - tx2: unit byte range (4,7)（B 段 "bbb"，旧事务 new 坐标系）
    ///   tx2.new_text = "aaa\nbbb"
    ///   per_tx_map = identity
    ///   unit (4,7) 映射到 current-old (4,7)，不在 changed_old_ranges (3,4) 内
    ///   但 offset_map(4,7) = (3,6) ≠ (4,7) → 被覆盖 ✓（tx2 被取消）
    ///
    /// 断言：
    /// - tx1 留在队列里（keep）
    /// - tx2 被取消
    /// - rebase_frames 非空（来自 tx2 的 unit）
    #[test]
    fn issue710_take_rebase_frames_handles_multiple_conflicting_transactions() {
        let now = Instant::now();
        let mut coord = LinuxEditorAnimationCoordinator::new();

        // current-old 文本：A 段 "aaa" + 换行 + B 段 "bbb"
        let current_old_text = "aaa\nbbb";
        // current-new 文本：删除换行后 "aaabbb"
        let current_new_text = "aaabbb";
        // current-old → current-new 的 OffsetMap
        let offset_map = OffsetMap::build(current_old_text, current_new_text);
        // 编辑范围：只删了换行符 (3, 4)
        let changed_old_ranges: [(usize, usize); 1] = [(3, 4)];

        // ── tx1: A 段 "aaa" 的旧事务，unit byte range (0,3) ──
        // tx1.new_snapshot.virtual_text = "aaa\nbbb"（tx1 之后文本没变直到当前编辑）
        // per_tx_map = OffsetMap::build("aaa\nbbb", "aaa\nbbb") = identity
        let tx1_key = VisualTransactionKey::new(100, 1);
        let mut tx1 = rendering_tx(
            tx1_key,
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reveal_slice(0, 3, 100.0, 60.0), 50, 100, now)],
            caret(100.0),
            caret(160.0),
            now,
            50,
        );
        tx1.new_snapshot = Some(make_test_snapshot("aaa\nbbb", vec![]));
        coord.prepared_queue.enqueue(tx1);

        // ── tx2: B 段 "bbb" 的旧事务，unit byte range (4,7) ──
        // tx2.new_snapshot.virtual_text = "aaa\nbbb"
        // per_tx_map = identity
        // unit (4,7) 映射到 current-old (4,7)，不在 changed_old_ranges (3,4) 内
        // 但 offset_map(4,7) = (3,6) ≠ (4,7) → 被覆盖
        let tx2_key = VisualTransactionKey::new(101, 1);
        let mut tx2 = rendering_tx(
            tx2_key,
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reveal_slice(4, 7, 200.0, 60.0), 50, 100, now)],
            caret(200.0),
            caret(260.0),
            now,
            50,
        );
        tx2.new_snapshot = Some(make_test_snapshot("aaa\nbbb", vec![]));
        coord.prepared_queue.enqueue(tx2);

        // 前置断言：队列里有两笔事务
        assert_eq!(
            coord.prepared_queue.active_transactions().len(),
            2,
            "前置：队列里应有 tx1 和 tx2 两笔事务"
        );

        // ── 调用 take_rebase_frames 处理全部冲突事务 ──
        // conflicting = [tx1_key, tx2_key]（模拟 find_conflicting_transaction 返回全部）
        let conflicting = vec![tx1_key, tx2_key];
        let (rebase_frames, _caret_handoff) = coord.take_rebase_frames(
            &conflicting,
            "rebased_by_delete",
            now,
            Some((&changed_old_ranges, &offset_map)),
            current_old_text,
            0,
        );

        // ── 断言 1: tx1 留在队列里（keep）──
        let active = coord.prepared_queue.active_transactions();
        let tx1_still_active = active.iter().any(|t| t.key == tx1_key);
        assert!(
            tx1_still_active,
            "Issue #710 评论 5733833897: tx1 的 unit (0,3) 在 A 段，未被删除换行覆盖，\
             应留在队列里继续播完。实际队列里只剩 {:?}",
            active.iter().map(|t| t.key).collect::<Vec<_>>()
        );

        // ── 断言 2: tx2 被取消 ──
        let tx2_still_active = active.iter().any(|t| t.key == tx2_key);
        assert!(
            !tx2_still_active,
            "Issue #710 评论 5733833897: tx2 的 unit (4,7) 在 B 段，删除换行后 B 段上移，\
             offset_map(4,7) = (3,6) ≠ (4,7)，被覆盖，必须被取消。\
             实际队列里仍有 tx2"
        );

        // ── 断言 3: rebase_frames 非空（来自 tx2 的 unit）──
        assert!(
            !rebase_frames.is_empty(),
            "Issue #710 评论 5733833897: tx2 被取消时应采集其 unit 的 rebase frames，\
             rebase_frames 不应为空"
        );
        assert_eq!(
            rebase_frames.len(),
            1,
            "应采集到 tx2 的 1 个 unit frame（tx1 untouched 不采集）"
        );

        // ── 断言 4: rebase_frames 的 byte range 已映射到 current-old 坐标系 ──
        // tx2 的 unit (4,7) 经 per_tx_map (identity) 映射后仍为 (4,7)
        // （per_tx_map 是 tx2.new_text → current_old_text 的 identity 映射）
        let frame = &rebase_frames[0];
        assert_eq!(
            (frame.byte_start, frame.byte_end),
            (4, 7),
            "rebase frame 的 byte range 应为 current-old 坐标系的 (4,7)\
             （per_tx_map=identity 映射）"
        );

        println!(
            "[BUGFIX_VERIFY] Issue #710 评论 5733833897: 多冲突事务逐笔处理 \
             (tx1 keep, tx2 cancel) FIXED"
        );
    }

    /// Issue #710 评论 5734282079: take_rebase_frames 映射失败的 frame
    /// 不应进入 all_rebase_frames。
    ///
    /// 场景：
    /// - 旧事务 tx 的 new_snapshot.virtual_text = "abc"（旧事务 new 坐标系）
    /// - current_old_text = "axyzc"（current-old 坐标系，"bc" 被替换成 "xyz"）
    /// - tx 的 unit byte range = (1, 3)（"bc" 在旧事务 new 坐标系 "abc" 中）
    /// - per_tx_map = OffsetMap::build("abc", "axyzc")
    ///   - entries: [0,0,1] Identity ("a"), [2,4,1] Shifted ("c")
    ///   - map_old_range_to_new(1, 3) 找不到包含 old_start=1 的 entry
    ///     （中间的 "bc" 被替换了）→ 返回 None
    /// - 调用 take_rebase_frames，断言 rebase_frames 为空（映射失败的 frame 被丢弃）
    /// - 断言旧事务被 cancel（不在 active_transactions 中）
    #[test]
    fn issue710_take_rebase_frames_drops_frame_on_mapping_failure() {
        let now = Instant::now();
        let mut coord = LinuxEditorAnimationCoordinator::new();

        // current-old 文本："abc" 中 "bc" 被替换成 "xyz" → "axyzc"
        let current_old_text = "axyzc";
        // current-new 文本：与 current-old 相同（本测试只关心 per_tx_map 映射失败，
        // 不关心 current-old → current-new 映射）
        let current_new_text = "axyzc";
        let offset_map = OffsetMap::build(current_old_text, current_new_text);
        // 编辑范围：覆盖整个 "xyz" 区域 (1, 4)
        let changed_old_ranges: [(usize, usize); 1] = [(1, 4)];

        // ── tx: unit byte range (1, 3)（"bc" 在旧事务 new 坐标系 "abc" 中）──
        // tx.new_snapshot.virtual_text = "abc"（旧事务 new 坐标系）
        // per_tx_map = OffsetMap::build("abc", "axyzc")
        //   entries: [0,0,1] Identity ("a"), [2,4,1] Shifted ("c")
        // map_old_range_to_new(1, 3) → None（"bc" 被替换了，找不到包含 1 的 entry）
        let tx_key = VisualTransactionKey::new(200, 1);
        let mut tx = rendering_tx(
            tx_key,
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reveal_slice(1, 3, 100.0, 60.0), 50, 100, now)],
            caret(100.0),
            caret(160.0),
            now,
            50,
        );
        tx.new_snapshot = Some(make_test_snapshot("abc", vec![]));
        coord.prepared_queue.enqueue(tx);

        // 前置断言：队列里有一笔事务
        assert_eq!(
            coord.prepared_queue.active_transactions().len(),
            1,
            "前置：队列里应有 tx 一笔事务"
        );

        // ── 调用 take_rebase_frames 处理冲突事务 ──
        let conflicting = vec![tx_key];
        let (rebase_frames, _caret_handoff) = coord.take_rebase_frames(
            &conflicting,
            "rebased_by_replace",
            now,
            Some((&changed_old_ranges, &offset_map)),
            current_old_text,
            0,
        );

        // ── 断言 1: rebase_frames 为空（映射失败的 frame 被丢弃）──
        assert!(
            rebase_frames.is_empty(),
            "Issue #710 评论 5734282079: per_tx_map 映射失败的 frame 不应进入 \
             all_rebase_frames。frame 的原值 (1,3) 属于旧事务 new 坐标系 \"abc\"，\
             在 current-old \"axyzc\" 中 \"bc\" 已被替换成 \"xyz\"，\
             map_old_range_to_new(1,3) 返回 None，该 frame 必须被丢弃。\
             实际 rebase_frames 有 {} 个",
            rebase_frames.len()
        );

        // ── 断言 2: 旧事务被 cancel（不在 active_transactions 中）──
        let active = coord.prepared_queue.active_transactions();
        let tx_still_active = active.iter().any(|t| t.key == tx_key);
        assert!(
            !tx_still_active,
            "Issue #710 评论 5734282079: 映射失败的 frame 虽不进入 byte-range rebase，\
             但旧事务仍应被 cancel（cancel 逻辑不变）。实际队列里仍有 tx"
        );

        println!(
            "[BUGFIX_VERIFY] Issue #710 评论 5734282079: take_rebase_frames 映射失败 \
             的 frame 被丢弃（不进入 all_rebase_frames），旧事务仍 cancel FIXED"
        );
    }

    /// Issue #710 评论 5733833897: 验证 `find_conflicting_transaction` 返回全部冲突事务。
    ///
    /// 构造两笔 active 事务，它们的 units byte range 都与查询 range 重叠，
    /// `find_conflicting_transaction` 应返回两个 key（而非只返回第一个）。
    #[test]
    fn issue710_find_conflicting_transaction_returns_all_conflicts() {
        let now = Instant::now();
        let mut coord = LinuxEditorAnimationCoordinator::new();

        // current_old_text = "aaa\nbbb"
        let current_old_text = "aaa\nbbb";

        // ── tx1: unit (0,3)（A 段 "aaa"），tx1.new_text = "aaa\nbbb" ──
        let tx1_key = VisualTransactionKey::new(100, 1);
        let mut tx1 = rendering_tx(
            tx1_key,
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reveal_slice(0, 3, 100.0, 60.0), 50, 100, now)],
            caret(100.0),
            caret(160.0),
            now,
            50,
        );
        tx1.new_snapshot = Some(make_test_snapshot("aaa\nbbb", vec![]));
        coord.prepared_queue.enqueue(tx1);

        // ── tx2: unit (4,7)（B 段 "bbb"），tx2.new_text = "aaa\nbbb" ──
        let tx2_key = VisualTransactionKey::new(101, 1);
        let mut tx2 = rendering_tx(
            tx2_key,
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reveal_slice(4, 7, 200.0, 60.0), 50, 100, now)],
            caret(200.0),
            caret(260.0),
            now,
            50,
        );
        tx2.new_snapshot = Some(make_test_snapshot("aaa\nbbb", vec![]));
        coord.prepared_queue.enqueue(tx2);

        // 查询 range 覆盖整个 A+B 段 (0,7)
        let conflicts = coord
            .prepared_queue
            .find_conflicting_transaction(current_old_text, 0, 7);

        // 应返回两个 key
        assert_eq!(
            conflicts.len(),
            2,
            "Issue #710 评论 5733833897: find_conflicting_transaction 应返回全部冲突事务\
             （2 笔），而非只返回第一个。实际返回 {:?}",
            conflicts
        );
        assert!(conflicts.contains(&tx1_key), "应包含 tx1_key={:?}", tx1_key);
        assert!(conflicts.contains(&tx2_key), "应包含 tx2_key={:?}", tx2_key);

        println!(
            "[BUGFIX_VERIFY] Issue #710 评论 5733833897: find_conflicting_transaction \
             返回全部冲突事务 (2 笔) FIXED"
        );
    }

    /// Issue #710 评论 5733833897: 验证 caret handoff 在多冲突事务中选最新拥有
    /// coordinated caret 的一笔。
    ///
    /// 构造两笔冲突事务，都拥有 coordinated caret（cursor_owner_epoch == current_cursor_epoch），
    /// tx2 的 transaction_id 更大（更新创建）。take_rebase_frames 应选 tx2 的 caret handoff。
    #[test]
    fn issue710_take_rebase_frames_caret_handoff_picks_latest_coordinated_caret() {
        let now = Instant::now();
        let mut coord = LinuxEditorAnimationCoordinator::new();

        let current_old_text = "abc";
        let current_new_text = "ab";
        let offset_map = OffsetMap::build(current_old_text, current_new_text);
        let changed_old_ranges: [(usize, usize); 1] = [(2, 3)];

        // ── tx1: transaction_id=100，cursor_owner_epoch=0 ──
        // unit (0,3) 在 changed_old_ranges (2,3) 内 → 被覆盖 → cancel
        let tx1_key = VisualTransactionKey::new(100, 1);
        let mut tx1 = rendering_tx(
            tx1_key,
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reveal_slice(0, 3, 100.0, 60.0), 50, 100, now)],
            caret(100.0),
            caret(160.0),
            now,
            50,
        );
        tx1.new_snapshot = Some(make_test_snapshot("abc", vec![]));
        // 给 tx1 设置 caret track，使其拥有 coordinated caret
        tx1.cursor_visual_track = Some(PreparedCursorVisualTrack {
            from: caret(100.0),
            to: caret(160.0),
            from_visual_line_id: None,
            to_visual_line_id: None,
            from_line_top: 0.0,
            from_line_bottom: 0.0,
            to_line_top: 0.0,
            to_line_bottom: 0.0,
            started_at: Some(now - Duration::from_millis(50)),
            duration_ms: 100,
            pause_start: None,
        });
        coord.prepared_queue.enqueue(tx1);

        // ── tx2: transaction_id=200，cursor_owner_epoch=0（更新创建）──
        // unit (0,3) 在 changed_old_ranges (2,3) 内 → 被覆盖 → cancel
        let tx2_key = VisualTransactionKey::new(200, 1);
        let mut tx2 = rendering_tx(
            tx2_key,
            TextVisualOperationKind::Insert,
            vec![elapsed_unit(reveal_slice(0, 3, 200.0, 60.0), 50, 100, now)],
            caret(200.0),
            caret(260.0),
            now,
            50,
        );
        tx2.new_snapshot = Some(make_test_snapshot("abc", vec![]));
        // 给 tx2 设置 caret track，使其拥有 coordinated caret
        tx2.cursor_visual_track = Some(PreparedCursorVisualTrack {
            from: caret(200.0),
            to: caret(260.0),
            from_visual_line_id: None,
            to_visual_line_id: None,
            from_line_top: 0.0,
            from_line_bottom: 0.0,
            to_line_top: 0.0,
            to_line_bottom: 0.0,
            started_at: Some(now - Duration::from_millis(50)),
            duration_ms: 100,
            pause_start: None,
        });
        coord.prepared_queue.enqueue(tx2);

        // 两笔都被覆盖，都应被取消。caret handoff 应选 tx2（transaction_id=200 更大）。
        let conflicting = vec![tx1_key, tx2_key];
        let (_rebase_frames, caret_handoff) = coord.take_rebase_frames(
            &conflicting,
            "rebased_by_delete",
            now,
            Some((&changed_old_ranges, &offset_map)),
            current_old_text,
            0, // current_cursor_epoch = 0，两笔 tx 都匹配
        );

        // 两笔都应被取消
        assert!(coord.prepared_queue.is_empty(), "两笔冲突事务都应被取消");

        // caret handoff 应来自 tx2（transaction_id=200 更大）
        // tx2 的 caret track: from=200, to=260, started_at=now-50ms, duration=100ms
        // progress = 50/100 = 0.5 → eased = 0.75
        // sampled.x = 200 + (260-200)*0.75 = 200 + 45 = 245
        let handoff = caret_handoff.expect("应采样到 caret handoff");
        let expected_tx2_cursor = 200.0 + (260.0 - 200.0) * AnimatedSlice::ease_out_quad(0.5);
        assert!(
            (handoff.sampled.x - expected_tx2_cursor).abs() < 1e-6,
            "Issue #710 评论 5733833897: caret handoff 应选 tx2（transaction_id=200 更大），\
             sampled.x 应为 {}（tx2 屏幕光标），got {}",
            expected_tx2_cursor,
            handoff.sampled.x
        );

        // 额外验证：不等于 tx1 的屏幕光标
        let expected_tx1_cursor = 100.0 + (160.0 - 100.0) * AnimatedSlice::ease_out_quad(0.5);
        assert!(
            (handoff.sampled.x - expected_tx1_cursor).abs() > 1e-6,
            "caret handoff 不应选 tx1（transaction_id=100 更小），tx1 屏幕光标为 {}",
            expected_tx1_cursor
        );

        println!(
            "[BUGFIX_VERIFY] Issue #710 评论 5733833897: 多冲突事务 caret handoff \
             选最新拥有 coordinated caret 的一笔 FIXED"
        );
    }

