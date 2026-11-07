//! 星图回放状态承诺。
//!
//! 用按对象 ID 哈希定位的稀疏 Merkle 树承诺完整图状态。每个对象叶子包含
//! 对象内容摘要以及同类型对象中的前驱/后继 ID，因此插入、删除或修改对象
//! 只需要更新该对象及相邻对象的叶子，根与插入顺序无关。检查点可以记录
//! 顺序验证得到的状态根，随机跳转时只加载目标快照并重建一次承诺即可核对，
//! 不必重新执行 0..N 的全部 delta。

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::starmap::types::{StarMapEdge, StarMapEmbed, StarMapGraph, StarMapLink, StarMapNode};
use crate::starmap::StarMapMeta;
use crate::writing_stats::contribution::{ContributionObjectType, ContributionOperation};
use crate::writing_stats::starmap_proof::{
    proof_object_sha256, state_sha256, StarMapProofDelta, StarMapProofObject, StarMapProofState,
};

const TREE_DEPTH: u8 = 128;

/// 单个类型对象的稳定顺序集合。
#[derive(Clone)]
struct OrderedObjects<T> {
    items: HashMap<String, (u64, T)>,
    order: BTreeMap<u64, String>,
}

impl<T> Default for OrderedObjects<T> {
    fn default() -> Self {
        Self {
            items: HashMap::new(),
            order: BTreeMap::new(),
        }
    }
}

impl<T: Clone> OrderedObjects<T> {
    fn get(&self, id: &str) -> Option<&T> {
        self.items.get(id).map(|(_, item)| item)
    }

    fn order_of(&self, id: &str) -> Option<u64> {
        self.items.get(id).map(|(order, _)| *order)
    }

    fn prev_id(&self, order: u64) -> String {
        self.order
            .range(..order)
            .next_back()
            .map(|(_, id)| id.clone())
            .unwrap_or_default()
    }

    fn next_id(&self, order: u64) -> String {
        self.order
            .range((std::ops::Bound::Excluded(order), std::ops::Bound::Unbounded))
            .next()
            .map(|(_, id)| id.clone())
            .unwrap_or_default()
    }

    /// 插入或更新；返回旧值、实际 order 和被影响的邻居 ID。
    fn upsert(&mut self, id: String, value: T, next_order: &mut u64) -> (Option<T>, Vec<String>) {
        if let Some((order, existing)) = self.items.get_mut(&id) {
            let old = std::mem::replace(existing, value);
            return (Some(old), vec![id]);
        }
        let order = *next_order;
        *next_order = next_order.saturating_add(1);
        let prev = self.prev_id(order);
        self.items.insert(id.clone(), (order, value));
        self.order.insert(order, id.clone());
        let mut affected = vec![id];
        if !prev.is_empty() {
            affected.push(prev);
        }
        (None, affected)
    }

    /// 删除；返回旧值和被影响的邻居 ID。
    fn remove(&mut self, id: &str) -> (Option<T>, Vec<String>) {
        let Some((order, value)) = self.items.remove(id) else {
            return (None, Vec::new());
        };
        self.order.remove(&order);
        let mut affected = Vec::new();
        let prev = self.prev_id(order);
        let next = self.next_id(order);
        if !prev.is_empty() {
            affected.push(prev);
        }
        if !next.is_empty() {
            affected.push(next);
        }
        (Some(value), affected)
    }

    fn sorted_values(&self) -> Vec<T> {
        self.order
            .values()
            .filter_map(|id| self.items.get(id).map(|(_, value)| value.clone()))
            .collect()
    }
}

/// 稀疏 Merkle 树：深度固定 128（对象 ID 哈希前 128 位），节点按前缀索引。
#[derive(Clone)]
struct SparseMerkle {
    leaves: HashMap<u128, [u8; 32]>,
    nodes: HashMap<(u8, u128), [u8; 32]>,
    empty: Vec<[u8; 32]>,
}

impl SparseMerkle {
    fn new() -> Self {
        let mut empty = vec![[0u8; 32]; usize::from(TREE_DEPTH) + 1];
        for depth in (0..TREE_DEPTH).rev() {
            empty[usize::from(depth)] = branch_hash(
                &empty[usize::from(depth) + 1],
                &empty[usize::from(depth) + 1],
            );
        }
        Self {
            leaves: HashMap::new(),
            nodes: HashMap::new(),
            empty,
        }
    }

    fn root(&self) -> [u8; 32] {
        self.nodes.get(&(0, 0)).copied().unwrap_or(self.empty[0])
    }

    fn node_hash(&self, depth: u8, prefix: u128) -> [u8; 32] {
        if depth == TREE_DEPTH {
            return self
                .leaves
                .get(&prefix)
                .map(|value| leaf_hash(prefix, value))
                .unwrap_or(self.empty[usize::from(depth)]);
        }
        self.nodes
            .get(&(depth, prefix))
            .copied()
            .unwrap_or(self.empty[usize::from(depth)])
    }

    fn set_leaf(&mut self, key: u128, value: Option<[u8; 32]>) {
        let leaf_depth = usize::from(TREE_DEPTH);
        let mut current = match value {
            Some(value) => {
                self.leaves.insert(key, value);
                leaf_hash(key, &value)
            }
            None => {
                self.leaves.remove(&key);
                self.empty[leaf_depth]
            }
        };
        for depth in (0..TREE_DEPTH).rev() {
            let child_depth = depth + 1;
            let child_prefix = path(key, child_depth);
            let sibling_prefix = child_prefix ^ 1;
            let sibling = self.node_hash(child_depth, sibling_prefix);
            let (left, right) = if child_prefix & 1 == 0 {
                (current, sibling)
            } else {
                (sibling, current)
            };
            let parent_prefix = path(key, depth);
            let hash = branch_hash(&left, &right);
            if hash == self.empty[usize::from(depth)] {
                self.nodes.remove(&(depth, parent_prefix));
            } else {
                self.nodes.insert((depth, parent_prefix), hash);
            }
            current = hash;
        }
    }
}

fn path(key: u128, depth: u8) -> u128 {
    if depth == 0 {
        0
    } else {
        key >> (128 - u32::from(depth))
    }
}

fn branch_hash(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update([1u8]);
    hasher.update(left);
    hasher.update(right);
    hasher.finalize().into()
}

fn leaf_hash(key: u128, value: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update([0u8]);
    hasher.update(key.to_be_bytes());
    hasher.update(value);
    hasher.finalize().into()
}

fn hash_bytes(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

fn hash_serializable<T: Serialize>(prefix: &[u8], value: &T) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(prefix);
    hasher.update(serde_json::to_vec(value).unwrap_or_default());
    hasher.finalize().into()
}

fn key_for_bytes(bytes: &[u8]) -> u128 {
    let hash = hash_bytes(bytes);
    u128::from_be_bytes(hash[..16].try_into().unwrap_or([0u8; 16]))
}

fn key_for_object(kind: u8, id: &str) -> u128 {
    let mut bytes = Vec::with_capacity(id.len() + 2);
    bytes.push(b'o');
    bytes.push(kind);
    bytes.extend_from_slice(id.as_bytes());
    key_for_bytes(&bytes)
}

const KIND_NODE: u8 = 1;
const KIND_EDGE: u8 = 2;
const KIND_EMBED: u8 = 3;
const KIND_LINK: u8 = 4;

fn object_key(object_type: ContributionObjectType, id: &str) -> u128 {
    let kind = match object_type {
        ContributionObjectType::StarMapNode => KIND_NODE,
        ContributionObjectType::StarMapEdge => KIND_EDGE,
        ContributionObjectType::StarMapEmbed => KIND_EMBED,
        ContributionObjectType::StarMapLink => KIND_LINK,
        ContributionObjectType::StarMap => 0,
        // 星图证明不承诺标签、锚点等其它贡献对象。
        _ => 0xff,
    };
    key_for_object(kind, id)
}

fn object_leaf_value(
    kind: u8,
    id: &str,
    object: &StarMapProofObject,
    prev: &str,
    next: &str,
) -> [u8; 32] {
    let content = hash_bytes(&serde_json::to_vec(object).unwrap_or_default());
    let mut bytes = Vec::new();
    bytes.push(0x11);
    bytes.push(kind);
    bytes.extend_from_slice(&(id.len() as u32).to_le_bytes());
    bytes.extend_from_slice(id.as_bytes());
    bytes.extend_from_slice(&content);
    bytes.extend_from_slice(&(prev.len() as u32).to_le_bytes());
    bytes.extend_from_slice(prev.as_bytes());
    bytes.extend_from_slice(&(next.len() as u32).to_le_bytes());
    bytes.extend_from_slice(next.as_bytes());
    hash_bytes(&bytes)
}

fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// 可增量更新的完整图状态承诺。
#[derive(Clone)]
pub struct StateCommitment {
    schema_version: u32,
    starmap_id: String,
    meta: Option<StarMapMeta>,
    root_starmap_ids: Vec<String>,
    nodes: OrderedObjects<StarMapNode>,
    edges: OrderedObjects<StarMapEdge>,
    embeds: OrderedObjects<StarMapEmbed>,
    links: OrderedObjects<StarMapLink>,
    next_order: u64,
    tree: SparseMerkle,
}

impl StateCommitment {
    pub fn from_state(state: &StarMapProofState) -> Self {
        let mut commitment = Self {
            schema_version: state.graph.schema_version,
            starmap_id: state.graph.starmap_id.clone(),
            meta: state.meta.clone(),
            root_starmap_ids: state.root_starmap_ids.clone(),
            nodes: OrderedObjects::default(),
            edges: OrderedObjects::default(),
            embeds: OrderedObjects::default(),
            links: OrderedObjects::default(),
            next_order: 0,
            tree: SparseMerkle::new(),
        };
        for node in &state.graph.nodes {
            commitment
                .nodes
                .upsert(node.id.clone(), node.clone(), &mut commitment.next_order);
        }
        for edge in &state.graph.edges {
            commitment
                .edges
                .upsert(edge.id.clone(), edge.clone(), &mut commitment.next_order);
        }
        for embed in &state.graph.embeds {
            commitment.embeds.upsert(
                embed.instance_id.clone(),
                embed.clone(),
                &mut commitment.next_order,
            );
        }
        for link in &state.graph.links {
            commitment.links.upsert(
                link.link_id.clone(),
                link.clone(),
                &mut commitment.next_order,
            );
        }
        commitment.refresh_fixed_leaves();
        commitment.refresh_all_object_leaves();
        commitment
    }

    pub fn root_hex(&self) -> String {
        hex(&self.tree.root())
    }

    pub fn state_sha256(&self) -> String {
        state_sha256(&self.to_state())
    }

    pub fn object_for_delta(&self, delta: &StarMapProofDelta) -> Option<StarMapProofObject> {
        match delta.object_type {
            ContributionObjectType::StarMap => self
                .meta
                .clone()
                .map(|meta| StarMapProofObject::Meta { meta }),
            ContributionObjectType::StarMapNode => self
                .nodes
                .get(&delta.object_id)
                .cloned()
                .map(|node| StarMapProofObject::Node { node }),
            ContributionObjectType::StarMapEdge => self
                .edges
                .get(&delta.object_id)
                .cloned()
                .map(|edge| StarMapProofObject::Edge { edge }),
            ContributionObjectType::StarMapEmbed => self
                .embeds
                .get(&delta.object_id)
                .cloned()
                .map(|embed| StarMapProofObject::Embed { embed }),
            ContributionObjectType::StarMapLink => self
                .links
                .get(&delta.object_id)
                .cloned()
                .map(|link| StarMapProofObject::Link { link }),
            _ => None,
        }
    }

    pub fn apply_delta(&mut self, delta: &StarMapProofDelta) -> Result<()> {
        if delta.starmap_id != self.starmap_id {
            return Err(Error::Other(format!(
                "starmap proof delta belongs to {:?}, expected {:?}",
                delta.starmap_id, self.starmap_id
            )));
        }
        if let Some(roots) = &delta.roots_after {
            self.root_starmap_ids = roots.clone();
            self.refresh_fixed_leaves();
        }
        let object_type = delta.object_type;
        match object_type {
            ContributionObjectType::StarMap => {
                if let Some(StarMapProofObject::Meta { meta }) = &delta.after {
                    self.meta = Some(meta.clone());
                } else if delta.after.is_none() {
                    self.meta = None;
                    self.root_starmap_ids.clear();
                }
                self.refresh_fixed_leaves();
            }
            ContributionObjectType::StarMapNode => {
                if let Some(StarMapProofObject::Node { node }) = &delta.after {
                    let affected = self
                        .nodes
                        .upsert(node.id.clone(), node.clone(), &mut self.next_order)
                        .1;
                    self.refresh_object_leaves(ContributionObjectType::StarMapNode, &affected);
                } else if delta.after.is_none() {
                    let (_, affected) = self.nodes.remove(&delta.object_id);
                    self.refresh_object_leaves(ContributionObjectType::StarMapNode, &affected);
                    let key = object_key(ContributionObjectType::StarMapNode, &delta.object_id);
                    self.tree.set_leaf(key, None);
                }
            }
            ContributionObjectType::StarMapEdge => {
                if let Some(StarMapProofObject::Edge { edge }) = &delta.after {
                    let affected = self
                        .edges
                        .upsert(edge.id.clone(), edge.clone(), &mut self.next_order)
                        .1;
                    self.refresh_object_leaves(ContributionObjectType::StarMapEdge, &affected);
                } else if delta.after.is_none() {
                    let (_, affected) = self.edges.remove(&delta.object_id);
                    self.refresh_object_leaves(ContributionObjectType::StarMapEdge, &affected);
                    let key = object_key(ContributionObjectType::StarMapEdge, &delta.object_id);
                    self.tree.set_leaf(key, None);
                }
            }
            ContributionObjectType::StarMapEmbed => {
                if let Some(StarMapProofObject::Embed { embed }) = &delta.after {
                    let affected = self
                        .embeds
                        .upsert(
                            embed.instance_id.clone(),
                            embed.clone(),
                            &mut self.next_order,
                        )
                        .1;
                    self.refresh_object_leaves(ContributionObjectType::StarMapEmbed, &affected);
                } else if delta.after.is_none() {
                    let (_, affected) = self.embeds.remove(&delta.object_id);
                    self.refresh_object_leaves(ContributionObjectType::StarMapEmbed, &affected);
                    let key = object_key(ContributionObjectType::StarMapEmbed, &delta.object_id);
                    self.tree.set_leaf(key, None);
                }
            }
            ContributionObjectType::StarMapLink => {
                if let Some(StarMapProofObject::Link { link }) = &delta.after {
                    let affected = self
                        .links
                        .upsert(link.link_id.clone(), link.clone(), &mut self.next_order)
                        .1;
                    self.refresh_object_leaves(ContributionObjectType::StarMapLink, &affected);
                } else if delta.after.is_none() {
                    let (_, affected) = self.links.remove(&delta.object_id);
                    self.refresh_object_leaves(ContributionObjectType::StarMapLink, &affected);
                    let key = object_key(ContributionObjectType::StarMapLink, &delta.object_id);
                    self.tree.set_leaf(key, None);
                }
            }
            _ => {
                return Err(Error::Other(
                    "unsupported starmap proof object type".to_string(),
                ));
            }
        }
        Ok(())
    }

    pub fn to_state(&self) -> StarMapProofState {
        StarMapProofState {
            meta: self.meta.clone(),
            graph: StarMapGraph {
                schema_version: self.schema_version,
                starmap_id: self.starmap_id.clone(),
                nodes: self.nodes.sorted_values(),
                edges: self.edges.sorted_values(),
                embeds: self.embeds.sorted_values(),
                links: self.links.sorted_values(),
            },
            root_starmap_ids: self.root_starmap_ids.clone(),
        }
    }

    fn refresh_fixed_leaves(&mut self) {
        let schema_key = key_for_bytes(b"$schema");
        let mut schema_bytes = Vec::new();
        schema_bytes.extend_from_slice(b"schema");
        schema_bytes.extend_from_slice(&self.schema_version.to_le_bytes());
        self.tree
            .set_leaf(schema_key, Some(hash_bytes(&schema_bytes)));

        let starmap_key = key_for_bytes(b"$starmap");
        self.tree.set_leaf(
            starmap_key,
            Some(hash_serializable(b"starmap", &self.starmap_id)),
        );

        let meta_key = key_for_bytes(b"$meta");
        let meta_value = match &self.meta {
            Some(meta) => hash_serializable(b"meta", meta),
            None => hash_bytes(b"meta-missing"),
        };
        self.tree.set_leaf(meta_key, Some(meta_value));

        let roots_key = key_for_bytes(b"$roots");
        self.tree.set_leaf(
            roots_key,
            Some(hash_serializable(b"roots", &self.root_starmap_ids)),
        );
    }

    fn refresh_all_object_leaves(&mut self) {
        for kind in [
            ContributionObjectType::StarMapNode,
            ContributionObjectType::StarMapEdge,
            ContributionObjectType::StarMapEmbed,
            ContributionObjectType::StarMapLink,
        ] {
            let ids: Vec<String> = match kind {
                ContributionObjectType::StarMapNode => self.nodes.items.keys().cloned().collect(),
                ContributionObjectType::StarMapEdge => self.edges.items.keys().cloned().collect(),
                ContributionObjectType::StarMapEmbed => self.embeds.items.keys().cloned().collect(),
                ContributionObjectType::StarMapLink => self.links.items.keys().cloned().collect(),
                _ => Vec::new(),
            };
            self.refresh_object_leaves(kind, &ids);
        }
    }

    fn refresh_object_leaves(&mut self, object_type: ContributionObjectType, ids: &[String]) {
        for id in ids {
            self.refresh_object_leaf(object_type, id);
        }
    }

    fn refresh_object_leaf(&mut self, object_type: ContributionObjectType, id: &str) {
        let key = object_key(object_type, id);
        let value = match object_type {
            ContributionObjectType::StarMapNode => self.nodes.items.get(id).map(|(order, node)| {
                object_leaf_value(
                    KIND_NODE,
                    id,
                    &StarMapProofObject::Node { node: node.clone() },
                    &self.nodes.prev_id(*order),
                    &self.nodes.next_id(*order),
                )
            }),
            ContributionObjectType::StarMapEdge => self.edges.items.get(id).map(|(order, edge)| {
                object_leaf_value(
                    KIND_EDGE,
                    id,
                    &StarMapProofObject::Edge { edge: edge.clone() },
                    &self.edges.prev_id(*order),
                    &self.edges.next_id(*order),
                )
            }),
            ContributionObjectType::StarMapEmbed => {
                self.embeds.items.get(id).map(|(order, embed)| {
                    object_leaf_value(
                        KIND_EMBED,
                        id,
                        &StarMapProofObject::Embed {
                            embed: embed.clone(),
                        },
                        &self.embeds.prev_id(*order),
                        &self.embeds.next_id(*order),
                    )
                })
            }
            ContributionObjectType::StarMapLink => self.links.items.get(id).map(|(order, link)| {
                object_leaf_value(
                    KIND_LINK,
                    id,
                    &StarMapProofObject::Link { link: link.clone() },
                    &self.links.prev_id(*order),
                    &self.links.next_id(*order),
                )
            }),
            _ => None,
        };
        if !matches!(
            object_type,
            ContributionObjectType::StarMapNode
                | ContributionObjectType::StarMapEdge
                | ContributionObjectType::StarMapEmbed
                | ContributionObjectType::StarMapLink
        ) {
            return;
        }
        self.tree.set_leaf(key, value);
    }
}

/// 供检查点/事件哈希复用的对象级摘要。
pub fn commitment_object_sha256(object: Option<&StarMapProofObject>) -> String {
    proof_object_sha256(object)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_state() -> StarMapProofState {
        let mut state = StarMapProofState::empty("sm1");
        state.graph.nodes.push(StarMapNode {
            id: "n1".to_string(),
            ..StarMapNode::default()
        });
        state.graph.nodes.push(StarMapNode {
            id: "n2".to_string(),
            ..StarMapNode::default()
        });
        state
    }

    #[test]
    fn full_and_incremental_roots_match() {
        let state = sample_state();
        let mut commitment = StateCommitment::from_state(&state);
        let root = commitment.root_hex();
        assert_eq!(root, StateCommitment::from_state(&state).root_hex());

        let delta = StarMapProofDelta::for_object(
            "sm1",
            ContributionObjectType::StarMapNode,
            "n3",
            ContributionOperation::Create,
            None,
            None,
        );
        let mut delta = delta;
        delta.after = Some(StarMapProofObject::Node {
            node: StarMapNode {
                id: "n3".to_string(),
                ..StarMapNode::default()
            },
        });
        commitment.apply_delta(&delta).unwrap();
        let rebuilt = StateCommitment::from_state(&commitment.to_state());
        assert_eq!(commitment.root_hex(), rebuilt.root_hex());
    }
}
