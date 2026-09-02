use super::composition::OffsetMap;
use super::rebase::{RebaseContinuation, RebaseFrameSnapshot, RebaseReason, RebaseSliceMapping};
use super::types::{AnimationMode, EditorChange, EditorTransactionCause};
use super::visual::{AnimatedSliceRole, ClusterRect, ClusterRun, VisualClassKind};
use crate::editor::strong_types::{Utf8ByteOffset, Utf8ByteRange};

/// #606: Composition 操作类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompositionOperationKind {
    Update,
    Commit,
    Cancel,
}

/// #606: Composition 视觉分类结果
#[derive(Debug, Clone, PartialEq)]
pub struct CompositionVisualClassification {
    pub old_affected_byte_ranges: Vec<Utf8ByteRange>,
    pub new_affected_byte_ranges: Vec<Utf8ByteRange>,
    pub animation_mode: AnimationMode,
    pub is_visual_same: bool,
    pub visual_class_kinds: Vec<VisualClassKind>,
}

/// #606: 统一的 composition 视觉分类入口
#[allow(clippy::too_many_arguments)]
pub fn classify_composition_visual(
    old_visual_text: &str,
    new_visual_text: &str,
    replace_start: usize,
    replace_end_exclusive: usize,
    operation_kind: CompositionOperationKind,
    animation_enabled: bool,
) -> CompositionVisualClassification {
    let visual_class_kinds = classify_visual_diff(old_visual_text, new_visual_text);
    let is_visual_same = visual_class_kinds.is_empty();

    let (old_affected, new_affected) = compute_composition_affected_ranges(
        old_visual_text, new_visual_text, replace_start, replace_end_exclusive, operation_kind,
    );

    let animation_mode = compute_composition_animation_mode(
        old_visual_text, new_visual_text, operation_kind, animation_enabled, &old_affected,
    );

    CompositionVisualClassification {
        old_affected_byte_ranges: old_affected,
        new_affected_byte_ranges: new_affected,
        animation_mode,
        is_visual_same,
        visual_class_kinds,
    }
}

fn compute_composition_affected_ranges(
    old_visual_text: &str,
    new_visual_text: &str,
    replace_start: usize,
    replace_end_exclusive: usize,
    operation_kind: CompositionOperationKind,
) -> (Vec<Utf8ByteRange>, Vec<Utf8ByteRange>) {
    match operation_kind {
        CompositionOperationKind::Update => {
            let old_affected = if old_visual_text.is_empty() {
                Vec::new()
            } else {
                vec![Utf8ByteRange::from_start_len(replace_start, old_visual_text.len())]
            };
            let new_affected = if new_visual_text.is_empty() {
                Vec::new()
            } else {
                vec![Utf8ByteRange::from_start_len(replace_start, new_visual_text.len())]
            };
            (old_affected, new_affected)
        }
        CompositionOperationKind::Commit => {
            let range = if new_visual_text.is_empty() {
                Vec::new()
            } else {
                vec![Utf8ByteRange::from_start_len(replace_start, new_visual_text.len())]
            };
            (range.clone(), range)
        }
        CompositionOperationKind::Cancel => {
            let old_affected = if !old_visual_text.is_empty() {
                vec![Utf8ByteRange::from_start_len(replace_start, old_visual_text.len())]
            } else if replace_start != replace_end_exclusive {
                vec![Utf8ByteRange::from_ordered(replace_start, replace_end_exclusive)]
            } else {
                Vec::new()
            };
            (old_affected, Vec::new())
        }
    }
}

fn compute_composition_animation_mode(
    old_visual_text: &str,
    new_visual_text: &str,
    operation_kind: CompositionOperationKind,
    animation_enabled: bool,
    old_affected: &[Utf8ByteRange],
) -> AnimationMode {
    if !animation_enabled {
        return AnimationMode::SystemSuppressed;
    }
    if operation_kind == CompositionOperationKind::Cancel && old_affected.is_empty() {
        return AnimationMode::SystemSuppressed;
    }
    let changed_text = if new_visual_text.len() >= old_visual_text.len() {
        new_visual_text
    } else {
        old_visual_text
    };
    let cluster_count = count_grapheme_clusters(changed_text);
    let contains_newline = changed_text.contains("\n");
    let contains_complex = text_contains_complex_grapheme(changed_text);
    choose_animation_mode(cluster_count, contains_newline, contains_complex, false, false, false, false, animation_enabled)
}

/// #516: 视觉对象分类器
pub fn classify_visual_diff(old_text: &str, new_text: &str) -> Vec<VisualClassKind> {
    if old_text == new_text {
        return Vec::new();
    }
    if old_text.is_empty() && !new_text.is_empty() {
        return vec![VisualClassKind::Insert];
    }
    if !old_text.is_empty() && new_text.is_empty() {
        return vec![VisualClassKind::Delete];
    }

    let prefix = common_prefix_byte_len(old_text, new_text);
    let suffix = common_suffix_byte_len(old_text, new_text, prefix);
    let old_end = old_text.len().saturating_sub(suffix);
    let new_end = new_text.len().saturating_sub(suffix);

    let mut kinds = Vec::new();
    if prefix > 0 {
        kinds.push(VisualClassKind::Static);
    }
    let removed = &old_text[prefix..old_end];
    let inserted = &new_text[prefix..new_end];
    if !removed.is_empty() && !inserted.is_empty() {
        kinds.push(VisualClassKind::Crossfade);
    } else if !removed.is_empty() {
        kinds.push(VisualClassKind::Delete);
    } else if !inserted.is_empty() {
        kinds.push(VisualClassKind::Insert);
    }
    if suffix > 0 {
        if !removed.is_empty() || !inserted.is_empty() {
            kinds.push(VisualClassKind::Move);
        } else {
            kinds.push(VisualClassKind::Static);
        }
    }
    kinds
}

/// 统一动画模式选择函数
#[allow(clippy::too_many_arguments)]
pub fn choose_animation_mode(
    cluster_count: usize,
    contains_newline: bool,
    contains_complex_grapheme: bool,
    is_scrolling: bool,
    is_loading: bool,
    is_applying_format: bool,
    is_applying_settings: bool,
    animation_enabled: bool,
) -> AnimationMode {
    if !animation_enabled || is_scrolling || is_loading || is_applying_format || is_applying_settings {
        return AnimationMode::SystemSuppressed;
    }
    if cluster_count == 0 {
        return AnimationMode::SystemSuppressed;
    }
    if contains_newline {
        return AnimationMode::LineReflowAnimation;
    }
    if contains_complex_grapheme {
        return AnimationMode::ClusterAnimation;
    }
    if cluster_count <= 8 {
        AnimationMode::GlyphAnimation
    } else {
        AnimationMode::RunAnimation
    }
}

/// 计算文本的 grapheme cluster 数量
pub fn count_grapheme_clusters(text: &str) -> usize {
    use unicode_segmentation::UnicodeSegmentation;
    text.graphemes(true).count()
}

/// 检测文本是否包含复杂 grapheme
pub fn text_contains_complex_grapheme(text: &str) -> bool {
    use unicode_segmentation::UnicodeSegmentation;
    text.graphemes(true).any(|g| g.chars().any(|ch| is_complex_grapheme_code_point(ch as u32)))
}

/// 检测单个 code point 是否属于复杂 grapheme
pub fn is_complex_grapheme_code_point(cp: u32) -> bool {
    if cp > 0xFFFF { return true; }
    if cp == 0x200D { return true; }
    if (0xFE00..=0xFE0F).contains(&cp) || (0xE0100..=0xE01EF).contains(&cp) { return true; }
    if (0x0300..=0x036F).contains(&cp) { return true; }
    if (0x1AB0..=0x1AFF).contains(&cp) { return true; }
    if (0x1DC0..=0x1DFF).contains(&cp) { return true; }
    if (0x20D0..=0x20FF).contains(&cp) { return true; }
    if (0xFE20..=0xFE2F).contains(&cp) { return true; }
    if (0x1F600..=0x1F64F).contains(&cp) { return true; }
    if (0x1F300..=0x1F5FF).contains(&cp) { return true; }
    if (0x1F680..=0x1F6FF).contains(&cp) { return true; }
    if (0x1F900..=0x1F9FF).contains(&cp) { return true; }
    if (0x1F1E6..=0x1F1FF).contains(&cp) { return true; }
    false
}

/// 检测单个 code point 是否为组合字符
pub fn is_combining_code_point(cp: u32) -> bool {
    (0x0300..=0x036F).contains(&cp)
        || (0x1AB0..=0x1AFF).contains(&cp)
        || (0x1DC0..=0x1DFF).contains(&cp)
        || (0x20D0..=0x20FF).contains(&cp)
        || (0xFE20..=0xFE2F).contains(&cp)
        || (0xFE00..=0xFE0F).contains(&cp)
        || (0xE0100..=0xE01EF).contains(&cp)
        || cp == 0x200D
}

/// 检测单个 code point 是否属于 CJK 字符
pub fn is_cjk_code_point(cp: u32) -> bool {
    (0x4E00..=0x9FFF).contains(&cp)
        || (0x3400..=0x4DBF).contains(&cp)
        || (0x20000..=0x2A6DF).contains(&cp)
        || (0x2A700..=0x2B73F).contains(&cp)
        || (0x2B740..=0x2B81F).contains(&cp)
        || (0xF900..=0xFAFF).contains(&cp)
        || (0x2F800..=0x2FA1F).contains(&cp)
        || (0x3000..=0x303F).contains(&cp)
        || (0x3040..=0x309F).contains(&cp)
        || (0x30A0..=0x30FF).contains(&cp)
        || (0xAC00..=0xD7AF).contains(&cp)
}

/// 将文本按 run/word/chunk 分组，用于 RunAnimation。
pub fn split_text_into_runs(text: &str, base_offset: usize) -> Vec<ClusterRun> {
    let mut runs = Vec::new();
    let mut current_text = String::new();
    let mut current_cluster_count = 0usize;
    let mut current_byte_start = base_offset;
    let chinese_chunk_size = 5;

    for (byte_offset, ch) in text.char_indices() {
        let absolute_byte = base_offset + byte_offset;
        if ch.is_whitespace() {
            if !current_text.is_empty() {
                runs.push(ClusterRun {
                    byte_start: Utf8ByteOffset::unchecked(current_byte_start),
                    byte_end: Utf8ByteOffset::unchecked(absolute_byte),
                    text: current_text.clone(),
                    cluster_count: current_cluster_count,
                });
                current_text.clear();
                current_cluster_count = 0;
            }
            runs.push(ClusterRun {
                byte_start: Utf8ByteOffset::unchecked(absolute_byte),
                byte_end: Utf8ByteOffset::unchecked(absolute_byte + ch.len_utf8()),
                text: ch.to_string(),
                cluster_count: 1,
            });
            current_byte_start = absolute_byte + ch.len_utf8();
            continue;
        }
        let is_cjk = is_cjk_code_point(ch as u32);
        if current_text.is_empty() {
            current_byte_start = absolute_byte;
        }
        current_text.push(ch);
        current_cluster_count += 1;
        if is_cjk && current_cluster_count >= chinese_chunk_size {
            runs.push(ClusterRun {
                byte_start: Utf8ByteOffset::unchecked(current_byte_start),
                byte_end: Utf8ByteOffset::unchecked(absolute_byte + ch.len_utf8()),
                text: current_text.clone(),
                cluster_count: current_cluster_count,
            });
            current_text.clear();
            current_cluster_count = 0;
            current_byte_start = absolute_byte + ch.len_utf8();
        }
        if !is_cjk && current_cluster_count >= 8 {
            runs.push(ClusterRun {
                byte_start: Utf8ByteOffset::unchecked(current_byte_start),
                byte_end: Utf8ByteOffset::unchecked(absolute_byte + ch.len_utf8()),
                text: current_text.clone(),
                cluster_count: current_cluster_count,
            });
            current_text.clear();
            current_cluster_count = 0;
            current_byte_start = absolute_byte + ch.len_utf8();
        }
    }
    if !current_text.is_empty() {
        runs.push(ClusterRun {
            byte_start: Utf8ByteOffset::unchecked(current_byte_start),
            byte_end: Utf8ByteOffset::unchecked(base_offset + text.len()),
            text: current_text,
            cluster_count: current_cluster_count,
        });
    }
    runs
}

/// 将文本按 grapheme cluster 分割，用于 ClusterAnimation。
pub fn split_text_into_clusters(text: &str, base_offset: usize) -> Vec<ClusterRect> {
    use unicode_segmentation::UnicodeSegmentation;
    let mut clusters = Vec::new();
    for grapheme in text.graphemes(true) {
        let byte_start = base_offset + (grapheme.as_ptr() as usize - text.as_ptr() as usize);
        let byte_end = byte_start + grapheme.len();
        let is_complex = grapheme.chars().any(|ch| is_complex_grapheme_code_point(ch as u32));
        clusters.push(ClusterRect {
            byte_start: Utf8ByteOffset::unchecked(byte_start),
            byte_end: Utf8ByteOffset::unchecked(byte_end),
            text: grapheme.to_string(),
            is_complex,
        });
    }
    clusters
}

/// 判断编辑变更是否应产生动画。
pub(crate) fn should_animate_changes(
    changes: &[EditorChange],
    cause: EditorTransactionCause,
    _max_animated_chars: usize,
) -> bool {
    if matches!(
        cause,
        EditorTransactionCause::Load
            | EditorTransactionCause::Format
            | EditorTransactionCause::Programmatic
            | EditorTransactionCause::ImeComposition
    ) {
        return false;
    }
    if changes.len() != 1 {
        return false;
    }
    let text = changes[0].text();
    !text.is_empty()
}

/// 基于最长公共前缀/后缀的纯文本差异算法。
pub fn diff_plain_text(old_text: &str, new_text: &str) -> Vec<EditorChange> {
    if old_text == new_text {
        return Vec::new();
    }
    let prefix = common_prefix_byte_len(old_text, new_text);
    let suffix = common_suffix_byte_len(old_text, new_text, prefix);
    let old_end = old_text.len() - suffix;
    let new_end = new_text.len() - suffix;
    let removed = &old_text[prefix..old_end];
    let inserted = &new_text[prefix..new_end];
    let mut changes = Vec::new();
    if !removed.is_empty() {
        changes.push(EditorChange::Delete {
            index: Utf8ByteOffset::unchecked(prefix),
            text: removed.to_string(),
        });
    }
    if !inserted.is_empty() {
        changes.push(EditorChange::Insert {
            index: Utf8ByteOffset::unchecked(prefix),
            text: inserted.to_string(),
        });
    }
    changes
}

pub fn common_prefix_byte_len(old_text: &str, new_text: &str) -> usize {
    let mut prefix = 0;
    for ((old_index, old_char), (_, new_char)) in old_text.char_indices().zip(new_text.char_indices()) {
        if old_char != new_char {
            break;
        }
        prefix = old_index + old_char.len_utf8();
    }
    prefix
}

pub fn common_suffix_byte_len(old_text: &str, new_text: &str, prefix: usize) -> usize {
    let old_tail = &old_text[prefix..];
    let new_tail = &new_text[prefix..];
    let mut suffix = 0;
    for ((_, old_char), (_, new_char)) in old_tail.char_indices().rev().zip(new_tail.char_indices().rev()) {
        if old_char != new_char {
            break;
        }
        suffix += old_char.len_utf8();
    }
    suffix
}
