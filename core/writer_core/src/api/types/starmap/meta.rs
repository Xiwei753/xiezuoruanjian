#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapMetaDto {
    pub starmap_id: String,
    pub title: String,
    pub description: String,
    pub project_id: Option<String>,
    pub accent_color: String,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::starmap::StarMapMeta> for StarMapMetaDto {
    fn from(m: crate::starmap::StarMapMeta) -> Self {
        Self {
            starmap_id: m.starmap_id,
            title: m.title,
            description: m.description,
            project_id: m.project_id,
            accent_color: m.accent_color,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

impl From<StarMapMetaDto> for crate::starmap::StarMapMeta {
    fn from(d: StarMapMetaDto) -> Self {
        Self {
            starmap_id: d.starmap_id,
            title: d.title,
            description: d.description,
            project_id: d.project_id,
            accent_color: d.accent_color,
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapReferenceDto {
    pub host_starmap_id: String,
    pub host_title: String,
    pub ref_type: String,
    pub ref_id: String,
    pub target_starmap_id: String,
}

impl From<crate::starmap::StarMapReference> for StarMapReferenceDto {
    fn from(r: crate::starmap::StarMapReference) -> Self {
        Self {
            host_starmap_id: r.host_starmap_id,
            host_title: r.host_title,
            ref_type: r.ref_type,
            ref_id: r.ref_id,
            target_starmap_id: r.target_starmap_id,
        }
    }
}

impl From<StarMapReferenceDto> for crate::starmap::StarMapReference {
    fn from(d: StarMapReferenceDto) -> Self {
        Self {
            host_starmap_id: d.host_starmap_id,
            host_title: d.host_title,
            ref_type: d.ref_type,
            ref_id: d.ref_id,
            target_starmap_id: d.target_starmap_id,
        }
    }
}
