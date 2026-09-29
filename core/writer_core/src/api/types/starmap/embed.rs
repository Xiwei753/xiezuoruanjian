use super::*;

/// 创建子星图并嵌入父图的原子组合操作结果。
///
/// `starmap` 是新创建的子星图元数据，`embed` 是建立的 Embed 关系。
/// `pending` 表示 Core 是否还有后台持久事务（Git history）待补：
/// - `false`：数据已 durable 且 history 已记录，操作完全完成。
/// - `true`：数据已 durable（child meta、Embed、graph 均已落盘），
///   但 Git history 记录失败，下次启动会通过 journal recovery 补记。
///   调用方不应将 `pending: true` 视为创建失败——子星图和 Embed
///   已实际创建成功，应正常加入当前层显示。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CreateStarMapChildEmbedResultDto {
    pub starmap: StarMapMetaDto,
    pub embed: StarMapEmbedDto,
    #[serde(default)]
    pub pending: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbedDto {
    pub instance_id: String,
    pub target_starmap_id: String,
    pub label: Option<String>,
    pub position: StarMapPointDto,
    pub host_path: StarMapTargetPathDto,
    pub provenance: StarMapProvenanceDto,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::starmap::types::StarMapEmbed> for StarMapEmbedDto {
    fn from(e: crate::starmap::types::StarMapEmbed) -> Self {
        Self {
            instance_id: e.instance_id,
            target_starmap_id: e.target_starmap_id,
            label: e.label,
            position: e.position.into(),
            host_path: e.host_path.into(),
            provenance: e.provenance.into(),
            created_at: e.created_at,
            updated_at: e.updated_at,
        }
    }
}

impl TryFrom<StarMapEmbedDto> for crate::starmap::types::StarMapEmbed {
    type Error = crate::error::Error;

    fn try_from(d: StarMapEmbedDto) -> Result<Self, Self::Error> {
        Ok(Self {
            instance_id: d.instance_id,
            target_starmap_id: d.target_starmap_id,
            label: d.label,
            position: d.position.into(),
            host_path: d.host_path.try_into()?,
            provenance: d.provenance.into(),
            created_at: d.created_at,
            updated_at: d.updated_at,
        })
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbedPatchDto {
    pub label: Option<Option<String>>,
    pub position: Option<StarMapPointDto>,
    pub host_path: Option<StarMapTargetPathDto>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbedPatchInputDto {
    pub label: Option<String>,
    /// JSON C-ABI（Harmony）只发要改的字段，省略 `clearLabel` 即"不清空"。
    #[serde(default)]
    pub clear_label: bool,
    pub position: Option<StarMapPointDto>,
    pub host_path: Option<StarMapTargetPathDto>,
}

impl From<StarMapEmbedPatchInputDto> for StarMapEmbedPatchDto {
    fn from(d: StarMapEmbedPatchInputDto) -> Self {
        Self {
            label: if d.clear_label {
                Some(None)
            } else {
                d.label.map(Some)
            },
            position: d.position,
            host_path: d.host_path,
        }
    }
}

impl TryFrom<StarMapEmbedPatchDto> for crate::starmap::types::StarMapEmbedPatch {
    type Error = crate::error::Error;

    fn try_from(d: StarMapEmbedPatchDto) -> Result<Self, Self::Error> {
        Ok(Self {
            label: d.label,
            position: d.position.map(Into::into),
            host_path: d.host_path.map(|p| p.try_into()).transpose()?,
        })
    }
}
