use super::*;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapLinkDto {
    pub link_id: String,
    pub source: StarMapTargetPathDto,
    pub target: StarMapTargetPathDto,
    pub label: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::starmap::types::StarMapLink> for StarMapLinkDto {
    fn from(l: crate::starmap::types::StarMapLink) -> Self {
        Self {
            link_id: l.link_id,
            source: l.source.into(),
            target: l.target.into(),
            label: l.label,
            created_at: l.created_at,
            updated_at: l.updated_at,
        }
    }
}

impl From<StarMapLinkDto> for crate::starmap::types::StarMapLink {
    fn from(d: StarMapLinkDto) -> Self {
        Self {
            link_id: d.link_id,
            source: d.source.into(),
            target: d.target.into(),
            label: d.label,
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapLinkPatchDto {
    pub source: Option<StarMapTargetPathDto>,
    pub target: Option<StarMapTargetPathDto>,
    pub label: Option<Option<String>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapLinkPatchInputDto {
    pub source: Option<StarMapTargetPathDto>,
    pub target: Option<StarMapTargetPathDto>,
    pub label: Option<String>,
    pub clear_label: bool,
}

impl From<StarMapLinkPatchInputDto> for StarMapLinkPatchDto {
    fn from(d: StarMapLinkPatchInputDto) -> Self {
        Self {
            source: d.source,
            target: d.target,
            label: if d.clear_label {
                Some(None)
            } else {
                d.label.map(Some)
            },
        }
    }
}

impl From<StarMapLinkPatchDto> for crate::starmap::types::StarMapLinkPatch {
    fn from(d: StarMapLinkPatchDto) -> Self {
        Self {
            source: d.source.map(Into::into),
            target: d.target.map(Into::into),
            label: d.label,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LoadDiagnosticDto {
    pub kind: String,
    pub object_type: String,
    pub object_id: String,
    pub detail: Option<String>,
}

impl From<crate::starmap::store::LoadDiagnostic> for LoadDiagnosticDto {
    fn from(d: crate::starmap::store::LoadDiagnostic) -> Self {
        Self {
            kind: format!("{:?}", d.kind),
            object_type: d.object_type,
            object_id: d.object_id,
            detail: if d.detail.is_empty() {
                None
            } else {
                Some(d.detail)
            },
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapLinkListWithDiagnosticsDto {
    pub items: Vec<StarMapLinkDto>,
    pub diagnostics: Vec<LoadDiagnosticDto>,
}

impl From<crate::starmap::store::ListWithDiagnostics<crate::starmap::types::StarMapLink>>
    for StarMapLinkListWithDiagnosticsDto
{
    fn from(
        r: crate::starmap::store::ListWithDiagnostics<crate::starmap::types::StarMapLink>,
    ) -> Self {
        Self {
            items: r.items.into_iter().map(StarMapLinkDto::from).collect(),
            diagnostics: r
                .diagnostics
                .into_iter()
                .map(LoadDiagnosticDto::from)
                .collect(),
        }
    }
}
