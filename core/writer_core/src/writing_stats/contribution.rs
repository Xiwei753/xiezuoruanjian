//! # 贡献来源事件
//!
//! 记录星图、标签、超链接等对象的真实修改者。它与章节正文的
//! [`super::WritingInputEvent`] 是两套事实：正文事件回答「敲了多少字」，
//! 本模块回答「谁改了哪个对象，以及当前保留的文字来自谁」。
//!
//! 对象事件只由 Core 的业务写入口在**持久化成功之后**记录。同步只搬运
//! 原始事件，不按搬运设备重写 actor；UI 默认是 `Human`，MCP 服务端固定
//! 使用 [`MutationContext::mcp`]，工具参数不能自称 Human。

use std::cell::RefCell;

use serde::{Deserialize, Serialize};
use similar::{capture_diff_slices, Algorithm, DiffOp};

/// 对象修改的真实操作者。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationActor {
    Human,
    Ai,
    Unknown,
    System,
}

impl MutationActor {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Ai => "ai",
            Self::Unknown => "unknown",
            Self::System => "system",
        }
    }
}

/// 修改从哪个入口发起。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationEntrypoint {
    Ui,
    Mcp,
    Import,
    Sync,
    System,
}

impl MutationEntrypoint {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ui => "ui",
            Self::Mcp => "mcp",
            Self::Import => "import",
            Self::Sync => "sync",
            Self::System => "system",
        }
    }
}

/// 可信的修改上下文。字段私有，只能通过受控构造函数创建；
/// MCP 工具参数无法直接构造一个 `Human` 上下文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationContext {
    actor: MutationActor,
    entrypoint: MutationEntrypoint,
    transaction_id: String,
    occurred_at_ms: i64,
}

impl MutationContext {
    pub fn new(
        actor: MutationActor,
        entrypoint: MutationEntrypoint,
        transaction_id: Option<String>,
    ) -> Self {
        Self {
            actor,
            entrypoint,
            transaction_id: transaction_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            occurred_at_ms: chrono::Utc::now().timestamp_millis(),
        }
    }

    /// 本机 UI 操作：真实人工编辑。
    pub fn human_ui() -> Self {
        Self::new(MutationActor::Human, MutationEntrypoint::Ui, None)
    }

    /// MCP 服务端操作：服务端固定标记为 AI，不读取工具调用参数。
    pub fn mcp() -> Self {
        Self::new(MutationActor::Ai, MutationEntrypoint::Mcp, None)
    }

    /// 外部导入/无法确认来源：不得推断成人工或 AI。
    pub fn import() -> Self {
        Self::new(MutationActor::Unknown, MutationEntrypoint::Import, None)
    }

    pub fn system() -> Self {
        Self::new(MutationActor::System, MutationEntrypoint::System, None)
    }

    pub fn with_transaction_id(mut self, transaction_id: impl Into<String>) -> Self {
        self.transaction_id = transaction_id.into();
        self
    }

    pub fn actor(&self) -> MutationActor {
        self.actor
    }

    pub fn entrypoint(&self) -> MutationEntrypoint {
        self.entrypoint
    }

    pub fn transaction_id(&self) -> &str {
        &self.transaction_id
    }

    pub fn occurred_at_ms(&self) -> i64 {
        self.occurred_at_ms
    }
}

thread_local! {
    static ACTIVE_CONTEXT: RefCell<Option<MutationContext>> = const { RefCell::new(None) };
}

/// 当前线程的业务写入口上下文。未显式设置时按本机 UI 人工操作处理。
pub fn current_mutation_context() -> MutationContext {
    ACTIVE_CONTEXT.with(|slot| {
        slot.borrow()
            .clone()
            .unwrap_or_else(MutationContext::human_ui)
    })
}

/// 在指定上下文内执行业务写操作。MCP 适配层用它把调用标记为 AI。
pub fn with_mutation_context<T>(context: MutationContext, operation: impl FnOnce() -> T) -> T {
    struct Reset(Option<MutationContext>);
    impl Drop for Reset {
        fn drop(&mut self) {
            ACTIVE_CONTEXT.with(|slot| {
                *slot.borrow_mut() = self.0.take();
            });
        }
    }

    let previous = ACTIVE_CONTEXT.with(|slot| slot.replace(Some(context)));
    let _reset = Reset(previous);
    operation()
}

/// 对象种类。分类只用于统计和权限，不改变对象本身的数据结构。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContributionObjectType {
    StarMap,
    StarMapNode,
    StarMapEdge,
    StarMapEmbed,
    StarMapLink,
    Tag,
    TagBinding,
    TextAnchor,
    ObjectLink,
    NamedEntry,
}

impl ContributionObjectType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StarMap => "starmap",
            Self::StarMapNode => "starmap_node",
            Self::StarMapEdge => "starmap_edge",
            Self::StarMapEmbed => "starmap_embed",
            Self::StarMapLink => "starmap_link",
            Self::Tag => "tag",
            Self::TagBinding => "tag_binding",
            Self::TextAnchor => "text_anchor",
            Self::ObjectLink => "object_link",
            Self::NamedEntry => "named_entry",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContributionOperation {
    Create,
    Update,
    Delete,
    Link,
    Unlink,
}

impl ContributionOperation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::Link => "link",
            Self::Unlink => "unlink",
        }
    }
}

/// 修改前/后的对象快照。`text` 只对真正含正文的对象设置，
/// 标签、连线、超链接不伪造文字贡献。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContributionObjectSnapshot {
    pub version: String,
    pub summary: String,
    pub text: Option<String>,
}

impl ContributionObjectSnapshot {
    pub fn text(
        version: impl Into<String>,
        summary: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            version: version.into(),
            summary: summary.into(),
            text: Some(text.into()),
        }
    }

    pub fn object(version: impl Into<String>, summary: impl Into<String>) -> Self {
        Self {
            version: version.into(),
            summary: summary.into(),
            text: None,
        }
    }
}

/// 当前真实对象中的可归属文字快照。
///
/// 投影「当前保留内容来源」时必须与磁盘上的当前对象对账，不能只相信事件流：
/// 对象可能被删除、被旧版本覆盖，或由没有贡献事件的旧代码写入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentTextSnapshot {
    pub project_id: String,
    pub object_type: ContributionObjectType,
    pub object_id: String,
    pub container_id: Option<String>,
    pub version: String,
    pub text: String,
}

/// 一次成功对象修改的来源事件。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributionEvent {
    pub event_id: String,
    /// 由 store 写入时补齐；同步来的事件保留原分区。
    #[serde(default)]
    pub origin_device_id: String,
    pub occurred_at_ms: i64,
    #[serde(default)]
    pub local_date: String,
    pub project_id: String,
    pub object_type: ContributionObjectType,
    pub object_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_id: Option<String>,
    pub operation: ContributionOperation,
    pub actor: MutationActor,
    pub entrypoint: MutationEntrypoint,
    /// 同一次业务事务内的多个对象事件共享此 ID。
    pub transaction_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_summary: Option<String>,
    #[serde(default)]
    pub inserted_chars: u32,
    #[serde(default)]
    pub deleted_chars: u32,
    #[serde(default)]
    pub modified_chars: u32,
    /// 仅含正文的对象使用：用于增量维护「当前保留内容来源」。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_after: Option<String>,
}

impl ContributionEvent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        context: &MutationContext,
        project_id: impl Into<String>,
        object_type: ContributionObjectType,
        object_id: impl Into<String>,
        operation: ContributionOperation,
        container_id: Option<String>,
        before: Option<ContributionObjectSnapshot>,
        after: Option<ContributionObjectSnapshot>,
    ) -> Self {
        let (inserted, deleted, modified) = text_delta(
            before
                .as_ref()
                .and_then(|snapshot| snapshot.text.as_deref()),
            after.as_ref().and_then(|snapshot| snapshot.text.as_deref()),
        );
        let object_id = object_id.into();
        let after_version = after.as_ref().map(|snapshot| snapshot.version.clone());
        let event_id = format!(
            "{}:{}:{}:{}:{}",
            context.transaction_id(),
            object_type.as_str(),
            object_id,
            operation.as_str(),
            after_version.as_deref().unwrap_or("deleted")
        );
        Self {
            event_id,
            origin_device_id: String::new(),
            occurred_at_ms: context.occurred_at_ms(),
            local_date: String::new(),
            project_id: project_id.into(),
            object_type,
            object_id,
            container_id,
            operation,
            actor: context.actor(),
            entrypoint: context.entrypoint(),
            transaction_id: context.transaction_id().to_string(),
            before_version: before.as_ref().map(|snapshot| snapshot.version.clone()),
            after_version,
            before_summary: before.as_ref().map(|snapshot| snapshot.summary.clone()),
            after_summary: after.as_ref().map(|snapshot| snapshot.summary.clone()),
            inserted_chars: inserted,
            deleted_chars: deleted,
            modified_chars: modified,
            text_before: before.and_then(|snapshot| snapshot.text),
            text_after: after.and_then(|snapshot| snapshot.text),
        }
    }

    pub fn is_textual(&self) -> bool {
        self.text_before.is_some() || self.text_after.is_some()
    }
}

/// 字符级差异片段。索引均为**字符**下标，不是 UTF-8 字节偏移。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CharDiffOp {
    Equal {
        old_start: usize,
        new_start: usize,
        len: usize,
    },
    Delete {
        old_start: usize,
        old_len: usize,
    },
    Insert {
        new_start: usize,
        new_len: usize,
    },
    Replace {
        old_start: usize,
        old_len: usize,
        new_start: usize,
        new_len: usize,
    },
}

/// 对前后文本做确定性的多段字符 diff。
///
/// 只使用公共前缀/后缀会把「甲X丙丁Y己」中的「丙丁」也算成一次替换；
/// 这里用 Myers 算法保留所有未变化的连续片段。
pub(crate) fn char_diff_ops(before: &str, after: &str) -> Vec<CharDiffOp> {
    let before: Vec<char> = before.chars().collect();
    let after: Vec<char> = after.chars().collect();
    capture_diff_slices(Algorithm::Myers, &before, &after)
        .into_iter()
        .map(|op| match op {
            DiffOp::Equal {
                old_index,
                new_index,
                len,
            } => CharDiffOp::Equal {
                old_start: old_index,
                new_start: new_index,
                len,
            },
            DiffOp::Delete {
                old_index,
                old_len,
                ..
            } => CharDiffOp::Delete {
                old_start: old_index,
                old_len,
            },
            DiffOp::Insert {
                new_index, new_len, ..
            } => CharDiffOp::Insert {
                new_start: new_index,
                new_len,
            },
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => CharDiffOp::Replace {
                old_start: old_index,
                old_len,
                new_start: new_index,
                new_len,
            },
        })
        .collect()
}

/// 基于多段字符差异统计增删量；未变化的片段不归本次操作者。
pub fn text_delta(before: Option<&str>, after: Option<&str>) -> (u32, u32, u32) {
    let mut inserted = 0usize;
    let mut deleted = 0usize;
    for op in char_diff_ops(before.unwrap_or_default(), after.unwrap_or_default()) {
        match op {
            CharDiffOp::Equal { .. } => {}
            CharDiffOp::Delete { old_len, .. } => deleted += old_len,
            CharDiffOp::Insert { new_len, .. } => inserted += new_len,
            CharDiffOp::Replace {
                old_len, new_len, ..
            } => {
                deleted += old_len;
                inserted += new_len;
            }
        }
    }
    let inserted = u32::try_from(inserted).unwrap_or(u32::MAX);
    let deleted = u32::try_from(deleted).unwrap_or(u32::MAX);
    (inserted, deleted, inserted.min(deleted))
}

/// 应用一条文本事件，返回对象的最新字符来源。
///
/// 未变化的字符保留原有来源；新增字符归本次 actor；删除字符直接移除。
/// 多段修改逐段处理，AI 只改一个词时不会把整篇节点都算成 AI。
pub fn apply_text_sources(current: &mut Vec<(char, MutationActor)>, event: &ContributionEvent) {
    let Some(after_text) = event.text_after.as_deref() else {
        current.clear();
        return;
    };
    let before_text = event.text_before.as_deref().unwrap_or_default();
    let before: Vec<char> = before_text.chars().collect();
    let after: Vec<char> = after_text.chars().collect();
    if current.len() != before.len() {
        // 事件链对不上（旧数据缺失/外部改盘）时，不能借用错位的来源。
        current.clear();
        current.extend(before.iter().copied().map(|ch| (ch, MutationActor::Unknown)));
    }
    let mut updated = Vec::with_capacity(after.len());
    for op in char_diff_ops(before_text, after_text) {
        match op {
            CharDiffOp::Equal {
                old_start, len, ..
            } => {
                updated.extend(current[old_start..old_start + len].iter().copied());
            }
            CharDiffOp::Delete { .. } => {}
            CharDiffOp::Insert { new_start, new_len } => {
                updated.extend(
                    after[new_start..new_start + new_len]
                        .iter()
                        .copied()
                        .map(|ch| (ch, event.actor)),
                );
            }
            CharDiffOp::Replace {
                new_start, new_len, ..
            } => {
                updated.extend(
                    after[new_start..new_start + new_len]
                        .iter()
                        .copied()
                        .map(|ch| (ch, event.actor)),
                );
            }
        }
    }
    *current = updated;
}
