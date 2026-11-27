use std::path::Path;

use serde::de::DeserializeOwned;

use crate::error::{Error, Result};
use crate::starmap::migration::nodes::{NodeObjectBindings, ReferenceBindingState};
use crate::starmap::package_storage;
use crate::starmap::types::*;

use super::super::types::*;
use super::super::StarMapStore;

/// 严格加载单个对象的通用 helper：文件存在 -> 读取 -> 解析 -> ID 校验。
///
/// 统一 node / edge / embed 三种包内对象的 fail-closed 错误处理，
/// 避免四处复制同一套「文件存在 → 读取 → 解析 → ID 校验」代码。
///
/// - 文件不存在 → push `Missing` diagnostic 后 `Err`；
/// - `read_to_string` 失败 → `Err`（IO 错误直接传播，不压成 None）；
/// - JSON 解析失败 → push `Corrupt` diagnostic 后 `Err`；
/// - `extract_id(obj)` 与文件名不一致 → push `Corrupt` diagnostic 后 `Err`。
///
/// `id_label` 仅用于 ID 不一致错误消息中描述内部字段名（node/edge 用
/// `"id"`，embed 用 `"instance_id"`），以保持与原逐字实现完全一致的消息文本。
///
/// diagnostic 只负责说明「哪里坏了」，不承担「错误已处理、可继续返回成功」的作用。
///
/// 导航链接不在包内对象文件里，不走此 helper，见 [`StarMapStore::try_load_link`]。
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
    /// 取出节点迁移解码用的素材库视图；第一次调用时按
    /// `starmaps/library/references.json` 懒加载。
    ///
    /// 解码期间借用它，调用方必须把同一个视图放回
    /// `self.node_object_bindings`，同一 store 生命周期内只读一次索引。
    /// 索引整体缺失/损坏时先按已确认实例批量重建一次：正常加载只需要补自己
    /// 缺的那一条，不能因为派生索引不可用就逐节点反复重写全量索引。
    fn take_node_object_bindings(&mut self) -> Result<NodeObjectBindings> {
        match self.node_object_bindings.take() {
            Some(bindings) => Ok(bindings),
            None => Ok(NodeObjectBindings::from_reference_index(
                crate::starmap::library::load_or_rebuild_reference_index(&self.app_data_root)?,
            )),
        }
    }

    /// 严格加载单个 node：文件缺失 / IO 失败 / JSON 解析失败 / 内部 ID 不一致
    /// 都返回 `Err`（仍 push diagnostic 到 recovery_log 便于调试）。
    /// 不再返回 `Option<T>` 吞掉错误——GraphMeta 声明的对象必须加载成功。
    ///
    /// Issue #1062：旧格式完整节点经统一迁移入口
    /// [`crate::starmap::migration::nodes::decode_node_object`] 解码；加载时先写
    /// 共享本体、再把节点文件重写成实例（原子写），然后才继续本体投影。
    /// 解码带上本 store 懒加载并缓存的 `NodeObjectBindings`：旧端只写
    /// `{instanceId, position}`、缺 `assetId` 的实例，或跨作品复用导致真实
    /// `assetId` 不等于 `instanceId` 时，仍能按 `references.json` 反向绑定
    /// 解析出稳定素材身份。
    /// 迁移失败 fail-closed：错误带星图 ID、对象 ID 和节点路径，不返回半成品。
    pub(in crate::starmap::store) fn try_load_node(
        &mut self,
        node_id: &str,
    ) -> Result<StarMapNode> {
        let dir = self.starmap_dir();
        let node_path = dir
            .join("nodes")
            .join(package_storage::bucket_for_id(node_id))
            .join(format!("{}.json", node_id));
        if !node_path.exists() {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Missing,
                object_type: "node instance".to_string(),
                object_id: node_id.to_string(),
                detail: format!("node instance file not found: {}", node_path.display()),
            });
            return Err(Error::Other(format!(
                "node instance '{}' file not found: {}",
                node_id,
                node_path.display()
            )));
        }
        let bytes = std::fs::read(&node_path)?;
        let mut bindings = match self.take_node_object_bindings() {
            Ok(bindings) => bindings,
            Err(error) => {
                self.recovery_log.push(LoadDiagnostic {
                    kind: LoadDiagnosticKind::Corrupt,
                    object_type: "node instance".to_string(),
                    object_id: node_id.to_string(),
                    detail: format!(
                        "node reference bindings load failed: starmap_id={}: {error}",
                        self.starmap_id
                    ),
                });
                return Err(Error::Other(format!(
                    "node '{}' migration failed: starmap_id={}: {error}",
                    node_id, self.starmap_id
                )));
            }
        };
        let decoded = crate::starmap::migration::nodes::decode_node_object(
            &self.app_data_root,
            &self.starmap_id,
            node_id,
            &bytes,
            Some(&bindings),
        );
        let plan = match decoded {
            Ok(plan) => plan,
            Err(error) => {
                self.recovery_log.push(LoadDiagnostic {
                    kind: LoadDiagnosticKind::Corrupt,
                    object_type: "node instance".to_string(),
                    object_id: node_id.to_string(),
                    detail: format!(
                        "node migration/decode failed: path={} starmap_id={}: {error}",
                        node_path.display(),
                        self.starmap_id
                    ),
                });
                return Err(Error::Other(format!(
                    "node '{}' migration failed: path={} starmap_id={}: {error}",
                    node_id,
                    node_path.display(),
                    self.starmap_id
                )));
            }
        };
        // Issue #1062 评论 #7213634487 第 C 项：解析出的规范实例与
        // `references.json` 的绑定必须完整一致。缺失/`projectId` 落后是派生
        // 索引待修复（只补引用，不改写已合法的节点文件）；同一实例绑定到别的
        // 资产身份是身份冲突，保留原始材料和既有绑定并明确报错。
        let reference_state = bindings.reference_binding_state(&plan.instance);
        if let ReferenceBindingState::Conflict { reference_asset_id } = &reference_state {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Corrupt,
                object_type: "node instance".to_string(),
                object_id: node_id.to_string(),
                detail: format!(
                    "node reference conflict: path={} starmap_id={} instance_asset_id={} \
                     reference_asset_id={reference_asset_id}",
                    node_path.display(),
                    self.starmap_id,
                    plan.instance.asset_id
                ),
            });
            return Err(Error::Other(format!(
                "starmap node identity conflict: path={} object_type=node object_id={} \
                 starmap_id={} instance_asset_id={} reference_asset_id={reference_asset_id} \
                 conflict_fields=assetId; preserve original node bytes and existing reference",
                node_path.display(),
                node_id,
                self.starmap_id,
                plan.instance.asset_id
            )));
        }
        if plan.wrote_definition || plan.rewrite_instance {
            crate::starmap::migration::nodes::persist_node_object_plan(
                &self.app_data_root,
                &self.starmap_id,
                node_id,
                &plan,
            )
            .map_err(|error| {
                Error::Other(format!(
                    "node '{}' migration write failed: path={} starmap_id={}: {error}",
                    node_id,
                    node_path.display(),
                    self.starmap_id
                ))
            })?;
        } else if reference_state != ReferenceBindingState::Consistent {
            // 崩溃重入：实例文件已是合法新格式，迁移 flag 全为 false，
            // 但派生反向索引缺这一条（或 projectId 落后）；只补索引。
            crate::starmap::library::record_node_reference(
                &self.app_data_root,
                &self.starmap_id,
                plan.instance.project_id.as_deref(),
                &plan.instance.instance_id,
                &plan.instance.asset_id,
            )
            .map_err(|error| {
                Error::Other(format!(
                    "node '{}' reference index repair failed: path={} starmap_id={}: {error}",
                    node_id,
                    node_path.display(),
                    self.starmap_id
                ))
            })?;
        }
        // 同一 store 后续加载的节点复用同一份反向绑定视图；这里同步本次加载
        // 已确认（或被补写）的绑定，避免后续节点基于旧状态判断。
        bindings.remember_reference(StarMapNodeReference {
            asset_id: plan.instance.asset_id.clone(),
            starmap_id: self.starmap_id.clone(),
            instance_id: plan.instance.instance_id.clone(),
            project_id: plan.instance.project_id.clone(),
        });
        self.node_object_bindings = Some(bindings);

        let instance = plan.instance;
        let asset_id = instance.asset_id.clone();
        self.node_assets
            .insert(node_id.to_string(), asset_id.clone());
        let definition = match plan.definition {
            Some(definition) => definition,
            None => crate::starmap::library::load_node_definition(&self.app_data_root, &asset_id)?
                .unwrap_or_else(|| {
                    // Issue #1062：实例可以先于/独立于共享本体存在（旧数据、
                    // 旧端只写实例的包）。缺本体不阻断加载，用只存在于内存的
                    // 占位本体完成投影；不写回素材库，不伪造共享内容。
                    crate::starmap::migration::nodes::placeholder_definition(&instance)
                }),
        };
        if definition.asset_id != asset_id {
            return Err(Error::Other(format!(
                "shared node definition id mismatch for {}: {} (path={} starmap_id={})",
                asset_id,
                definition.asset_id,
                node_path.display(),
                self.starmap_id
            )));
        }
        Ok(definition.instantiate_instance(&instance))
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

    /// 从 annotations 仓库按 `link_id` 读链接并转成当前画布投影。
    ///
    /// 与包内对象不同，链接的持久事实在注解仓库，`GraphMeta.link_ids` 只是
    /// 派生缓存，因此这里**不 fail-closed**：
    /// - 仓库没有该记录（尚未迁移 / 已被清理）→ `Ok(None)`；
    /// - 记录已 tombstone，或记录源属于其它画布 → `Ok(None)`；
    /// - 仓库读取/解析失败才返回 `Err`。
    pub(in crate::starmap::store) fn try_load_link(
        &mut self,
        link_id: &str,
    ) -> Result<Option<StarMapLink>> {
        let store = self.annotation_store_for_canvas()?;
        let Some(link) = store.get_link(link_id)? else {
            return Ok(None);
        };
        Ok(self.object_link_to_star_map_link(&link))
    }
}
