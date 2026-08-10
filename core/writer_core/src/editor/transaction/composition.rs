use serde::{Deserialize, Serialize};

use super::engine::{common_prefix_byte_len, common_suffix_byte_len};
use super::visual::{build_virtual_text, CursorRect, DecorationSlice};
use crate::editor::strong_types::{
    EditorRevision, EditorSessionGeneration, EditorSessionId, Utf8ByteOffset, Utf8ByteRange,
};

/// 预输入视觉修订 — 把预输入改为临时视觉正文版本。
///
/// virtualText 仅用于排版和渲染，不写入正文、Undo、保存和同步和 Core 正文状态。
/// 每次预输入变化生成新 CompositionVisualRevision，
/// 使用相同 StaticLinePatch + AnimatedSlice 分类。
///
/// #516: virtualText 必须通过 `build_virtual_text()` 构造，
/// 严格按 committedText[0..replaceStart] + preeditText + committedText[replaceEnd..] 拼接。
/// 不得丢失 replaceEnd 后正文，也不得默认把预输入永远当成零长度插入。
///
/// #517: 增加不可变 revision 链接。每次更新必须从 previous visual revision 接续，
/// 不允许从 committed revision 重新开始。replaceStart/replaceEndExclusive 始终是
/// committed 正文坐标，preeditCursorOffset 始终是 preedit 内部坐标。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompositionVisualRevision {
    /// 修订唯一 ID（递增，由 CompositionSession 分配）
    #[serde(default)]
    pub revision_id: u64,
    /// 所属 composition session ID
    #[serde(default)]
    pub session_id: EditorSessionId,
    /// 此修订基于的 committed revision ID
    #[serde(default)]
    pub committed_revision_id: EditorRevision,
    /// 已提交文本（不含预输入）
    pub committed_text: String,
    /// 预输入替换范围（UTF-8 byte offset，committed 正文坐标）
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::editor::strong_types::ser_opt_range",
        deserialize_with = "crate::editor::strong_types::de_opt_range"
    )]
    pub composition_replace_range: Option<Utf8ByteRange>,
    /// 预输入文本
    #[serde(default)]
    pub preedit_text: String,
    /// 预输入光标偏移（preedit 内部 UTF-8 byte offset）
    #[serde(
        default,
        serialize_with = "crate::editor::strong_types::ser_offset",
        deserialize_with = "crate::editor::strong_types::de_offset"
    )]
    pub preedit_cursor_offset: Utf8ByteOffset,
    /// 虚拟文本 — 仅用于排版和渲染，不写入正文
    #[serde(default)]
    pub virtual_text: String,
    /// 受影响段落范围（UTF-8 byte offset）
    #[serde(
        serialize_with = "crate::editor::strong_types::ser_range",
        deserialize_with = "crate::editor::strong_types::de_range"
    )]
    pub affected_paragraph_range: Utf8ByteRange,
    /// 行快照 ID 列表（由平台层填充）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub line_snapshot_ids: Vec<u64>,
    /// 光标矩形
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_rect: Option<CursorRect>,
    /// 装饰范围
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decoration_ranges: Vec<DecorationSlice>,
    /// IME 光标范围/位置（UTF-8 byte offset）
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::editor::strong_types::ser_opt_range",
        deserialize_with = "crate::editor::strong_types::de_opt_range"
    )]
    pub ime_cursor_range: Option<Utf8ByteRange>,
    /// 从上一 CompositionVisualRevision 的偏移映射
    ///
    /// #517: 连续更新必须从 previous visual revision 接续，
    /// 不允许从 committed revision 重新开始。
    /// OffsetMap 记录 old virtualText → new virtualText 的字符映射，
    /// 用于后续正文 cluster 保持身份并生成 Move，而不是全部 Crossfade/Insert。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_map_from_previous: Option<OffsetMap>,
}

impl CompositionVisualRevision {
    /// 使用 `build_virtual_text()` 正确构造 CompositionVisualRevision。
    ///
    /// virtualText 由 committed_text、composition_replace_range 和 preedit_text
    /// 自动计算，不手动传入。
    pub fn new(
        committed_text: String,
        composition_replace_range: Option<Utf8ByteRange>,
        preedit_text: String,
        affected_paragraph_range: Utf8ByteRange,
    ) -> Self {
        let composition_replace_range_tuple =
            composition_replace_range.map(|r| (r.start().value(), r.end().value()));
        let virtual_text = build_virtual_text(
            &committed_text,
            composition_replace_range_tuple,
            &preedit_text,
        );
        Self {
            revision_id: 0,
            session_id: EditorSessionId::new(0),
            committed_revision_id: EditorRevision::initial(),
            committed_text,
            composition_replace_range,
            preedit_text,
            preedit_cursor_offset: Utf8ByteOffset::unchecked(0),
            virtual_text,
            affected_paragraph_range,
            line_snapshot_ids: Vec::new(),
            cursor_rect: None,
            decoration_ranges: Vec::new(),
            ime_cursor_range: None,
            offset_map_from_previous: None,
        }
    }

    /// #517: 从 previous visual revision 构造新 CompositionVisualRevision。
    ///
    /// 更新链必须是：previous visual revision -> new visual revision，
    /// 而不是：committed revision -> 每一次新的 preedit。
    ///
    /// 此方法自动计算 OffsetMap，记录 old virtualText → new virtualText 的映射。
    pub fn from_previous(
        previous: &CompositionVisualRevision,
        new_preedit_text: String,
        new_preedit_cursor_offset: usize,
        affected_paragraph_range: Utf8ByteRange,
    ) -> Self {
        let composition_replace_range_tuple = previous
            .composition_replace_range
            .map(|r| (r.start().value(), r.end().value()));
        let virtual_text = build_virtual_text(
            &previous.committed_text,
            composition_replace_range_tuple,
            &new_preedit_text,
        );
        let offset_map = OffsetMap::build(&previous.virtual_text, &virtual_text);
        Self {
            revision_id: 0,
            session_id: previous.session_id,
            committed_revision_id: previous.committed_revision_id,
            committed_text: previous.committed_text.clone(),
            composition_replace_range: previous.composition_replace_range,
            preedit_text: new_preedit_text,
            preedit_cursor_offset: Utf8ByteOffset::unchecked(new_preedit_cursor_offset),
            virtual_text,
            affected_paragraph_range,
            line_snapshot_ids: Vec::new(),
            cursor_rect: None,
            decoration_ranges: Vec::new(),
            ime_cursor_range: None,
            offset_map_from_previous: Some(offset_map),
        }
    }

    /// 预输入文本在 virtualText 中的字节范围。
    ///
    /// #517: 此范围只能表示 virtualText 中 preedit 的范围，
    /// 不能表示 committed replaceRange；两者必须分开命名和存储。
    pub fn preedit_byte_range_in_virtual_text(&self) -> (usize, usize) {
        match self.composition_replace_range {
            Some(range) => {
                let replace_start = range.start().value();
                (replace_start, replace_start + self.preedit_text.len())
            }
            None => {
                let start = self.committed_text.len();
                (start, start + self.preedit_text.len())
            }
        }
    }
}

/// #517: 偏移映射 — 两个 visualText 之间的字符身份映射。
///
/// 记录 old virtualText 中每个字符在 new virtualText 中的对应位置。
/// 用于后续正文 cluster 保持身份并生成 Move，而不是全部 Crossfade/Insert。
///
/// 映射规则：
/// - 前缀相同部分：old[i] → new[i]（identity）
/// - 中间差异部分：无映射（Insert/Delete/Crossfade）
/// - 后缀相同部分：old[i] → new[j]（shifted）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OffsetMap {
    /// 映射条目列表，按 old byte offset 排序
    pub entries: Vec<OffsetMapEntry>,
}

/// #517: 单个偏移映射条目。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OffsetMapEntry {
    /// old virtualText 中的 UTF-8 byte offset
    #[serde(
        serialize_with = "crate::editor::strong_types::ser_offset",
        deserialize_with = "crate::editor::strong_types::de_offset"
    )]
    pub old_byte_offset: Utf8ByteOffset,
    /// new virtualText 中的 UTF-8 byte offset
    #[serde(
        serialize_with = "crate::editor::strong_types::ser_offset",
        deserialize_with = "crate::editor::strong_types::de_offset"
    )]
    pub new_byte_offset: Utf8ByteOffset,
    /// 映射的字符数（UTF-8 bytes）
    pub length: usize,
    /// 映射类型
    pub kind: OffsetMapKind,
}

/// #517: 偏移映射类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OffsetMapKind {
    /// 文本和位置均相同（前缀/后缀静态部分）
    Identity,
    /// 文本相同但位置变化（后缀移动）
    Shifted,
}

impl OffsetMap {
    /// 从 old/new virtualText 构建偏移映射。
    ///
    /// 使用最长公共前缀/后缀算法确定映射区域。
    pub fn build(old_text: &str, new_text: &str) -> Self {
        if old_text.is_empty() || new_text.is_empty() || old_text == new_text {
            return OffsetMap {
                entries: Vec::new(),
            };
        }

        let prefix = common_prefix_byte_len(old_text, new_text);
        let suffix = common_suffix_byte_len(old_text, new_text, prefix);

        let mut entries = Vec::new();

        if prefix > 0 {
            entries.push(OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::unchecked(0),
                new_byte_offset: Utf8ByteOffset::unchecked(0),
                length: prefix,
                kind: OffsetMapKind::Identity,
            });
        }

        if suffix > 0 {
            let old_suffix_start = old_text.len() - suffix;
            let new_suffix_start = new_text.len() - suffix;
            let kind = if prefix > 0 || (old_text.len() != new_text.len()) {
                OffsetMapKind::Shifted
            } else {
                OffsetMapKind::Identity
            };
            entries.push(OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::unchecked(old_suffix_start),
                new_byte_offset: Utf8ByteOffset::unchecked(new_suffix_start),
                length: suffix,
                kind,
            });
        }

        OffsetMap { entries }
    }

    /// 查找 old byte offset 在 new text 中的对应位置。
    pub fn map_old_to_new(&self, old_byte_offset: usize) -> Option<usize> {
        for entry in &self.entries {
            let entry_old = entry.old_byte_offset.value();
            if old_byte_offset >= entry_old && old_byte_offset < entry_old + entry.length {
                let offset_within = old_byte_offset - entry_old;
                return Some(entry.new_byte_offset.value() + offset_within);
            }
        }
        None
    }

    pub fn map_new_to_old(&self, new_byte_offset: usize) -> Option<usize> {
        for entry in &self.entries {
            let entry_new = entry.new_byte_offset.value();
            if new_byte_offset >= entry_new && new_byte_offset < entry_new + entry.length {
                let offset_within = new_byte_offset - entry_new;
                return Some(entry.old_byte_offset.value() + offset_within);
            }
        }
        None
    }
}

/// #517: 预输入会话 — 跨平台 composition 状态模型。
///
/// Android 和 Linux 都必须维护一个明确的 composition session，
/// 而不是零散地存 preedit_text 和临时 snapshot。
///
/// 关键规则：
/// - replaceStart/replaceEndExclusive 始终是 committed 正文坐标
/// - preeditCursorOffset 始终是 preedit 内部坐标
/// - virtualText 由 committed replaceRange 和 preeditText 构造
/// - composing 更新不能修改 committed buffer、Undo、保存、同步和 Core 正文状态
/// - 连续 setComposingText 必须保持原 session 的 committed replaceRange，
///   不能随着 preedit 长度变化而移动 end
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompositionSession {
    /// 会话唯一 ID
    pub session_id: EditorSessionId,
    /// 此会话基于的 committed revision ID
    pub committed_revision_id: EditorRevision,
    /// 会话开始时的 committed 文本
    pub committed_text_at_start: String,
    /// committed 正文替换范围起始（UTF-8 byte offset）
    pub replace_start: Utf8ByteOffset,
    /// committed 正文替换范围结束（不含，UTF-8 byte offset）
    pub replace_end_exclusive: Utf8ByteOffset,
    /// 当前预输入文本
    #[serde(default)]
    pub preedit_text: String,
    /// 预输入光标偏移（preedit 内部 UTF-8 byte offset）
    #[serde(
        default,
        serialize_with = "crate::editor::strong_types::ser_offset",
        deserialize_with = "crate::editor::strong_types::de_offset"
    )]
    pub preedit_cursor_offset: Utf8ByteOffset,
    /// 当前视觉修订
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_visual_revision: Option<CompositionVisualRevision>,
    /// 最后提交的 generation
    #[serde(default)]
    pub last_submitted_generation: EditorSessionGeneration,
    /// 下一个 revision ID
    #[serde(default)]
    pub next_revision_id: u64,
}

impl CompositionSession {
    /// 创建新的 composition session。
    ///
    /// #517: 普通 setComposingText 初次预输入默认是零长度插入：
    /// replace_start == replace_end_exclusive == 原 committed 光标位置。
    /// 只有 setComposingRegion 或平台明确给出替换范围时才能形成非零替换范围。
    pub fn new(
        session_id: u64,
        committed_revision_id: u64,
        committed_text: String,
        cursor_position: usize,
    ) -> Self {
        let cursor = Utf8ByteOffset::unchecked(cursor_position);
        Self {
            session_id: EditorSessionId::new(session_id),
            committed_revision_id: EditorRevision::new(committed_revision_id),
            committed_text_at_start: committed_text.clone(),
            replace_start: cursor,
            replace_end_exclusive: cursor,
            preedit_text: String::new(),
            preedit_cursor_offset: Utf8ByteOffset::unchecked(0),
            current_visual_revision: None,
            last_submitted_generation: EditorSessionGeneration::initial(),
            next_revision_id: 1,
        }
    }

    /// 创建带替换范围的 composition session（setComposingRegion）。
    pub fn new_with_replace_range(
        session_id: u64,
        committed_revision_id: u64,
        committed_text: String,
        replace_start: usize,
        replace_end_exclusive: usize,
    ) -> Self {
        Self {
            session_id: EditorSessionId::new(session_id),
            committed_revision_id: EditorRevision::new(committed_revision_id),
            committed_text_at_start: committed_text,
            replace_start: Utf8ByteOffset::unchecked(replace_start),
            replace_end_exclusive: Utf8ByteOffset::unchecked(replace_end_exclusive),
            preedit_text: String::new(),
            preedit_cursor_offset: Utf8ByteOffset::unchecked(0),
            current_visual_revision: None,
            last_submitted_generation: EditorSessionGeneration::initial(),
            next_revision_id: 1,
        }
    }

    /// 更新预输入文本。
    ///
    /// #517: 连续 setComposingText 必须保持原 session 的 committed replaceRange，
    /// 不能随着 preedit 长度变化而移动 end。
    pub fn update_preedit(
        &mut self,
        new_preedit_text: String,
        new_preedit_cursor_offset: usize,
    ) -> CompositionVisualRevision {
        let new_revision = match &self.current_visual_revision {
            Some(previous) => {
                let mut rev = CompositionVisualRevision::from_previous(
                    previous,
                    new_preedit_text.clone(),
                    new_preedit_cursor_offset,
                    Utf8ByteRange::from_start_len(0, self.committed_text_at_start.len()),
                );
                rev.revision_id = self.take_revision_id();
                rev.session_id = self.session_id;
                rev.committed_revision_id = self.committed_revision_id;
                rev
            }
            None => {
                let replace_range = Utf8ByteRange::from_values(
                    self.replace_start.value(),
                    self.replace_end_exclusive.value(),
                );
                let mut rev = CompositionVisualRevision::new(
                    self.committed_text_at_start.clone(),
                    replace_range,
                    new_preedit_text.clone(),
                    Utf8ByteRange::from_start_len(0, self.committed_text_at_start.len()),
                );
                rev.revision_id = self.take_revision_id();
                rev.session_id = self.session_id;
                rev.committed_revision_id = self.committed_revision_id;
                rev
            }
        };
        self.preedit_text = new_preedit_text;
        self.preedit_cursor_offset = Utf8ByteOffset::unchecked(new_preedit_cursor_offset);
        self.current_visual_revision = Some(new_revision.clone());
        self.last_submitted_generation = self.last_submitted_generation.next();
        new_revision
    }

    /// 通过 setComposingRegion 更新替换范围。
    ///
    /// `start`/`end` 为 committed 正文 UTF-8 byte offset（半开区间），
    /// 会被 clamp 到 committed_text_at_start.len()，并自动交换保证 start <= end。
    /// #517: 只有 setComposingRegion 或平台明确给出替换范围时才能修改 replaceRange。
    pub fn set_composing_region(&mut self, start: usize, end: usize) {
        self.replace_start =
            Utf8ByteOffset::unchecked(start.min(self.committed_text_at_start.len()));
        self.replace_end_exclusive =
            Utf8ByteOffset::unchecked(end.min(self.committed_text_at_start.len()));
        if self.replace_start.value() > self.replace_end_exclusive.value() {
            std::mem::swap(&mut self.replace_start, &mut self.replace_end_exclusive);
        }
    }

    /// 会话是否活跃（有预输入文本或有视觉修订）。
    pub fn is_active(&self) -> bool {
        !self.preedit_text.is_empty() || self.current_visual_revision.is_some()
    }

    /// 获取当前 composition_replace_range。
    pub fn composition_replace_range(&self) -> Option<Utf8ByteRange> {
        if self.replace_start == self.replace_end_exclusive && self.preedit_text.is_empty() {
            None
        } else {
            Utf8ByteRange::from_values(
                self.replace_start.value(),
                self.replace_end_exclusive.value(),
            )
        }
    }

    /// 构造当前虚拟文本。
    pub fn virtual_text(&self) -> String {
        let range_tuple = self
            .composition_replace_range()
            .map(|r| (r.start().value(), r.end().value()));
        build_virtual_text(
            &self.committed_text_at_start,
            range_tuple,
            &self.preedit_text,
        )
    }

    /// 预输入文本在 virtualText 中的字节范围。
    ///
    /// #517: 此范围只能表示 virtualText 中 preedit 的范围，
    /// 不能表示 committed replaceRange；两者必须分开命名和存储。
    pub fn preedit_byte_range_in_virtual_text(&self) -> (usize, usize) {
        let start = self.replace_start.value();
        let end = start + self.preedit_text.len();
        (start, end)
    }

    /// 提交预输入。
    ///
    /// #517: commitText 必须使用 session 的 replaceRange 替换 committed 正文。
    /// 返回 (composition_visual_revision, committed_text_after)。
    /// 如果 commit 文字与当前视觉文字相同，调用方可标记 is_visual_same 以避免重复吐字。
    pub fn commit(&mut self, commit_text: &str) -> (CompositionVisualRevision, String) {
        let composition_revision = self.current_visual_revision.clone().unwrap_or_else(|| {
            CompositionVisualRevision::new(
                self.committed_text_at_start.clone(),
                self.composition_replace_range(),
                self.preedit_text.clone(),
                Utf8ByteRange::from_start_len(0, self.committed_text_at_start.len()),
            )
        });

        let mut committed_after = self.committed_text_at_start.clone();
        committed_after.replace_range(
            self.replace_start.value()..self.replace_end_exclusive.value(),
            commit_text,
        );

        self.preedit_text.clear();
        self.preedit_cursor_offset = Utf8ByteOffset::unchecked(0);
        self.current_visual_revision = None;
        self.replace_start = Utf8ByteOffset::unchecked(0);
        self.replace_end_exclusive = Utf8ByteOffset::unchecked(0);

        (composition_revision, committed_after)
    }

    /// 取消预输入。
    ///
    /// #517: cancel 删除 preedit 并让后续正文回流。
    /// 返回取消前的 composition_visual_revision。
    pub fn cancel(&mut self) -> CompositionVisualRevision {
        let composition_revision = self.current_visual_revision.clone().unwrap_or_else(|| {
            CompositionVisualRevision::new(
                self.committed_text_at_start.clone(),
                self.composition_replace_range(),
                self.preedit_text.clone(),
                Utf8ByteRange::from_start_len(0, self.committed_text_at_start.len()),
            )
        });

        self.preedit_text.clear();
        self.preedit_cursor_offset = Utf8ByteOffset::unchecked(0);
        self.current_visual_revision = None;
        self.replace_start = Utf8ByteOffset::unchecked(0);
        self.replace_end_exclusive = Utf8ByteOffset::unchecked(0);

        composition_revision
    }

    /// 清除会话。
    pub fn clear(&mut self) {
        self.preedit_text.clear();
        self.preedit_cursor_offset = Utf8ByteOffset::unchecked(0);
        self.current_visual_revision = None;
        self.replace_start = Utf8ByteOffset::unchecked(0);
        self.replace_end_exclusive = Utf8ByteOffset::unchecked(0);
        self.last_submitted_generation = EditorSessionGeneration::initial();
    }

    fn take_revision_id(&mut self) -> u64 {
        let id = self.next_revision_id;
        self.next_revision_id = self.next_revision_id.saturating_add(1);
        id
    }
}
