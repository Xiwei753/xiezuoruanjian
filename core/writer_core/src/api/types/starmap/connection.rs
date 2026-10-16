//! # 星图跨画布连接 DTO — 端点选择、连线信息与错误类型的跨语言类型
//!
//! - `StarMapEndpointSelectionDto`：跨画布边的端点选择（导航路由 + 本地目标）
//! - `StarMapConnectionInfoDto`：连接信息（边的宿主图、ID、端点路径、类型和标签）
//! - `CrossCanvasEdgeErrorDto`：跨画布边规划错误（使用 `error_type` 字符串判别）

use super::{
    StarMapEdgeDto, StarMapEdgeKindDto, StarMapNavigationRouteDto, StarMapTargetDetailDto,
    StarMapTargetPathDto,
};

/// 端点选择 DTO：导航路由 + 本地目标。
///
/// `route` 描述从根星图到当前画布的语义路径，`target` 是当前画布内的具体目标
///（Node / Anchor / Starmap 等）。两者组合唯一确定一个跨画布边的端点。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEndpointSelectionDto {
    pub route: StarMapNavigationRouteDto,
    pub target: StarMapTargetDetailDto,
}

impl From<crate::starmap::connections::StarMapEndpointSelection> for StarMapEndpointSelectionDto {
    fn from(s: crate::starmap::connections::StarMapEndpointSelection) -> Self {
        Self {
            route: s.route.into(),
            target: s.target.into(),
        }
    }
}

impl TryFrom<StarMapEndpointSelectionDto>
    for crate::starmap::connections::StarMapEndpointSelection
{
    type Error = crate::error::Error;

    fn try_from(d: StarMapEndpointSelectionDto) -> Result<Self, Self::Error> {
        Ok(Self {
            route: d.route.try_into()?,
            target: d.target.try_into()?,
        })
    }
}

/// 连线信息 DTO：边的宿主图、ID、端点路径、类型和标签。
///
/// 由 `list_starmap_connections` 返回（内部走反向依赖索引查询），
/// 既包括本图 Edge，也包括存于祖先图但一端指向当前图的 Edge。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapConnectionInfoDto {
    pub owner_starmap_id: String,
    pub edge_id: String,
    pub from: StarMapTargetPathDto,
    pub to: StarMapTargetPathDto,
    pub kind: StarMapEdgeKindDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl From<crate::starmap::connections::StarMapConnectionInfo> for StarMapConnectionInfoDto {
    fn from(c: crate::starmap::connections::StarMapConnectionInfo) -> Self {
        Self {
            owner_starmap_id: c.owner_starmap_id,
            edge_id: c.edge_id,
            from: c.from.into(),
            to: c.to.into(),
            kind: c.kind.into(),
            label: c.label,
        }
    }
}

impl TryFrom<StarMapConnectionInfoDto> for crate::starmap::connections::StarMapConnectionInfo {
    type Error = crate::error::Error;

    fn try_from(d: StarMapConnectionInfoDto) -> Result<Self, Self::Error> {
        Ok(Self {
            owner_starmap_id: d.owner_starmap_id,
            edge_id: d.edge_id,
            from: d.from.try_into()?,
            to: d.to.try_into()?,
            kind: d.kind.into(),
            label: d.label,
        })
    }
}

/// 跨画布连线错误 DTO — 使用 `error_type` 字符串判别 + `resolve_status` Optional 字段。
///
/// - `error_type`：`"NoCommonHost"` / `"FromResolveFailed"` / `"ToResolveFailed"`
/// - `resolve_status`：解析失败时的 `StarMapTargetResolveStatus` 字符串表示（仅
///   `FromResolveFailed` / `ToResolveFailed` 有值），使用 serde 序列化值
///   （如 `"missingStarmap"`、`"cycleDetected"` 等）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CrossCanvasEdgeErrorDto {
    pub error_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolve_status: Option<String>,
}

impl From<crate::starmap::connections::CrossCanvasEdgeError> for CrossCanvasEdgeErrorDto {
    fn from(e: crate::starmap::connections::CrossCanvasEdgeError) -> Self {
        match e {
            crate::starmap::connections::CrossCanvasEdgeError::NoCommonHost => Self {
                error_type: "NoCommonHost".to_string(),
                resolve_status: None,
            },
            crate::starmap::connections::CrossCanvasEdgeError::FromResolveFailed(status) => Self {
                error_type: "FromResolveFailed".to_string(),
                resolve_status: resolve_status_to_string(&status),
            },
            crate::starmap::connections::CrossCanvasEdgeError::ToResolveFailed(status) => Self {
                error_type: "ToResolveFailed".to_string(),
                resolve_status: resolve_status_to_string(&status),
            },
        }
    }
}

impl TryFrom<CrossCanvasEdgeErrorDto> for crate::starmap::connections::CrossCanvasEdgeError {
    type Error = crate::error::Error;

    fn try_from(d: CrossCanvasEdgeErrorDto) -> Result<Self, Self::Error> {
        match d.error_type.as_str() {
            "NoCommonHost" => Ok(Self::NoCommonHost),
            "FromResolveFailed" => {
                let status = resolve_status_from_string(d.resolve_status)?;
                Ok(Self::FromResolveFailed(status))
            }
            "ToResolveFailed" => {
                let status = resolve_status_from_string(d.resolve_status)?;
                Ok(Self::ToResolveFailed(status))
            }
            unknown => Err(crate::error::Error::Other(format!(
                "unknown cross canvas edge error type: {}",
                unknown
            ))),
        }
    }
}

/// 将 `StarMapTargetResolveStatus` 序列化为不带 JSON 引号的字符串值。
///
/// 例如 `MissingStarmap` → `"missingStarmap"`（serde rename_all = camelCase）。
fn resolve_status_to_string(
    status: &crate::starmap::semantic::StarMapTargetResolveStatus,
) -> Option<String> {
    serde_json::to_value(status)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
}

/// 将字符串值反序列化为 `StarMapTargetResolveStatus`。
///
/// 输入应为 serde 序列化的字符串值，如 `"missingStarmap"`。
fn resolve_status_from_string(
    s: Option<String>,
) -> Result<crate::starmap::semantic::StarMapTargetResolveStatus, crate::error::Error> {
    let s = s.filter(|s| !s.is_empty()).ok_or_else(|| {
        crate::error::Error::Other(
            "missing or empty required field resolve_status for FromResolveFailed/ToResolveFailed"
                .into(),
        )
    })?;
    serde_json::from_value::<crate::starmap::semantic::StarMapTargetResolveStatus>(
        serde_json::Value::String(s),
    )
    .map_err(crate::error::Error::from)
}

/// 跨画布连线创建结果 DTO：宿主星图 ID 和真正写入 Store 的 Edge。
///
/// 由 AppService 的 `create_cross_canvas_edge` 返回，`ownerStarmapId` 是 Edge
/// 实际持久化所在的宿主星图，`edge` 是完整写入的 `StarMapEdgeDto`。
/// 平台端可直接用返回的 `edge.id` 对原始边做后续修改/删除。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CreateCrossCanvasEdgeResultDto {
    pub owner_starmap_id: String,
    pub edge: StarMapEdgeDto,
}
