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
    pub from: Option<StarMapTargetPathDto>,
    pub to: Option<StarMapTargetPathDto>,
}

impl From<StarMapEdgePatchDto> for crate::starmap::types::StarMapEdgePatch {
    fn from(d: StarMapEdgePatchDto) -> Self {
        Self {
            kind: d.kind.map(Into::into),
            label: d.label,
            payload: d.payload.map(|opt| {
                opt.map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null))
            }),
            from: d.from.map(Into::into),
            to: d.to.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEdgePatchInputDto {
    pub kind: Option<StarMapEdgeKindDto>,
    pub label: Option<String>,
    pub clear_label: bool,
    pub payload: Option<String>,
    pub clear_payload: bool,
    pub from: Option<StarMapTargetPathDto>,
    pub to: Option<StarMapTargetPathDto>,
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
            payload: if d.clear_payload {
                Some(None)
            } else {
                d.payload.map(Some)
            },
            from: d.from,
            to: d.to,
        }
    }
}
