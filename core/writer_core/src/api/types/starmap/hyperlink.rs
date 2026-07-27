use super::*;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapHyperlinkDto {
    pub hyperlink_id: String,
    pub source: StarMapEndpointPathDto,
    pub target_uri: String,
    pub label: Option<String>,
    pub target_starmap_id: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::starmap::types::StarMapHyperlink> for StarMapHyperlinkDto {
    fn from(h: crate::starmap::types::StarMapHyperlink) -> Self {
        Self {
            hyperlink_id: h.hyperlink_id,
            source: h.source.into(),
            target_uri: h.target_uri,
            label: h.label,
            target_starmap_id: h.target_starmap_id,
            created_at: h.created_at,
            updated_at: h.updated_at,
        }
    }
}

impl From<StarMapHyperlinkDto> for crate::starmap::types::StarMapHyperlink {
    fn from(d: StarMapHyperlinkDto) -> Self {
        Self {
            hyperlink_id: d.hyperlink_id,
            source: d.source.into(),
            target_uri: d.target_uri,
            label: d.label,
            target_starmap_id: d.target_starmap_id,
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapHyperlinkPatchDto {
    pub label: Option<Option<String>>,
    pub target_uri: Option<Option<String>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapHyperlinkPatchInputDto {
    pub label: Option<String>,
    pub clear_label: bool,
    pub target_uri: Option<String>,
    pub clear_target_uri: bool,
}

impl From<StarMapHyperlinkPatchInputDto> for StarMapHyperlinkPatchDto {
    fn from(d: StarMapHyperlinkPatchInputDto) -> Self {
        Self {
            label: if d.clear_label {
                Some(None)
            } else {
                d.label.map(Some)
            },
            target_uri: if d.clear_target_uri {
                Some(None)
            } else {
                d.target_uri.map(Some)
            },
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapHyperlinkListWithDiagnosticsDto {
    pub items: Vec<StarMapHyperlinkDto>,
    pub diagnostics: Vec<LoadDiagnosticDto>,
}

impl From<crate::starmap::store::ListWithDiagnostics<crate::starmap::types::StarMapHyperlink>> for StarMapHyperlinkListWithDiagnosticsDto {
    fn from(r: crate::starmap::store::ListWithDiagnostics<crate::starmap::types::StarMapHyperlink>) -> Self {
        Self {
            items: r.items.into_iter().map(StarMapHyperlinkDto::from).collect(),
            diagnostics: r.diagnostics.into_iter().map(LoadDiagnosticDto::from).collect(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PhasedSnapshotRequestDto {
    pub target_phase: String,
    pub since_revision: u64,
}

impl From<crate::starmap::store::PhasedSnapshotRequest> for PhasedSnapshotRequestDto {
    fn from(r: crate::starmap::store::PhasedSnapshotRequest) -> Self {
        Self {
            target_phase: match r.target_phase {
                crate::starmap::store::LoadPhase::GraphMeta => "GraphMeta".to_string(),
                crate::starmap::store::LoadPhase::ViewportAndLayoutIndex => "ViewportAndLayoutIndex".to_string(),
                crate::starmap::store::LoadPhase::CurrentViewportObjects => "CurrentViewportObjects".to_string(),
                crate::starmap::store::LoadPhase::PrefetchNearbyObjects => "PrefetchNearbyObjects".to_string(),
                crate::starmap::store::LoadPhase::BackgroundFullLoad => "BackgroundFullLoad".to_string(),
            },
            since_revision: r.since_revision,
        }
    }
}

impl From<PhasedSnapshotRequestDto> for crate::starmap::store::PhasedSnapshotRequest {
    fn from(d: PhasedSnapshotRequestDto) -> Self {
        Self {
            target_phase: match d.target_phase.as_str() {
                "GraphMeta" => crate::starmap::store::LoadPhase::GraphMeta,
                "ViewportAndLayoutIndex" => crate::starmap::store::LoadPhase::ViewportAndLayoutIndex,
                "CurrentViewportObjects" => crate::starmap::store::LoadPhase::CurrentViewportObjects,
                "BackgroundFullLoad" => crate::starmap::store::LoadPhase::BackgroundFullLoad,
                _ => crate::starmap::store::LoadPhase::PrefetchNearbyObjects,
            },
            since_revision: d.since_revision,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapPhasedSnapshotDto {
    pub starmap_id: String,
    pub title: String,
    pub load_phase: String,
    pub package_revision: u64,
    pub complete: bool,
    pub nodes: Vec<StarMapNodeDto>,
    pub edges: Vec<StarMapEdgeDto>,
    pub embeds: Vec<StarMapEmbedDto>,
    pub links: Vec<StarMapLinkDto>,
    pub hyperlinks: Vec<StarMapHyperlinkDto>,
    pub layout: Option<StarMapLayoutDto>,
    pub viewport: Option<StarMapViewportDto>,
    pub diagnostics: Vec<LoadDiagnosticDto>,
}

impl From<crate::starmap::store::StarMapPhasedSnapshot> for StarMapPhasedSnapshotDto {
    fn from(s: crate::starmap::store::StarMapPhasedSnapshot) -> Self {
        Self {
            starmap_id: s.starmap_id,
            title: s.title,
            load_phase: match s.load_phase {
                crate::starmap::store::LoadPhase::GraphMeta => "GraphMeta".to_string(),
                crate::starmap::store::LoadPhase::ViewportAndLayoutIndex => "ViewportAndLayoutIndex".to_string(),
                crate::starmap::store::LoadPhase::CurrentViewportObjects => "CurrentViewportObjects".to_string(),
                crate::starmap::store::LoadPhase::PrefetchNearbyObjects => "PrefetchNearbyObjects".to_string(),
                crate::starmap::store::LoadPhase::BackgroundFullLoad => "BackgroundFullLoad".to_string(),
            },
            package_revision: s.package_revision,
            complete: s.complete,
            nodes: s.nodes.into_iter().map(Into::into).collect(),
            edges: s.edges.into_iter().map(Into::into).collect(),
            embeds: s.embeds.into_iter().map(Into::into).collect(),
            links: s.links.into_iter().map(Into::into).collect(),
            hyperlinks: s.hyperlinks.into_iter().map(Into::into).collect(),
            layout: s.layout.map(Into::into),
            viewport: s.viewport.map(Into::into),
            diagnostics: s.diagnostics.into_iter().map(LoadDiagnosticDto::from).collect(),
        }
    }
}
