//! 版本化清单根（`manifest/root.json`）。
//!
//! 根是清单存储的权威入口：`namespaces` 把命名空间映射到对应索引文件的
//! sha256；`generation` 由 `(schemaVersion, namespaces, localPreviousRoot)`
//! 的内容派生（`manifest-<sha256>`），同一内容得到稳定身份，不再接受调用方
//! 随机串作为版本身份。
//!
//! 两个"上一代"字段语义不同，禁止混用：
//!
//! - `previousRoot`：**远端发布视图专用**。只有远端发布决议（本轮读取的固定
//!   远端 base 根）能填写，值是那个已发布根 canonical bytes 的 sha256，只服务
//!   发布端"上一代仍可达"的 GC 保留边界；本地 [`super::ManifestStore`] 写盘时
//!   恒为 `None`，本地 staging/merge 不得自称已发布代。
//! - `localPreviousRoot`：**本地存储视图专用**。值是写盘前当前本地根 canonical
//!   bytes 的 sha256，只表示本地前一个持久根，不是任何"已发布"事实；发布视图
//!   把它置 `None`，发布根不携带本地血缘。
//!
//! 旧根没有这两个字段，反序列化为 `None`；序列化省略 `None` 字段，保证旧根
//! 重新序列化后逐字节不变。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 当前清单存储格式版本。
pub(crate) const MANIFEST_SCHEMA_VERSION: u32 = 2;

/// 命名空间名最大长度（与旧分片键保持一致）。
pub(crate) const MAX_NAMESPACE_LEN: usize = 64;

/// 权威清单根。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ManifestRoot {
    /// 存储格式版本，必须等于 [`MANIFEST_SCHEMA_VERSION`]。
    pub schema_version: u32,
    /// 内容派生的版本串：`manifest-<sha256(schema+namespaces+localPreviousRoot)>`。
    pub generation: String,
    /// namespace → index 文件 sha256。
    #[serde(default)]
    pub namespaces: BTreeMap<String, String>,
    /// 本轮远端发布读取到的固定 base 根 canonical bytes 的 sha256。
    ///
    /// 只允许由远端发布决议填写；本地存储写盘恒为 `None`。序列化时省略，
    /// 保证旧根重新序列化后逐字节不变。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_root: Option<String>,
    /// 写盘前当前本地根 canonical bytes 的 sha256。
    ///
    /// 只表示本地前一个持久根，不是已发布事实；发布视图恒为 `None`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_previous_root: Option<String>,
}

impl ManifestRoot {
    /// 本地存储根：`previousRoot` 恒为 `None`（本地不是已发布代），
    /// `localPreviousRoot` 记录写盘前本地根身份。
    pub(crate) fn local(
        namespaces: BTreeMap<String, String>,
        local_previous_root: Option<String>,
    ) -> Self {
        Self {
            schema_version: MANIFEST_SCHEMA_VERSION,
            generation: Self::content_generation(&namespaces, local_previous_root.as_deref()),
            namespaces,
            previous_root: None,
            local_previous_root,
        }
    }

    /// 远端发布根：`previousRoot` 只记录本轮读取的固定远端 base 根身份，
    /// 不携带任何本地血缘（`localPreviousRoot` 恒为 `None`）。
    pub(crate) fn published(
        namespaces: BTreeMap<String, String>,
        previous_root: Option<&str>,
    ) -> Self {
        Self {
            schema_version: MANIFEST_SCHEMA_VERSION,
            generation: Self::content_generation(&namespaces, None),
            namespaces,
            previous_root: previous_root.map(str::to_string),
            local_previous_root: None,
        }
    }

    /// 清单内容身份：对 `(schemaVersion, namespaces, localPreviousRoot)` 的
    /// 规范化编码做 sha256，返回 `manifest-<hex>`；同内容稳定。
    ///
    /// 编码固定为按 namespace 字典序逐行 `namespace <ns> <sha>`，并以
    /// `localPreviousRoot=<sha 或 ->` 收尾。namespace 只允许字母数字/`._-`，
    /// sha 固定 64 位小写 hex，因此该编码无歧义。
    pub(crate) fn content_generation(
        namespaces: &BTreeMap<String, String>,
        local_previous_root: Option<&str>,
    ) -> String {
        let mut canonical = String::new();
        canonical.push_str("manifest-generation-v1\n");
        canonical.push_str(&format!("schemaVersion={MANIFEST_SCHEMA_VERSION}\n"));
        for (namespace, sha) in namespaces {
            canonical.push_str(&format!("namespace {namespace} {sha}\n"));
        }
        canonical.push_str(&format!(
            "localPreviousRoot={}\n",
            local_previous_root.unwrap_or("-")
        ));
        format!(
            "manifest-{}",
            super::objects::sha256_hex(canonical.as_bytes())
        )
    }

    /// canonical JSON：字段顺序固定、namespace 按字典序、compact。
    pub(crate) fn canonical_bytes(&self) -> crate::Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(crate::Error::Json)
    }

    /// canonical bytes 的 sha256（`previousRoot`/`localPreviousRoot` 记录的就是这个值）。
    pub(crate) fn canonical_sha256(&self) -> crate::Result<String> {
        Ok(super::objects::sha256_hex(&self.canonical_bytes()?))
    }

    /// 校验根的结构约束；错误消息带来源路径。
    pub(crate) fn validate(&self, origin: &str) -> crate::Result<()> {
        if self.schema_version != MANIFEST_SCHEMA_VERSION {
            return Err(crate::Error::Other(format!(
                "manifest root {} has unsupported schemaVersion {} (expected {})",
                origin, self.schema_version, MANIFEST_SCHEMA_VERSION
            )));
        }
        for (field, value) in [
            ("previousRoot", &self.previous_root),
            ("localPreviousRoot", &self.local_previous_root),
        ] {
            if let Some(value) = value {
                if !super::objects::is_sha256_hex(value) {
                    return Err(crate::Error::Other(format!(
                        "manifest root {} has invalid {field} sha256: {value}",
                        origin
                    )));
                }
            }
        }
        for (namespace, sha) in &self.namespaces {
            super::index::validate_namespace(namespace, origin)?;
            if !super::objects::is_sha256_hex(sha) {
                return Err(crate::Error::Other(format!(
                    "manifest root {} has invalid index sha256 for namespace {namespace}: {sha}",
                    origin
                )));
            }
        }
        Ok(())
    }
}
