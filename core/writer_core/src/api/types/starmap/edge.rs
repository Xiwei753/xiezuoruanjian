use super::*;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]

pub enum StarMapEdgeKindDto {
    Contains,
    References,
    AppearsIn,
    Causes,
    RelatedTo,
    LocatedAt,
    CharacterRelation,
    Timeline,
    Foreshadows,
    Resolves,
    DependsOn,
    ConflictsWith,
    Custom,
}

impl From<crate::starmap::types::StarMapEdgeKind> for StarMapEdgeKindDto {
    fn from(k: crate::starmap::types::StarMapEdgeKind) -> Self {
        match k {
            crate::starmap::types::StarMapEdgeKind::Contains => Self::Contains,
            crate::starmap::types::StarMapEdgeKind::References => Self::References,
            crate::starmap::types::StarMapEdgeKind::AppearsIn => Self::AppearsIn,
            crate::starmap::types::StarMapEdgeKind::Causes => Self::Causes,
            crate::starmap::types::StarMapEdgeKind::RelatedTo => Self::RelatedTo,
            crate::starmap::types::StarMapEdgeKind::LocatedAt => Self::LocatedAt,
            crate::starmap::types::StarMapEdgeKind::CharacterRelation => Self::CharacterRelation,
            crate::starmap::types::StarMapEdgeKind::Timeline => Self::Timeline,
            crate::starmap::types::StarMapEdgeKind::Foreshadows => Self::Foreshadows,
            crate::starmap::types::StarMapEdgeKind::Resolves => Self::Resolves,
            crate::starmap::types::StarMapEdgeKind::DependsOn => Self::DependsOn,
            crate::starmap::types::StarMapEdgeKind::ConflictsWith => Self::ConflictsWith,
            crate::starmap::types::StarMapEdgeKind::Custom => Self::Custom,
        }
    }
}

impl From<StarMapEdgeKindDto> for crate::starmap::types::StarMapEdgeKind {
    fn from(dto: StarMapEdgeKindDto) -> Self {
        match dto {
            StarMapEdgeKindDto::Contains => Self::Contains,
            StarMapEdgeKindDto::References => Self::References,
            StarMapEdgeKindDto::AppearsIn => Self::AppearsIn,
            StarMapEdgeKindDto::Causes => Self::Causes,
            StarMapEdgeKindDto::RelatedTo => Self::RelatedTo,
            StarMapEdgeKindDto::LocatedAt => Self::LocatedAt,
            StarMapEdgeKindDto::CharacterRelation => Self::CharacterRelation,
            StarMapEdgeKindDto::Timeline => Self::Timeline,
            StarMapEdgeKindDto::Foreshadows => Self::Foreshadows,
            StarMapEdgeKindDto::Resolves => Self::Resolves,
            StarMapEdgeKindDto::DependsOn => Self::DependsOn,
            StarMapEdgeKindDto::ConflictsWith => Self::ConflictsWith,
            StarMapEdgeKindDto::Custom => Self::Custom,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEdgePatchDto {
    pub kind: Option<StarMapEdgeKindDto>,
    pub label: Option<Option<String>>,
    pub payload: Option<Option<String>>,
    pub from_target: Option<Option<StarMapDeepTargetDto>>,
    pub to_target: Option<Option<StarMapDeepTargetDto>>,
    pub from_endpoint: Option<Option<StarMapEdgeEndpointDto>>,
    pub to_endpoint: Option<Option<StarMapEdgeEndpointDto>>,
    pub from_endpoint_path: Option<Option<StarMapEndpointPathDto>>,
    pub to_endpoint_path: Option<Option<StarMapEndpointPathDto>>,
}

impl From<StarMapEdgePatchDto> for crate::starmap::types::StarMapEdgePatch {
    fn from(d: StarMapEdgePatchDto) -> Self {
        Self {
            kind: d.kind.map(Into::into),
            label: d.label,
            payload: d.payload.map(|opt| {
                opt.map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null))
            }),
            from_target: d.from_target.map(|v| v.map(Into::into)),
            to_target: d.to_target.map(|v| v.map(Into::into)),
            from_endpoint: d.from_endpoint.map(|v| v.map(Into::into)),
            to_endpoint: d.to_endpoint.map(|v| v.map(Into::into)),
            from_endpoint_path: d.from_endpoint_path.map(|v| v.map(Into::into)),
            to_endpoint_path: d.to_endpoint_path.map(|v| v.map(Into::into)),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEdgePatchInputDto {
    pub kind: Option<StarMapEdgeKindDto>,
    pub label: Option<String>,
    pub clear_label: bool,
}

impl From<StarMapEdgePatchInputDto> for StarMapEdgePatchDto {
    fn from(d: StarMapEdgePatchInputDto) -> Self {
        Self {
            kind: d.kind,
            label: if d.clear_label {
                Some(None)
            } else {
                d.label.map(Some)
            },
            payload: None,
            from_target: None,
            to_target: None,
            from_endpoint: None,
            to_endpoint: None,
            from_endpoint_path: None,
            to_endpoint_path: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_starmap_edge_kind_dto_roundtrip() {
        let kinds = vec![
            crate::starmap::types::StarMapEdgeKind::Contains,
            crate::starmap::types::StarMapEdgeKind::References,
            crate::starmap::types::StarMapEdgeKind::AppearsIn,
            crate::starmap::types::StarMapEdgeKind::Causes,
            crate::starmap::types::StarMapEdgeKind::RelatedTo,
            crate::starmap::types::StarMapEdgeKind::LocatedAt,
            crate::starmap::types::StarMapEdgeKind::CharacterRelation,
            crate::starmap::types::StarMapEdgeKind::Timeline,
            crate::starmap::types::StarMapEdgeKind::Foreshadows,
            crate::starmap::types::StarMapEdgeKind::Resolves,
            crate::starmap::types::StarMapEdgeKind::DependsOn,
            crate::starmap::types::StarMapEdgeKind::ConflictsWith,
            crate::starmap::types::StarMapEdgeKind::Custom,
        ];

        for kind in kinds {
            let dto: StarMapEdgeKindDto = kind.clone().into();
            let back: crate::starmap::types::StarMapEdgeKind = dto.into();
            assert_eq!(kind, back);
        }
    }
}
