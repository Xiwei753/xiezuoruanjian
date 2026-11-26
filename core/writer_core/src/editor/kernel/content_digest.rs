//! 可增量更新的正文内容摘要。
//!
//! 编辑器每次按键都复制整篇正文再算 SHA-256 会把输入热路径变成 O(N)。这里用
//! 持久化 treap 维护正文分块：每个节点保存分块文本、长度和多项式内容摘要，
//! 子树摘要按「左 + 节点 + 右」在 O(1) 内拼接。编辑时只 split/merge 受影响的
//! 路径，根摘要随时 O(1) 读取；插入文本只对新增内容哈希一次。
//!
//! 根摘要与分块方式无关：多项式拼接满足结合律，因此同样的正文无论被切成
//! 多少块都得到同一个根值。回放/存储可以从完整正文一次性构建摘要，再用同一
//! 组 changes 增量推进。最终对「长度 + 双 128 位根值」再做 SHA-256，得到
//! `content-v1:` 前缀的稳定字符串。完整正文 SHA-256 仍只用于 checkpoint 和
//! 导出锚定（见 [`crate::writing_stats::proof::text_sha256`]）。

use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::editor::transaction::EditorChange;

/// 增量内容摘要值的前缀。与旧事件的完整正文 `sha256:v1:` 明确区分。
pub const CONTENT_DIGEST_VERSION: &str = "content-v1";

const BASE1: u128 = 0x9E37_79B1_85EB_CA87_9E37_79B1_85EB_CA87;
const BASE2: u128 = 0xC2B2_AE3D_27D4_EB4F_C2B2_AE3D_27D4_EB4F;

/// 每个叶子的最大 UTF-8 byte 长度。分块上限保证中间编辑只拆一个（或两个）
/// 有界叶子，不会对整章正文做 `Arc::from` 复制或重新哈希。
const CHUNK_MAX_BYTES: usize = 4096;

type NodeRef = Option<Arc<Node>>;

#[derive(Debug, Clone)]
struct Node {
    text: Arc<str>,
    len: usize,
    hash1: u128,
    hash2: u128,
    pow1: u128,
    pow2: u128,
    priority: u64,
    left: NodeRef,
    right: NodeRef,
    subtree_len: usize,
    subtree_hash1: u128,
    subtree_hash2: u128,
    subtree_pow1: u128,
    subtree_pow2: u128,
}

/// 章节正文的可增量内容摘要。`Clone` 只复制根引用，回滚/验证时开销 O(1)。
#[derive(Debug, Clone)]
pub struct ContentDigest {
    root: NodeRef,
    next_priority: u64,
}

impl Default for ContentDigest {
    fn default() -> Self {
        Self::new()
    }
}

impl ContentDigest {
    pub fn new() -> Self {
        Self {
            root: None,
            next_priority: 1,
        }
    }

    pub fn from_text(text: &str) -> Self {
        let mut digest = Self::new();
        let mut root = None;
        for chunk in chunk_text(text) {
            let node = make_leaf(Arc::from(chunk), digest.next_priority);
            digest.next_priority = digest.next_priority.wrapping_add(1);
            root = merge(root, Some(node));
        }
        digest.root = root;
        digest
    }

    pub fn len(&self) -> usize {
        self.root.as_ref().map_or(0, |node| node.subtree_len)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 当前根摘要（`content-v1:<sha256hex>`）。
    pub fn digest(&self) -> String {
        let (len, hash1, hash2) = self.root.as_ref().map_or((0, 0, 0), |node| {
            (node.subtree_len, node.subtree_hash1, node.subtree_hash2)
        });
        let mut hasher = Sha256::new();
        hasher.update(b"sujian-content-root-v1\0");
        hasher.update((len as u64).to_le_bytes());
        hasher.update(hash1.to_le_bytes());
        hasher.update(hash2.to_le_bytes());
        format!("{CONTENT_DIGEST_VERSION}:{}", hex(&hasher.finalize()))
    }

    /// 测试用：正文文本块（treap 节点）数量。
    ///
    /// treap 的每个节点持有一段正文；逐字输入若每字建一个节点，这个数字会随
    /// 输入次数线性增长。seam coalesce 后它应按 `正文长度 / CHUNK_MAX_BYTES`
    /// 有界增长。
    #[cfg(test)]
    pub(crate) fn leaf_count(&self) -> usize {
        fn count(node: &NodeRef) -> usize {
            match node {
                None => 0,
                Some(node) => 1 + count(&node.left) + count(&node.right),
            }
        }
        count(&self.root)
    }

    pub fn apply_changes(&mut self, changes: &[EditorChange]) -> crate::Result<()> {
        for change in changes {
            self.apply_change(change)?;
        }
        Ok(())
    }

    pub fn apply_change(&mut self, change: &EditorChange) -> crate::Result<()> {
        match change {
            EditorChange::Insert { index, text } => {
                let index = index.value();
                if index > self.len() {
                    return Err(crate::Error::Other(format!(
                        "内容摘要插入偏移 {} 超出正文长度 {}",
                        index,
                        self.len()
                    )));
                }
                let (left, right) = split(self.root.take(), index)?;
                let mut inserted = None;
                for chunk in chunk_text(text) {
                    let priority = self.next_priority;
                    self.next_priority = self.next_priority.wrapping_add(1);
                    inserted = merge(inserted, Some(make_leaf(Arc::from(chunk), priority)));
                }
                // 两次 seam join：left | inserted | right，边界相邻的有界叶子会
                // 合并，普通逐字输入不会一字一节点。
                self.root = merge_coalesced(merge_coalesced(left, inserted), right);
                Ok(())
            }
            EditorChange::Delete { index, text } => {
                let index = index.value();
                let end = index.saturating_add(text.len());
                if end > self.len() {
                    return Err(crate::Error::Other(format!(
                        "内容摘要删除区间 {}..{} 超出正文长度 {}",
                        index,
                        end,
                        self.len()
                    )));
                }
                let (left, rest) = split(self.root.take(), index)?;
                let (_, right) = split(rest, text.len())?;
                self.root = merge_coalesced(left, right);
                Ok(())
            }
        }
    }
}

/// 按 UTF-8 字符边界切分正文，每块不超过 [`CHUNK_MAX_BYTES`]。
///
/// 空正文返回空迭代；调用方按顺序建树，保证叶子长度恒定有界。
fn chunk_text(text: &str) -> impl Iterator<Item = &str> {
    let mut start = 0;
    std::iter::from_fn(move || {
        if start >= text.len() {
            return None;
        }
        let mut end = (start + CHUNK_MAX_BYTES).min(text.len());
        while end > start && !text.is_char_boundary(end) {
            end -= 1;
        }
        let chunk = &text[start..end];
        start = end;
        Some(chunk)
    })
}

/// 以已经过 splitmix64 的 priority 构造叶子（seam 合并复用邻居优先级时用）。
fn make_leaf_with_priority(text: Arc<str>, priority: u64) -> Arc<Node> {
    let (hash1, hash2) = hash_bytes(text.as_bytes());
    let (pow1, pow2) = (pow(BASE1, text.len()), pow(BASE2, text.len()));
    let len = text.len();
    Arc::new(Node {
        text,
        len,
        hash1,
        hash2,
        pow1,
        pow2,
        priority,
        left: None,
        right: None,
        subtree_len: len,
        subtree_hash1: hash1,
        subtree_hash2: hash2,
        subtree_pow1: pow1,
        subtree_pow2: pow2,
    })
}

fn make_leaf(text: Arc<str>, priority_seed: u64) -> Arc<Node> {
    make_leaf_with_priority(text, splitmix64(priority_seed))
}

fn rebuild(original: &Node, left: NodeRef, right: NodeRef) -> Arc<Node> {
    let left_len = left.as_ref().map_or(0, |node| node.subtree_len);
    let right_len = right.as_ref().map_or(0, |node| node.subtree_len);
    let subtree_len = left_len + original.len + right_len;

    let (mut hash1, mut hash2, mut pow1, mut pow2) = (
        left.as_ref().map_or(0, |node| node.subtree_hash1),
        left.as_ref().map_or(0, |node| node.subtree_hash2),
        left.as_ref().map_or(1, |node| node.subtree_pow1),
        left.as_ref().map_or(1, |node| node.subtree_pow2),
    );
    hash1 = hash1
        .wrapping_mul(original.pow1)
        .wrapping_add(original.hash1);
    hash2 = hash2
        .wrapping_mul(original.pow2)
        .wrapping_add(original.hash2);
    pow1 = pow1.wrapping_mul(original.pow1);
    pow2 = pow2.wrapping_mul(original.pow2);
    if let Some(node) = right.as_ref() {
        hash1 = hash1
            .wrapping_mul(node.subtree_pow1)
            .wrapping_add(node.subtree_hash1);
        hash2 = hash2
            .wrapping_mul(node.subtree_pow2)
            .wrapping_add(node.subtree_hash2);
        pow1 = pow1.wrapping_mul(node.subtree_pow1);
        pow2 = pow2.wrapping_mul(node.subtree_pow2);
    }

    Arc::new(Node {
        text: original.text.clone(),
        len: original.len,
        hash1: original.hash1,
        hash2: original.hash2,
        pow1: original.pow1,
        pow2: original.pow2,
        priority: original.priority,
        left,
        right,
        subtree_len,
        subtree_hash1: hash1,
        subtree_hash2: hash2,
        subtree_pow1: pow1,
        subtree_pow2: pow2,
    })
}

fn merge(left: NodeRef, right: NodeRef) -> NodeRef {
    match (left, right) {
        (None, right) => right,
        (left, None) => left,
        (Some(left), Some(right)) => {
            if left.priority >= right.priority {
                let new_right = merge(left.right.clone(), Some(right));
                Some(rebuild(&left, left.left.clone(), new_right))
            } else {
                let new_left = merge(Some(left), right.left.clone());
                Some(rebuild(&right, new_left, right.right.clone()))
            }
        }
    }
}

/// 相邻树的有界 seam 合并：把左树最右叶与右树最左叶合并成一个新叶子。
///
/// 只有两个边界文本块总长不超过 [`CHUNK_MAX_BYTES`] 时才合并；`Arc<str>` 由两段
/// 合法 UTF-8 字符串拼接而来，结果仍是合法 UTF-8（不会产生孤立代理项），因此
/// 无需额外校验。只复制这两个小块并重算该叶的 hash/pow；历史共享的子树 Arc
/// 原样复用，不原地修改已存在节点。priority 取两叶中较大者，treap 的堆序由
/// 标准 `merge` 重新保证。
fn merge_coalesced(left: NodeRef, right: NodeRef) -> NodeRef {
    let (Some(left_root), Some(right_root)) = (left.as_ref(), right.as_ref()) else {
        return merge(left, right);
    };
    let left_tail_len = rightmost_leaf_len(left_root);
    let right_head_len = leftmost_leaf_len(right_root);
    if left_tail_len + right_head_len > CHUNK_MAX_BYTES {
        return merge(left, right);
    }

    let left_split = left_root.subtree_len - left_tail_len;
    let Ok((left_rest, Some(left_tail))) = split(left.clone(), left_split) else {
        return merge(left, right);
    };
    let Ok((Some(right_head), right_rest)) = split(right.clone(), right_head_len) else {
        return merge(left, right);
    };

    let mut combined = String::with_capacity(left_tail.len + right_head.len);
    combined.push_str(&left_tail.text);
    combined.push_str(&right_head.text);
    let priority = left_tail.priority.max(right_head.priority);
    let leaf = make_leaf_with_priority(Arc::from(combined), priority);
    merge(left_rest, merge(Some(leaf), right_rest))
}

fn rightmost_leaf_len(node: &Node) -> usize {
    match node.right.as_ref() {
        Some(right) => rightmost_leaf_len(right),
        None => node.len,
    }
}

fn leftmost_leaf_len(node: &Node) -> usize {
    match node.left.as_ref() {
        Some(left) => leftmost_leaf_len(left),
        None => node.len,
    }
}

/// 按 UTF-8 byte 偏移拆分。偏移必须落在 char 边界。
fn split(root: NodeRef, offset: usize) -> crate::Result<(NodeRef, NodeRef)> {
    let Some(node) = root else {
        if offset == 0 {
            return Ok((None, None));
        }
        return Err(crate::Error::Other(format!(
            "内容摘要拆分偏移 {} 超出空正文",
            offset
        )));
    };
    let left_len = node.left.as_ref().map_or(0, |left| left.subtree_len);
    if offset < left_len {
        let (left, middle) = split(node.left.clone(), offset)?;
        return Ok((left, Some(rebuild(&node, middle, node.right.clone()))));
    }
    if offset > left_len + node.len {
        let (middle, right) = split(node.right.clone(), offset - left_len - node.len)?;
        return Ok((Some(rebuild(&node, node.left.clone(), middle)), right));
    }
    if offset == left_len {
        return Ok((
            node.left.clone(),
            Some(rebuild(&node, None, node.right.clone())),
        ));
    }
    if offset == left_len + node.len {
        return Ok((
            Some(rebuild(&node, node.left.clone(), None)),
            node.right.clone(),
        ));
    }

    let local = offset - left_len;
    if !node.text.is_char_boundary(local) {
        return Err(crate::Error::Other(format!(
            "内容摘要拆分偏移 {} 不在字符边界",
            offset
        )));
    }
    let (left_text, right_text) = node.text.split_at(local);
    let mut left_node = make_leaf(Arc::from(left_text), node.priority);
    let mut right_priority = node.priority.wrapping_add(0x9E37_79B9_7F4A_7C15);
    if right_priority == 0 {
        right_priority = 1;
    }
    let right_node = make_leaf(Arc::from(right_text), right_priority);
    left_node = merge(node.left.clone(), Some(left_node)).expect("left text node");
    let right_node = merge(Some(right_node), node.right.clone()).expect("right text node");
    Ok((Some(left_node), Some(right_node)))
}

fn hash_bytes(bytes: &[u8]) -> (u128, u128) {
    let mut hash1 = 0u128;
    let mut hash2 = 0u128;
    for byte in bytes {
        hash1 = hash1.wrapping_mul(BASE1).wrapping_add(u128::from(*byte));
        hash2 = hash2.wrapping_mul(BASE2).wrapping_add(u128::from(*byte));
    }
    (hash1, hash2)
}

fn pow(base: u128, mut exponent: usize) -> u128 {
    let mut result = 1u128;
    let mut factor = base;
    while exponent > 0 {
        if exponent & 1 == 1 {
            result = result.wrapping_mul(factor);
        }
        exponent >>= 1;
        if exponent > 0 {
            factor = factor.wrapping_mul(factor);
        }
    }
    result
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = value;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn hex(bytes: &[u8]) -> String {
    const LUT: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(LUT[usize::from(byte >> 4)]));
        out.push(char::from(LUT[usize::from(byte & 0x0f)]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::strong_types::Utf8ByteOffset;

    fn insert(offset: usize, text: &str) -> EditorChange {
        EditorChange::Insert {
            index: Utf8ByteOffset::unchecked(offset),
            text: text.to_string(),
        }
    }

    fn delete(offset: usize, text: &str) -> EditorChange {
        EditorChange::Delete {
            index: Utf8ByteOffset::unchecked(offset),
            text: text.to_string(),
        }
    }

    #[test]
    fn digest_is_independent_of_chunking_and_edit_sequence() {
        let text = "a".repeat(CHUNK_MAX_BYTES + 137) + "中文😀结尾";
        let reference = ContentDigest::from_text(&text);

        // 不同分块：按任意字符边界把正文分批 insert。
        let mut piecewise = ContentDigest::new();
        let mut chunk_start = 0usize;
        for (char_count, (byte_index, _)) in text.char_indices().enumerate() {
            if char_count % 59 == 0 && byte_index > chunk_start {
                assert!(piecewise
                    .apply_change(&insert(chunk_start, &text[chunk_start..byte_index]))
                    .is_ok());
                chunk_start = byte_index;
            }
        }
        assert!(piecewise
            .apply_change(&insert(chunk_start, &text[chunk_start..]))
            .is_ok());
        assert_eq!(piecewise.digest(), reference.digest());
        assert_eq!(piecewise.len(), text.len());

        // 不同编辑序列：删掉再插回同一个字符后回到同一根摘要。
        let Some(target) = text.find('中') else {
            panic!("test text must contain 中");
        };
        let mut edited = ContentDigest::from_text(&text);
        assert!(edited.apply_change(&delete(target, "中")).is_ok());
        assert!(edited.apply_change(&insert(target, "中")).is_ok());
        assert_eq!(edited.digest(), reference.digest());
        assert_eq!(edited.len(), text.len());
    }

    #[test]
    fn char_by_char_typing_keeps_leaf_count_bounded() {
        let mut digest = ContentDigest::new();
        let mut text = String::new();
        for _ in 0..(CHUNK_MAX_BYTES * 2 + 17) {
            assert!(digest.apply_change(&insert(text.len(), "字")).is_ok());
            text.push('字');
        }
        // 每 4096 byte 才应形成一个新的有界叶子；允许常数个边界叶。
        let expected_leaves = text.len().div_ceil(CHUNK_MAX_BYTES);
        assert!(
            digest.leaf_count() <= expected_leaves + 2,
            "leaf_count={} expected<={} len={}",
            digest.leaf_count(),
            expected_leaves + 2,
            text.len()
        );
        assert_eq!(digest.digest(), ContentDigest::from_text(&text).digest());
    }

    #[test]
    fn adjacent_leaves_coalesce_but_never_exceed_chunk_limit() {
        let mut digest = ContentDigest::new();
        assert!(digest.apply_change(&insert(0, "abc")).is_ok());
        assert!(digest.apply_change(&insert(3, "def")).is_ok());
        assert_eq!(digest.leaf_count(), 1);
        assert_eq!(digest.len(), 6);
        assert_eq!(digest.digest(), ContentDigest::from_text("abcdef").digest());

        // 超过 CHUNK_MAX_BYTES 的 seam 不合并，叶子仍然有界。
        let long = "x".repeat(CHUNK_MAX_BYTES);
        let mut large = ContentDigest::new();
        assert!(large.apply_change(&insert(0, &long)).is_ok());
        assert!(large.apply_change(&insert(long.len(), "y")).is_ok());
        assert_eq!(large.leaf_count(), 2);
        assert_eq!(
            large.digest(),
            ContentDigest::from_text(&format!("{long}y")).digest()
        );
    }
}
