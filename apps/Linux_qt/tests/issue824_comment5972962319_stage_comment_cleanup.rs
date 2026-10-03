//! Issue #824 评论 5972962319 结构守卫 — 生产代码里不得再描述已删除的
//! 「旧 route 串行 + 新旧 stage 混在一条 track」模型。
//!
//! 复核结论：主模型已经改对，但 `transaction/types.rs`、`animation/sample.rs`、
//! `animation/frame_state.rs` 的注释仍在描述 #824 明确删除的旧架构（新旧 stage
//! 混合驱动、旧剩余段合成新 route），与代码正面矛盾。本守卫锁死清干净后的语义：
//! - 一条 active route 只属于当前 motion，所有 segment 用同一个 stage；
//! - 旧 glyph 继续显示时从当前屏幕帧转成 Timed，不带历史 stage / 旧 route 段；
//! - stage mismatch 分支只剩防御性兼容，不是正常架构。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{function_window, read_src};

const TYPES: &str = "src/sujian_editor_item/animation/transaction/types.rs";
const SAMPLE: &str = "src/sujian_editor_item/animation/sample.rs";
const FRAME_STATE: &str = "src/sujian_editor_item/animation/frame_state.rs";

/// 守卫 1: `IngestStageId` 注释只描述当前 motion 的 stage，不再写旧剩余段合成。
#[test]
fn guard1_ingest_stage_id_describes_current_motion_only() {
    let src = read_src(TYPES);
    assert!(
        !src.contains("旧剩余段(stage A) + 新段(stage B)"),
        "Issue #824 评论 5972962319: types.rs 不得再写 \
         `旧剩余段(stage A) + 新段(stage B)`——那是已删除的串行 route 模型。"
    );
    let window = function_window(&src, "**当前 active motion** 的阶段/快照身份", 1000);
    assert!(
        window.contains("一条 active route 只属于当前 motion"),
        "Issue #824 评论 5972962319: IngestStageId 注释必须写明一条 active route \
         只属于当前 motion，所有 segment 用当前 stage。"
    );
    assert!(
        window.contains("detach_caret_track_to_timed"),
        "Issue #824 评论 5972962319: IngestStageId 注释必须写明旧 glyph 继续显示时\
         从当前屏幕帧转成 Timed（detach helper），不插回旧 stage / 旧 route 段。"
    );
}

/// 守卫 2: `PreparedCursorVisualTrack::stage_id` 不再写新旧 stage 混在同一条 track。
#[test]
fn guard2_cursor_track_stage_id_is_current_motion_identity() {
    let src = read_src(TYPES);
    assert!(
        !src.contains("可能混合旧 stage_id"),
        "Issue #824 评论 5972962319: types.rs 不得再写 segments 可能混合旧 stage_id\
         （carried route 剩余段）——该模型已删除。"
    );
    let window = function_window(&src, "**当前 active motion 的身份**", 800);
    assert!(
        window.contains("当前 active motion 的身份"),
        "Issue #824 评论 5972962319: track.stage_id 必须是「当前 active motion 的身份」。"
    );
    assert!(
        window.contains("不会混合历史 stage"),
        "Issue #824 评论 5972962319: track.stage_id 注释必须写明 route 不会混合历史 stage。"
    );
}

/// 守卫 3: `sample.rs` 的 stage mismatch 只作为防御性兼容，不是正常架构。
#[test]
fn guard3_sample_stage_mismatch_is_defensive_only() {
    let src = read_src(SAMPLE);
    let window = function_window(&src, "Issue #824 评论 5972962319: stage_id 过滤", 1400);
    assert!(
        !window.contains("在同一条 track 上"),
        "Issue #824 评论 5972962319: 不得再写「carried unit（旧 stage_id）和新 unit\
         在同一条 track 上」——生产路径不出现这种情况。"
    );
    assert!(
        window.contains("防御性"),
        "Issue #824 评论 5972962319: stage mismatch 分支必须说明成防御性兼容 / \
         异常状态保护，而不是正常架构。"
    );
    assert!(
        window.contains("active motion"),
        "Issue #824 评论 5972962319: 必须写明生产路径上 unit 与 segment 同属这一笔\
         active motion（stage_id 总是匹配）。"
    );
}

/// 守卫 4: `frame_state.rs` 不再写「同一事务可以包含多个 carried stage」。
#[test]
fn guard4_frame_state_stage_docs_match_current_model() {
    let src = read_src(FRAME_STATE);
    assert!(
        !src.contains("同一事务可以包含多个 carried stage"),
        "Issue #824 评论 5972962319: SampledSliceFrame 不得再写同一事务包含多个\
         carried stage——吞吐 unit 只有当前 motion 的 stage。"
    );
    assert!(
        src.contains("只有**当前 active motion** 的"),
        "Issue #824 评论 5972962319: unit_stage_id 注释必须写明 CaretTrack 吞吐 unit\
         只有当前 active motion 的 stage。"
    );
    assert!(
        src.contains("不继承时间线，转成从当前屏幕帧继续的 Timed"),
        "Issue #824 评论 5972962319: CarriedVisualUnit.timing 注释必须写明协同吞吐字\
         不继承旧时间线，而是转成从当前屏幕帧继续的 Timed。"
    );
}
