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
        if !text.is_empty() {
            digest.root = Some(make_leaf(Arc::from(text), digest.next_priority));
            digest.next_priority = digest.next_priority.wrapping_add(1);
        }
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
                let priority = self.next_priority;
                self.next_priority = self.next_priority.wrapping_add(1);
                let node = make_leaf(Arc::from(text.as_str()), priority);
                self.root = merge(merge(left, Some(node)), right);
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
                self.root = merge(left, right);
                Ok(())
            }
        }
    }
}

fn make_leaf(text: Arc<str>, priority: u64) -> Arc<Node> {
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
        priority: splitmix64(priority),
        left: None,
        right: None,
        subtree_len: len,
        subtree_hash1: hash1,
        subtree_hash2: hash2,
        subtree_pow1: pow1,
        subtree_pow2: pow2,
    })
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
    hash1 = hash1.wrapping_mul(original.pow1).wrapping_add(original.hash1);
    hash2 = hash2.wrapping_mul(original.pow2).wrapping_add(original.hash2);
    pow1 = pow1.wrapping_mul(original.pow1);
    pow2 = pow2.wrapping_mul(original.pow2);
    if let Some(node) = right.as_ref() {
        hash1 = hash1.wrapping_mul(node.subtree_pow1).wrapping_add(node.subtree_hash1);
        hash2 = hash2.wrapping_mul(node.subtree_pow2).wrapping_add(node.subtree_hash2);
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
        return Ok((node.left.clone(), Some(rebuild(&node, None, node.right.clone()))));
    }
    if offset == left_len + node.len {
        return Ok((Some(rebuild(&node, node.left.clone(), None)), node.right.clone()));
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
