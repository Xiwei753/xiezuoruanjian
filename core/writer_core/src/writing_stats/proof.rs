//! 手写证明：可持久化、可校验的逐次编辑事务。
//!
//! 证明事实与写作统计事实是同一条 [`WritingInputEvent`]：事件上的
//! `proof` 字段保存实际应用的 [`EditorChange`] 列表和哈希链。旧事件没有
//! `proof`，仍按旧字段读取统计，但不能逐字回放。
//!
//! 哈希分两层：
//! - `before_content_hash` / `after_content_hash` 是“上一链摘要 + 本次变更”
//!   的命令链摘要，只用于校验事件顺序与变更连续性；它不是全文 SHA。
//! - `before_text_sha256` / `after_text_sha256` 是 Core 在应用事务前后对
//!   **真实正文**计算的 SHA-256，用来把事件锚定到具体文本。
//! - checkpoint 里的 `text_sha256` 是某个完整正文快照的 SHA-256。
//!
//! 哈希编码带明确版本前缀，字段用长度前缀拼接，避免分隔符歧义。校验只能
//! 说明「这份记录从记录起点起前后一致」，不宣称证明输入者一定是人类。

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::editor::transaction::{
    EditorChange, EditorSelection, EditorTransactionCause,
};
use crate::writing_stats::WritingInputEvent;

pub const PROOF_FORMAT_VERSION: u32 = 1;
pub const EVENT_HASH_VERSION: &str = "sujian-event-v1";
pub const CONTENT_CHAIN_VERSION: &str = "sujian-chain-v1";
pub const TEXT_HASH_VERSION: &str = "sha256:v1";

/// 一次成功提交的编辑事务的证明细节。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProofEditPayload {
    pub format_version: u32,
    /// 来源设备内的持久递增序号。跨设备不按毫秒交错，回放以设备内序号为准。
    pub seq: u64,
    /// 真实前台写作会话 ID（与 `WritingActivitySegment` 同源）。
    #[serde(default)]
    pub session_id: String,
    /// 记录起点/正文基线标识，对应一个 checkpoint。
    #[serde(default)]
    pub baseline_id: String,
    /// 本事件是否开启新的链（旧会话首次记录、或正文被显式重载）。
    #[serde(default)]
    pub starts_new_chain: bool,
    pub base_revision: u64,
    pub new_revision: u64,
    pub cause: EditorTransactionCause,
    /// 本次事务实际执行的插入/删除，按执行顺序，坐标为执行时的 UTF-8 byte 偏移。
    pub changes: Vec<EditorChange>,
    pub old_selection: EditorSelection,
    pub new_selection: EditorSelection,
    pub before_content_hash: String,
    pub after_content_hash: String,
    /// 事务应用前真实正文的全文 SHA-256；旧事件为空，回放时跳过强校验。
    #[serde(default)]
    pub before_text_sha256: String,
    /// 事务应用后真实正文的全文 SHA-256。
    #[serde(default)]
    pub after_text_sha256: String,
    /// 同设备、同章节上一条事件的 `event_hash`；新链为空。
    #[serde(default)]
    pub prev_event_hash: String,
    pub event_hash: String,
    /// 同步导致正文变化时，依赖的远端事件/设备；人工输入时为空。
    #[serde(default)]
    pub sync_source_device_id: String,
    /// 同步导入/外部变更的父事件引用。
    #[serde(default)]
    pub parent_event_id: String,
}

impl ProofEditPayload {
    /// 是否有可回放的正文变更。纯光标/选区事务不应产生证明事件。
    pub fn has_content_change(&self) -> bool {
        self.changes
            .iter()
            .any(|change| !change.text().is_empty())
    }
}

/// 事件链校验结果。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProofVerification {
    pub valid: bool,
    pub events_checked: usize,
    pub first_seq: u64,
    pub last_seq: u64,
    pub final_text: String,
    pub final_selection_anchor: u32,
    pub final_selection_head: u32,
    pub errors: Vec<String>,
}

/// 规范化 SHA-256 文本摘要。
pub fn text_sha256(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    format!("{TEXT_HASH_VERSION}:{}", hex(&digest))
}

/// 空正文的链式内容摘要。
pub fn empty_content_hash() -> String {
    chain_hash(CONTENT_CHAIN_VERSION, &[])
}

/// 把完整正文作为新链起点。
pub fn content_hash_of_text(text: &str) -> String {
    chain_hash(CONTENT_CHAIN_VERSION, text.as_bytes())
}

/// 在已有链式摘要上追加一次事务的实际变更。
pub fn chain_content_hash(before: &str, changes: &[EditorChange]) -> String {
    let mut encoder = FieldEncoder::new();
    encoder.field(before.as_bytes());
    encoder.field(&(changes.len() as u64).to_le_bytes());
    for change in changes {
        match change {
            EditorChange::Insert { index, text } => {
                encoder.field(b"insert");
                encoder.field(&(index.value() as u64).to_le_bytes());
                encoder.field(text.as_bytes());
            }
            EditorChange::Delete { index, text } => {
                encoder.field(b"delete");
                encoder.field(&(index.value() as u64).to_le_bytes());
                encoder.field(text.as_bytes());
            }
        }
    }
    let digest = Sha256::digest(encoder.finish());
    format!("{TEXT_HASH_VERSION}:{}", hex(&digest))
}

/// 计算一条证明事件的哈希。
///
/// 事件哈希覆盖事件身份、来源、字数和 proof 字段（`event_hash` 本身除外）。
/// 重发同一条事件必须得到同一个哈希。
pub fn compute_event_hash(event: &WritingInputEvent) -> String {
    let proof = match event.proof.as_ref() {
        Some(proof) => proof,
        None => return String::new(),
    };
    let mut encoder = FieldEncoder::new();
    encoder.field(EVENT_HASH_VERSION.as_bytes());
    encoder.field(event.event_id.as_bytes());
    encoder.field(&event.timestamp_ms.to_le_bytes());
    encoder.field(event.device_id.as_bytes());
    encoder.field(event.platform.to_str_name().as_bytes());
    encoder.field(event.device_class.as_bytes());
    encoder.field(event.project_id.as_bytes());
    encoder.field(event.volume_id.as_bytes());
    encoder.field(event.chapter_id.as_bytes());
    encoder.field(format!("{:?}", event.source).as_bytes());
    encoder.field(&event.inserted_chars.to_le_bytes());
    encoder.field(&event.deleted_chars.to_le_bytes());
    encoder.field(&event.pasted_chars.to_le_bytes());
    encoder.field(&event.ai_inserted_chars.to_le_bytes());
    encoder.field(&proof.format_version.to_le_bytes());
    encoder.field(&proof.seq.to_le_bytes());
    encoder.field(proof.session_id.as_bytes());
    encoder.field(proof.baseline_id.as_bytes());
    encoder.field(&[u8::from(proof.starts_new_chain)]);
    encoder.field(&proof.base_revision.to_le_bytes());
    encoder.field(&proof.new_revision.to_le_bytes());
    encoder.field(format!("{:?}", proof.cause).as_bytes());
    encoder.field(&(proof.changes.len() as u64).to_le_bytes());
    for change in &proof.changes {
        match change {
            EditorChange::Insert { index, text } => {
                encoder.field(b"insert");
                encoder.field(&(index.value() as u64).to_le_bytes());
                encoder.field(text.as_bytes());
            }
            EditorChange::Delete { index, text } => {
                encoder.field(b"delete");
                encoder.field(&(index.value() as u64).to_le_bytes());
                encoder.field(text.as_bytes());
            }
        }
    }
    encoder.field(&(proof.old_selection.anchor.index.value() as u64).to_le_bytes());
    encoder.field(&(proof.old_selection.head.index.value() as u64).to_le_bytes());
    encoder.field(&(proof.new_selection.anchor.index.value() as u64).to_le_bytes());
    encoder.field(&(proof.new_selection.head.index.value() as u64).to_le_bytes());
    encoder.field(proof.before_content_hash.as_bytes());
    encoder.field(proof.after_content_hash.as_bytes());
    // 全文 SHA 是后加字段：旧事件两项都为空时保持旧哈希编码不变，
    // 否则历史事件链会在升级后整体失效。新事件两项始终非空。
    if !proof.before_text_sha256.is_empty() || !proof.after_text_sha256.is_empty() {
        encoder.field(proof.before_text_sha256.as_bytes());
        encoder.field(proof.after_text_sha256.as_bytes());
    }
    encoder.field(proof.prev_event_hash.as_bytes());
    encoder.field(proof.sync_source_device_id.as_bytes());
    encoder.field(proof.parent_event_id.as_bytes());
    let digest = Sha256::digest(encoder.finish());
    format!("{TEXT_HASH_VERSION}:{}", hex(&digest))
}

/// 按顺序把变更应用到正文。坐标是变更执行时的 UTF-8 byte 偏移。
pub fn apply_changes(text: &mut String, changes: &[EditorChange]) -> crate::Result<()> {
    for change in changes {
        let index = change.index().value();
        if index > text.len() {
            return Err(crate::Error::Other(format!(
                "proof change offset {index} 超出正文长度 {}",
                text.len()
            )));
        }
        if !text.is_char_boundary(index) {
            return Err(crate::Error::Other(format!(
                "proof change offset {index} 不在 UTF-8 字符边界"
            )));
        }
        match change {
            EditorChange::Insert { text: inserted, .. } => {
                text.insert_str(index, inserted);
            }
            EditorChange::Delete { text: deleted, .. } => {
                let end = index.saturating_add(deleted.len());
                if end > text.len() {
                    return Err(crate::Error::Other(format!(
                        "proof delete 区间 {index}..{end} 超出正文长度 {}",
                        text.len()
                    )));
                }
                if !text.is_char_boundary(end) {
                    return Err(crate::Error::Other(format!(
                        "proof delete 区间 {index}..{end} 不在 UTF-8 字符边界"
                    )));
                }
                if &text[index..end] != deleted {
                    return Err(crate::Error::Other(format!(
                        "proof delete 原文不匹配：offset {index}"
                    )));
                }
                text.replace_range(index..end, "");
            }
        }
    }
    Ok(())
}

/// 校验一段按设备内序号排列的事件链，并重建最终正文。
///
/// 只接受同一设备、同一章节的事件。`baseline_text` 是链起点之前的正文；
/// 如果事件标记 `starts_new_chain`，则以该基线重新开始。没有基线时按空正文
/// 开始，并在结果中记录错误，避免把缺失历史伪装成完整证明。
pub fn verify_event_chain(
    events: &[WritingInputEvent],
    baseline_text: &str,
) -> ProofVerification {
    let mut result = ProofVerification {
        final_text: baseline_text.to_string(),
        valid: true,
        ..ProofVerification::default()
    };
    let mut previous_event_hash = String::new();
    let mut previous_content_hash: Option<String> = None;
    let mut previous_seq: Option<u64> = None;
    let mut saw_event = false;

    for event in events {
        let Some(proof) = event.proof.as_ref() else {
            result.valid = false;
            result.errors.push(format!(
                "事件 {} 没有 proof 细节，不能逐字回放",
                event.event_id
            ));
            continue;
        };

        if !saw_event {
            result.first_seq = proof.seq;
            saw_event = true;
            if !proof.starts_new_chain {
                result.final_text = baseline_text.to_string();
                previous_content_hash = Some(content_hash_of_text(baseline_text));
            }
        }

        if let Some(seq) = previous_seq {
            if proof.seq <= seq {
                result.valid = false;
                result.errors.push(format!(
                    "事件 {} 的序号 {} 未递增（上一条 {}）",
                    event.event_id, proof.seq, seq
                ));
            }
        }
        previous_seq = Some(proof.seq);

        if proof.starts_new_chain {
            previous_event_hash.clear();
            previous_content_hash = Some(content_hash_of_text(&result.final_text));
        }

        if let Some(expected) = previous_content_hash.as_ref() {
            if &proof.before_content_hash != expected {
                result.valid = false;
                result.errors.push(format!(
                    "事件 {} 的 before_content_hash 与链上一条不一致",
                    event.event_id
                ));
            }
        }
        if &proof.prev_event_hash != &previous_event_hash {
            result.valid = false;
            result.errors.push(format!(
                "事件 {} 的 prev_event_hash 断裂",
                event.event_id
            ));
        }

        let expected_hash = compute_event_hash(event);
        if expected_hash != proof.event_hash {
            result.valid = false;
            result.errors.push(format!(
                "事件 {} 的 event_hash 校验失败",
                event.event_id
            ));
        }

        if !proof.before_text_sha256.is_empty()
            && proof.before_text_sha256 != text_sha256(&result.final_text)
        {
            result.valid = false;
            result.errors.push(format!(
                "事件 {} 的 before_text_sha256 与真实正文不一致",
                event.event_id
            ));
        }

        if let Err(error) = apply_changes(&mut result.final_text, &proof.changes) {
            result.valid = false;
            result.errors.push(format!("事件 {}: {error}", event.event_id));
        }
        let expected_after = chain_content_hash(&proof.before_content_hash, &proof.changes);
        if expected_after != proof.after_content_hash {
            result.valid = false;
            result.errors.push(format!(
                "事件 {} 的 after_content_hash 与变更不一致",
                event.event_id
            ));
        }
        if !proof.after_text_sha256.is_empty()
            && proof.after_text_sha256 != text_sha256(&result.final_text)
        {
            result.valid = false;
            result.errors.push(format!(
                "事件 {} 的 after_text_sha256 与真实正文不一致",
                event.event_id
            ));
        }

        previous_event_hash = proof.event_hash.clone();
        previous_content_hash = Some(proof.after_content_hash.clone());
        result.last_seq = proof.seq;
        result.final_selection_anchor = proof.new_selection.anchor.index.value() as u32;
        result.final_selection_head = proof.new_selection.head.index.value() as u32;
        result.events_checked += 1;
    }

    result
}

/// 长度前缀字段编码器，避免 `a|b` 与 `a` + `|b` 之类的歧义。
struct FieldEncoder {
    buffer: Vec<u8>,
}

impl FieldEncoder {
    fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    fn field(&mut self, bytes: &[u8]) {
        self.buffer
            .extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        self.buffer.extend_from_slice(bytes);
    }

    fn finish(self) -> Vec<u8> {
        self.buffer
    }
}

fn chain_hash(version: &str, bytes: &[u8]) -> String {
    let mut encoder = FieldEncoder::new();
    encoder.field(version.as_bytes());
    encoder.field(bytes);
    let digest = Sha256::digest(encoder.finish());
    format!("{TEXT_HASH_VERSION}:{}", hex(&digest))
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        output.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0'));
    }
    output
}
