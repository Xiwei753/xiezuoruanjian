use std::path::Path;

use serde::de::DeserializeOwned;

use crate::error::{Error, Result};
use crate::starmap::package_storage;
use crate::starmap::types::*;

use super::super::types::*;
use super::super::StarMapStore;

/// 严格加载单个对象的通用 helper：文件存在 -> 读取 -> 解析 -> ID 校验。
///
/// 统一 node / edge / embed / link / hyperlink 五种对象的 fail-closed 错误处理，
/// 避免五处复制同一套「文件存在 → 读取 → 解析 → ID 校验」代码。
///
/// - 文件不存在 → push `Missing` diagnostic 后 `Err`；
/// - `read_to_string` 失败 → `Err`（IO 错误直接传播，不压成 None）；
/// - JSON 解析失败 → push `Corrupt` diagnostic 后 `Err`；
/// - `extract_id(obj)` 与文件名不一致 → push `Corrupt` diagnostic 后 `Err`。
///
/// `id_label` 仅用于 ID 不一致错误消息中描述内部字段名（node/edge/hyperlink/link
/// 用 `"id"`，embed 用 `"instance_id"`），以保持与原逐字实现完全一致的消息文本。
///
/// diagnostic 只负责说明「哪里坏了」，不承担「错误已处理、可继续返回成功」的作用。
fn load_object_strict<T, F>(
    recovery_log: &mut Vec<LoadDiagnostic>,
    starmap_dir: &Path,
    subdir: &str,
    object_type: &str,
    object_id: &str,
    id_label: &str,
    extract_id: F,
) -> Result<T>
where
    T: DeserializeOwned,
    F: FnOnce(&T) -> &str,
{
    let bucket_dir = starmap_dir
        .join(subdir)
        .join(package_storage::bucket_for_id(object_id));
    let bucket_path = bucket_dir.join(format!("{}.json", object_id));
    if !bucket_path.exists() {
        recovery_log.push(LoadDiagnostic {
            kind: LoadDiagnosticKind::Missing,
            object_type: object_type.to_string(),
            object_id: object_id.to_string(),
            detail: format!("{} file not found: {}", object_type, bucket_path.display()),
        });
        return Err(Error::Other(format!(
            "{} '{}' file not found: {}",
            object_type,
            object_id,
            bucket_path.display()
        )));
    }
    let content = std::fs::read_to_string(&bucket_path)?;
    let obj: T = serde_json::from_str(&content).map_err(|e| {
        recovery_log.push(LoadDiagnostic {
            kind: LoadDiagnosticKind::Corrupt,
            object_type: object_type.to_string(),
            object_id: object_id.to_string(),
            detail: format!("parse error: {}", e),
        });
        Error::Other(format!(
            "{} '{}' parse error: {}",
            object_type, object_id, e
        ))
    })?;
    let actual_id = extract_id(&obj);
    if actual_id != object_id {
        let actual = actual_id.to_string();
        recovery_log.push(LoadDiagnostic {
            kind: LoadDiagnosticKind::Corrupt,
            object_type: object_type.to_string(),
            object_id: object_id.to_string(),
            detail: format!(
                "{} internal {} '{}' does not match filename '{}'",
                object_type, id_label, actual, object_id
            ),
        });
        return Err(Error::Other(format!(
            "{} '{}' internal {} '{}' does not match filename",
            object_type, object_id, id_label, actual
        )));
    }
    Ok(obj)
}

impl StarMapStore {
    /// 严格加载单个 node：文件缺失 / IO 失败 / JSON 解析失败 / 内部 ID 不一致
    /// 都返回 `Err`（仍 push diagnostic 到 recovery_log 便于调试）。
    /// 不再返回 `Option<T>` 吞掉错误——GraphMeta 声明的对象必须加载成功。
    pub(in crate::starmap::store) fn try_load_node(
        &mut self,
        node_id: &str,
    ) -> Result<StarMapNode> {
        let dir = self.starmap_dir();
        load_object_strict(
            &mut self.recovery_log,
            &dir,
            "nodes",
            "node",
            node_id,
            "id",
            |n: &StarMapNode| n.id.as_str(),
        )
    }

    /// 严格加载单个 edge：同 `try_load_node` 的 fail-closed 语义。
    pub(in crate::starmap::store) fn try_load_edge(
        &mut self,
        edge_id: &str,
    ) -> Result<StarMapEdge> {
        let dir = self.starmap_dir();
        load_object_strict(
            &mut self.recovery_log,
            &dir,
            "edges",
            "edge",
            edge_id,
            "id",
            |e: &StarMapEdge| e.id.as_str(),
        )
    }

    /// 严格加载单个 embed：同 `try_load_node` 的 fail-closed 语义。
    pub(in crate::starmap::store) fn try_load_embed(
        &mut self,
        instance_id: &str,
    ) -> Result<StarMapEmbed> {
        let dir = self.starmap_dir();
        load_object_strict(
            &mut self.recovery_log,
            &dir,
            "embeds",
            "embed",
            instance_id,
            "instance_id",
            |e: &StarMapEmbed| e.instance_id.as_str(),
        )
    }

    /// 严格加载单个 hyperlink：同 `try_load_node` 的 fail-closed 语义。
    pub(in crate::starmap::store) fn try_load_hyperlink(
        &mut self,
        hyperlink_id: &str,
    ) -> Result<StarMapHyperlink> {
        let dir = self.starmap_dir();
        load_object_strict(
            &mut self.recovery_log,
            &dir,
            "hyperlinks",
            "hyperlink",
            hyperlink_id,
            "id",
            |h: &StarMapHyperlink| h.hyperlink_id.as_str(),
        )
    }

    /// 严格加载单个 link：同 `try_load_node` 的 fail-closed 语义。
    pub(in crate::starmap::store) fn try_load_link(
        &mut self,
        link_id: &str,
    ) -> Result<StarMapLink> {
        let dir = self.starmap_dir();
        load_object_strict(
            &mut self.recovery_log,
            &dir,
            "links",
            "link",
            link_id,
            "id",
            |l: &StarMapLink| l.link_id.as_str(),
        )
    }
}
