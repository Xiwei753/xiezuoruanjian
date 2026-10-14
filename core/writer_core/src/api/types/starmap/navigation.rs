//! # 星图导航 DTO — 导航路由与导航位置的跨语言类型
//!
//! `StarMapNavigationRouteDto` 和 `StarMapNavigationLocationDto` 分别对应
//! Core 层的 `StarMapNavigationRoute` 和 `StarMapNavigationLocation`，
//! 为跨语言接口提供稳定的序列化/反序列化类型。

use super::StarMapPathSegmentDto;

/// 导航路由 DTO：从根星图到当前画布的语义路径。
///
/// - `root_starmap_id`：导航起点（根星图 ID）
/// - `segments`：路径段列表，每段是一次层级穿越
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapNavigationRouteDto {
    pub root_starmap_id: String,
    #[serde(default)]
    pub segments: Vec<StarMapPathSegmentDto>,
}

impl From<crate::starmap::navigation::StarMapNavigationRoute> for StarMapNavigationRouteDto {
    fn from(r: crate::starmap::navigation::StarMapNavigationRoute) -> Self {
        Self {
            root_starmap_id: r.root_starmap_id,
            segments: r.segments.into_iter().map(Into::into).collect(),
        }
    }
}

impl TryFrom<StarMapNavigationRouteDto> for crate::starmap::navigation::StarMapNavigationRoute {
    type Error = crate::error::Error;

    fn try_from(d: StarMapNavigationRouteDto) -> Result<Self, Self::Error> {
        Ok(Self {
            root_starmap_id: d.root_starmap_id,
            segments: d
                .segments
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<_>, _>>()?,
        })
    }
}

/// 导航位置 DTO：解析路由后得到的当前画布信息。
///
/// - `current_starmap_id`：当前所在画布 ID
/// - `traversed_starmap_ids`：经过的所有画布 ID（含起点和终点，顺序为穿越顺序）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapNavigationLocationDto {
    pub current_starmap_id: String,
    #[serde(default)]
    pub traversed_starmap_ids: Vec<String>,
}

impl From<crate::starmap::navigation::StarMapNavigationLocation> for StarMapNavigationLocationDto {
    fn from(l: crate::starmap::navigation::StarMapNavigationLocation) -> Self {
        Self {
            current_starmap_id: l.current_starmap_id,
            traversed_starmap_ids: l.traversed_starmap_ids,
        }
    }
}

impl TryFrom<StarMapNavigationLocationDto>
    for crate::starmap::navigation::StarMapNavigationLocation
{
    type Error = crate::error::Error;

    fn try_from(d: StarMapNavigationLocationDto) -> Result<Self, Self::Error> {
        Ok(Self {
            current_starmap_id: d.current_starmap_id,
            traversed_starmap_ids: d.traversed_starmap_ids,
        })
    }
}

/// 进入子画布的结果 DTO：更新后的路由和当前位置。
///
/// `enter_starmap_embed` / `enter_starmap_portal` 返回此类型，
/// 供 UniFFI 暴露（UniFFI 不支持 tuple 返回值，故包装为单个 DTO）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EnterStarmapResultDto {
    pub route: StarMapNavigationRouteDto,
    pub location: StarMapNavigationLocationDto,
}
