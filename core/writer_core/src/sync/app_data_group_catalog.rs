//! Versioned App data-group catalog.
//!
//! The catalog is deliberately separate from `targets.sync.json`: that file tracks
//! project lifecycle, while this one selects the active App-data layout. When it is
//! absent, the old flat `app/` manifest remains the sole source of truth. Once
//! present, its group records select the legacy flat prefix (`None`) or an
//! immutable grouped generation (`Some(generation)`).
//!
//! The stable grouped tree is marked in [`APP_DATA_GROUP_LAYOUT_REMOTE_PATH`], a
//! sidecar file added for the schema-3 migration.  The catalog itself stays at
//! schema 2 so already-installed clients keep working; those clients never read
//! the sidecar, and the last legacy generation pointer stays readable for them.

use serde::{Deserialize, Serialize};

use crate::sync::provider::model::{RemoteVersion, WritePrecondition};
use crate::sync::provider::SyncProvider;
use crate::sync::types::{AppDataGroup, ManifestFileRecord, SyncManifest};

/// Root catalog path, relative to the remote repository root.
pub const APP_DATA_GROUP_CATALOG_REMOTE_PATH: &str = "app/app-meta/sync/app_groups.sync.json";

/// Legacy flat App manifest path, relative to the remote repository root.
pub const LEGACY_APP_MANIFEST_REMOTE_PATH: &str = "app/app-meta/sync/manifest.sync.json";

/// Current catalog wire version.
///
/// The catalog deliberately stays at schema 2: already-installed clients reject
/// unknown schema versions, so the stable-tree migration cannot travel in this
/// file.  It lives in [`APP_DATA_GROUP_LAYOUT_REMOTE_PATH`] instead, which old
/// clients never read or rewrite.
pub const APP_DATA_GROUP_CATALOG_SCHEMA_VERSION: u32 = 2;

/// Schema version used by catalogs that embedded `tree_snapshot` directly.
/// Such catalogs are read only so the marker can move into the layout sidecar;
/// new writes always use schema 2 plus the sidecar.
const TREE_SNAPSHOT_CATALOG_SCHEMA_VERSION: u32 = 3;

/// Stable-tree layout marker path, relative to the remote repository root.
///
/// Kept separate from the catalog so a schema-2 client that adds a new group and
/// re-serializes the catalog cannot drop the marker.
pub const APP_DATA_GROUP_LAYOUT_REMOTE_PATH: &str = "app/app-meta/sync/app_groups_layout.sync.json";

/// Wire version of the stable-tree layout marker.
pub const APP_DATA_GROUP_LAYOUT_SCHEMA_VERSION: u32 = 3;

/// One App group and the generation selected as its current remote snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppDataGroupRecord {
    pub group_id: String,
    /// Legacy generation pointer. `None` with `tree_snapshot = false` means the
    /// group still reads from the legacy flat `app/` layout.  For a stable group
    /// it stays as the last generation published by a schema-2 client so old
    /// clients keep a readable snapshot; new clients prefer `tree_snapshot`.
    #[serde(default)]
    pub active_generation: Option<String>,
    /// New layout: the group is published as a stable, complete tree under
    /// `app/__groups__/<storage_key>/`.
    ///
    /// This field is only read from legacy schema-3 catalogs.  New writes omit it
    /// and use [`AppDataGroupLayout`] instead.
    #[serde(default, skip_serializing)]
    pub tree_snapshot: bool,
}

/// Stable-tree groups, stored in a sidecar file old clients never touch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppDataGroupLayout {
    pub schema_version: u32,
    #[serde(default)]
    pub stable_groups: Vec<String>,
}

impl Default for AppDataGroupLayout {
    fn default() -> Self {
        Self {
            schema_version: APP_DATA_GROUP_LAYOUT_SCHEMA_VERSION,
            stable_groups: Vec::new(),
        }
    }
}

impl AppDataGroupLayout {
    /// Validate schema, stable ids, and duplicate entries.
    pub fn validate(&self) -> crate::Result<()> {
        if self.schema_version != APP_DATA_GROUP_LAYOUT_SCHEMA_VERSION {
            return Err(crate::Error::Other(format!(
                "unsupported app data group layout schema_version {}",
                self.schema_version
            )));
        }
        let mut seen = std::collections::HashSet::new();
        for group_id in &self.stable_groups {
            AppDataGroup::from_id(group_id)?;
            if !seen.insert(group_id.as_str()) {
                return Err(crate::Error::Other(format!(
                    "duplicate app data group id in layout: {group_id}"
                )));
            }
        }
        Ok(())
    }

    fn normalized(mut self) -> Self {
        self.schema_version = APP_DATA_GROUP_LAYOUT_SCHEMA_VERSION;
        self.stable_groups.sort();
        self.stable_groups.dedup();
        self
    }

    fn is_stable(&self, group_id: &str) -> bool {
        self.stable_groups
            .binary_search_by(|candidate| candidate.as_str().cmp(group_id))
            .is_ok()
    }

    fn mark_stable(&mut self, group_id: &str) {
        if !self.is_stable(group_id) {
            self.stable_groups.push(group_id.to_string());
            self.stable_groups.sort();
        }
    }

    fn from_catalog(catalog: &AppDataGroupCatalog) -> Self {
        let mut layout = Self::default();
        for record in &catalog.groups {
            if record.tree_snapshot {
                layout.mark_stable(&record.group_id);
            }
        }
        layout
    }

    fn apply(&self, catalog: &mut AppDataGroupCatalog) {
        catalog.schema_version = APP_DATA_GROUP_CATALOG_SCHEMA_VERSION;
        for record in &mut catalog.groups {
            record.tree_snapshot = self.is_stable(&record.group_id);
        }
    }
}

/// The active App layout. Each group has one deterministic source at a time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppDataGroupCatalog {
    pub schema_version: u32,
    pub groups: Vec<AppDataGroupRecord>,
}

/// Remote catalog plus the provider version used for its next CAS update.
#[derive(Debug, Clone)]
pub struct RemoteAppDataGroupCatalogSnapshot {
    pub catalog: AppDataGroupCatalog,
    pub version: RemoteVersion,
    pub catalog_present: bool,
    layout: AppDataGroupLayout,
    layout_version: RemoteVersion,
    layout_present: bool,
}

impl Default for AppDataGroupCatalog {
    fn default() -> Self {
        Self {
            schema_version: APP_DATA_GROUP_CATALOG_SCHEMA_VERSION,
            groups: Vec::new(),
        }
    }
}

impl AppDataGroupCatalog {
    /// Validate schema, stable ids, and duplicate entries before using remote state.
    pub fn validate(&self) -> crate::Result<()> {
        if !matches!(
            self.schema_version,
            APP_DATA_GROUP_CATALOG_SCHEMA_VERSION | TREE_SNAPSHOT_CATALOG_SCHEMA_VERSION
        ) {
            return Err(crate::Error::Other(format!(
                "unsupported app data group catalog schema_version {}",
                self.schema_version
            )));
        }
        let mut seen = std::collections::HashSet::new();
        for record in &self.groups {
            AppDataGroup::from_id(&record.group_id)?;
            if !seen.insert(record.group_id.as_str()) {
                return Err(crate::Error::Other(format!(
                    "duplicate app data group id in catalog: {}",
                    record.group_id
                )));
            }
            if let Some(generation) = &record.active_generation {
                crate::sync::target_lifecycle::validate_generation_id(generation)?;
            }
        }
        Ok(())
    }

    pub fn record(&self, group: &AppDataGroup) -> Option<&AppDataGroupRecord> {
        let group_id = group.id();
        self.groups
            .iter()
            .find(|record| record.group_id == group_id)
    }

    pub fn upsert(&mut self, group: &AppDataGroup, active_generation: Option<String>) {
        let group_id = group.id();
        if let Some(record) = self
            .groups
            .iter_mut()
            .find(|record| record.group_id == group_id)
        {
            record.active_generation = active_generation;
            record.tree_snapshot = false;
        } else {
            self.groups.push(AppDataGroupRecord {
                group_id,
                active_generation,
                tree_snapshot: false,
            });
            self.groups.sort_by(|a, b| a.group_id.cmp(&b.group_id));
        }
    }

    /// Select the stable grouped tree as the group's remote snapshot.
    ///
    /// The last legacy generation pointer is retained: schema-2 clients keep
    /// reading that immutable snapshot, while the sidecar makes new clients use
    /// the stable tree.  The catalog wire version itself stays at 2.
    pub fn upsert_tree(&mut self, group: &AppDataGroup) {
        let group_id = group.id();
        self.schema_version = APP_DATA_GROUP_CATALOG_SCHEMA_VERSION;
        if let Some(record) = self
            .groups
            .iter_mut()
            .find(|record| record.group_id == group_id)
        {
            record.tree_snapshot = true;
        } else {
            self.groups.push(AppDataGroupRecord {
                group_id,
                active_generation: None,
                tree_snapshot: true,
            });
            self.groups.sort_by(|a, b| a.group_id.cmp(&b.group_id));
        }
    }
}

fn parse_remote_catalog(bytes: &[u8]) -> crate::Result<AppDataGroupCatalog> {
    serde_json::from_slice(bytes).map_err(|e| {
        crate::Error::Other(format!(
            "parse app data group catalog {}: {e}",
            APP_DATA_GROUP_CATALOG_REMOTE_PATH
        ))
    })
}

/// Read the stable-tree layout sidecar.  Absence is not an error: a legacy
/// repository has no marker file and all groups keep their generation layout.
fn read_remote_layout(
    provider: &dyn SyncProvider,
) -> crate::Result<(AppDataGroupLayout, RemoteVersion, bool)> {
    match provider
        .read(APP_DATA_GROUP_LAYOUT_REMOTE_PATH)
        .map_err(crate::Error::from)?
    {
        Some(object) => {
            let layout: AppDataGroupLayout =
                serde_json::from_slice(&object.content).map_err(|e| {
                    crate::Error::Other(format!(
                        "parse app data group layout {}: {e}",
                        APP_DATA_GROUP_LAYOUT_REMOTE_PATH
                    ))
                })?;
            let layout = layout.normalized();
            layout.validate()?;
            Ok((layout, object.version, true))
        }
        None => Ok((
            AppDataGroupLayout::default(),
            RemoteVersion::new("__nonexistent__"),
            false,
        )),
    }
}

/// Fold stable-tree markers embedded by older schema-3 catalogs into the
/// sidecar representation.
fn merge_legacy_tree_markers(layout: &mut AppDataGroupLayout, catalog: &AppDataGroupCatalog) {
    for record in &catalog.groups {
        if record.tree_snapshot {
            layout.mark_stable(&record.group_id);
        }
    }
}

/// Load the group catalog. Before a persisted catalog exists, seed it with legacy group ids that
/// remain mapped to the old flat `app/` source until each group is published.
///
/// The catalog file stays schema 2 so already-installed clients keep syncing the
/// legacy generation/flat layout.  The stable-tree marker lives in the separate
/// [`APP_DATA_GROUP_LAYOUT_REMOTE_PATH`] sidecar, which those clients never read
/// or rewrite.  New clients read both files: legacy generation data remains
/// readable, and every upgraded client converges on the stable tree.
pub fn load_remote_app_data_group_catalog(
    provider: &dyn SyncProvider,
) -> crate::Result<RemoteAppDataGroupCatalogSnapshot> {
    let (mut layout, layout_version, layout_present) = read_remote_layout(provider)?;
    if let Some(object) = provider
        .read(APP_DATA_GROUP_CATALOG_REMOTE_PATH)
        .map_err(crate::Error::from)?
    {
        let mut catalog = parse_remote_catalog(&object.content)?;
        catalog.validate()?;
        merge_legacy_tree_markers(&mut layout, &catalog);
        layout = layout.normalized();
        layout.apply(&mut catalog);
        return Ok(RemoteAppDataGroupCatalogSnapshot {
            catalog,
            version: object.version,
            catalog_present: true,
            layout,
            layout_version,
            layout_present,
        });
    }

    let legacy_groups = discover_remote_app_data_groups(provider)?;
    let mut catalog = AppDataGroupCatalog::default();
    for group in legacy_groups {
        catalog.upsert(&group, None);
    }
    layout = layout.normalized();
    layout.apply(&mut catalog);
    Ok(RemoteAppDataGroupCatalogSnapshot {
        catalog,
        version: RemoteVersion::new("__nonexistent__"),
        catalog_present: false,
        layout,
        layout_version,
        layout_present,
    })
}

/// CAS-write one complete group catalog snapshot.
///
/// The sidecar is written first: if it succeeds but the catalog CAS fails, the
/// next round still sees the stable-tree marker and only republishes the
/// catalog; the reverse order could silently drop the layout marker.
pub fn write_remote_app_data_group_catalog_once(
    provider: &dyn SyncProvider,
    snapshot: &RemoteAppDataGroupCatalogSnapshot,
) -> crate::Result<RemoteAppDataGroupCatalogSnapshot> {
    snapshot.catalog.validate()?;
    let layout = AppDataGroupLayout::from_catalog(&snapshot.catalog);
    let write_layout = snapshot.layout_present || !layout.stable_groups.is_empty();
    let (layout_version, layout_present) = if write_layout {
        if !snapshot.layout_present || layout != snapshot.layout {
            let content = serde_json::to_vec(&layout).map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "serialize app data group layout: {e}"
                )))
            })?;
            let precondition = if snapshot.layout_version.as_str() == "__nonexistent__" {
                WritePrecondition::CreateNew
            } else {
                WritePrecondition::IfMatch(snapshot.layout_version.clone())
            };
            let version = provider
                .write(APP_DATA_GROUP_LAYOUT_REMOTE_PATH, &content, precondition)
                .map_err(crate::Error::from)?;
            (version, true)
        } else {
            (snapshot.layout_version.clone(), true)
        }
    } else {
        (snapshot.layout_version.clone(), false)
    };

    let mut catalog = snapshot.catalog.clone();
    catalog.schema_version = APP_DATA_GROUP_CATALOG_SCHEMA_VERSION;
    let content = serde_json::to_vec(&catalog).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "serialize app data group catalog: {e}"
        )))
    })?;
    let precondition = if snapshot.version.as_str() == "__nonexistent__" {
        WritePrecondition::CreateNew
    } else {
        WritePrecondition::IfMatch(snapshot.version.clone())
    };
    let version = provider
        .write(APP_DATA_GROUP_CATALOG_REMOTE_PATH, &content, precondition)
        .map_err(crate::Error::from)?;
    Ok(RemoteAppDataGroupCatalogSnapshot {
        catalog: snapshot.catalog.clone(),
        version,
        catalog_present: true,
        layout,
        layout_version,
        layout_present,
    })
}

/// Read and validate the remote App group catalog, or derive group ids from the
/// legacy App manifest when no catalog exists. This helper is read-only.
pub fn discover_remote_app_data_groups(
    provider: &dyn SyncProvider,
) -> crate::Result<Vec<AppDataGroup>> {
    let (mut layout, _, _) = read_remote_layout(provider)?;
    let catalog_object = provider
        .read(APP_DATA_GROUP_CATALOG_REMOTE_PATH)
        .map_err(crate::Error::from)?;
    if let Some(object) = catalog_object {
        let mut catalog = parse_remote_catalog(&object.content)?;
        catalog.validate()?;
        merge_legacy_tree_markers(&mut layout, &catalog);
        layout = layout.normalized();
        layout.apply(&mut catalog);
        let mut groups: std::collections::BTreeMap<String, AppDataGroup> = catalog
            .groups
            .iter()
            .map(|record| AppDataGroup::from_id(&record.group_id).map(|group| (group.id(), group)))
            .collect::<crate::Result<_>>()?;
        let index_group = AppDataGroup::StarmapIndex;
        if let Some(record) = catalog.record(&index_group) {
            let source_prefix = group_source_prefix(record, &index_group);
            let index_path = format!("{source_prefix}/starmaps/index.json");
            if let Some(object) = provider.read(&index_path).map_err(crate::Error::from)? {
                groups.insert(index_group.id(), index_group);
                for group in groups_from_index_bytes(&object.content)? {
                    groups.insert(group.id(), group);
                }
            }
        }
        return Ok(groups.into_values().collect());
    }

    let manifest_object = provider
        .read(LEGACY_APP_MANIFEST_REMOTE_PATH)
        .map_err(crate::Error::from)?;
    let mut groups: std::collections::BTreeMap<String, AppDataGroup> =
        if let Some(object) = manifest_object {
            let manifest: SyncManifest = serde_json::from_slice(&object.content).map_err(|e| {
                crate::Error::Other(format!(
                    "parse legacy App manifest {}: {e}",
                    LEGACY_APP_MANIFEST_REMOTE_PATH
                ))
            })?;
            groups_from_manifest_records(&manifest.files)
                .into_iter()
                .map(|group| (group.id(), group))
                .collect()
        } else {
            std::collections::BTreeMap::new()
        };
    if let Some(object) = provider
        .read("app/starmaps/index.json")
        .map_err(crate::Error::from)?
    {
        groups.insert(AppDataGroup::StarmapIndex.id(), AppDataGroup::StarmapIndex);
        for group in groups_from_index_bytes(&object.content)? {
            groups.insert(group.id(), group);
        }
    }
    Ok(groups.into_values().collect())
}

/// Enumerate groups represented by local App data. The remote group list is unioned
/// by the planner so remote-only/deleted groups remain reachable.
pub fn discover_local_app_data_groups(root: &std::path::Path) -> crate::Result<Vec<AppDataGroup>> {
    let mut groups = std::collections::BTreeMap::<String, AppDataGroup>::new();
    for group in [AppDataGroup::Global] {
        groups.insert(group.id(), group);
    }

    // Keep a group planned when its data directory disappeared but its local
    // manifest/state still records paths. The merge then reports unresolved
    // missing files instead of silently forgetting the group.
    let group_state_root = root.join("app-meta/sync/groups");
    if group_state_root.is_dir() {
        let entries = std::fs::read_dir(&group_state_root).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "discover_local_app_data_groups: read {}: {e}",
                group_state_root.display()
            )))
        })?;
        for entry in entries {
            let entry = entry.map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "discover_local_app_data_groups: enumerate {}: {e}",
                    group_state_root.display()
                )))
            })?;
            if !entry
                .file_type()
                .map_err(|e| {
                    crate::Error::Io(std::io::Error::other(format!(
                        "discover_local_app_data_groups: metadata {}: {e}",
                        entry.path().display()
                    )))
                })?
                .is_dir()
            {
                continue;
            }
            let group_dir = entry.path();
            if !group_dir.join("state.local.json").is_file()
                && !group_dir.join("manifest.sync.json").is_file()
            {
                continue;
            }
            let group_key = entry.file_name().to_string_lossy().into_owned();
            if let Some(group) = group_from_storage_key(&group_key) {
                groups.insert(group.id(), group);
            }
        }
    }

    // v1 App manifests are read-only migration input. They may be the only
    // surviving record of a map whose whole local starmaps/<id> directory is gone.
    let legacy_manifest_path = root.join("app-meta/sync/manifest.sync.json");
    if legacy_manifest_path.is_file() {
        let bytes = std::fs::read(&legacy_manifest_path).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "discover_local_app_data_groups: read {}: {e}",
                legacy_manifest_path.display()
            )))
        })?;
        let manifest: SyncManifest = serde_json::from_slice(&bytes).map_err(|e| {
            crate::Error::Other(format!(
                "discover_local_app_data_groups: parse {}: {e}",
                legacy_manifest_path.display()
            ))
        })?;
        for group in groups_from_manifest_records(&manifest.files) {
            groups.insert(group.id(), group);
        }
    }

    // `index.json` is a single aggregate record containing membership for every
    // map. Keep it as a dependent group, not as a peer of the map groups.
    if root.join("starmaps/index.json").is_file() {
        let group = AppDataGroup::StarmapIndex;
        groups.insert(group.id(), group);
        let bytes = std::fs::read(root.join("starmaps/index.json")).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "discover_local_app_data_groups: read starmaps/index.json: {e}"
            )))
        })?;
        for group in groups_from_index_bytes(&bytes)? {
            groups.insert(group.id(), group);
        }
    }

    let starmaps_root = root.join("starmaps");
    if starmaps_root.exists() {
        let entries = std::fs::read_dir(&starmaps_root).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "discover_local_app_data_groups: read {}: {e}",
                starmaps_root.display()
            )))
        })?;
        for entry in entries {
            let entry = entry.map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "discover_local_app_data_groups: enumerate {}: {e}",
                    starmaps_root.display()
                )))
            })?;
            let file_type = entry.file_type().map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "discover_local_app_data_groups: metadata {}: {e}",
                    entry.path().display()
                )))
            })?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let id = if file_type.is_dir() {
                Some(name)
            } else if file_type.is_file() {
                name.strip_suffix(".meta.json").map(str::to_string)
            } else {
                None
            };
            let Some(id) = id else {
                continue;
            };
            if id == "index" || id.is_empty() {
                continue;
            }
            let group = AppDataGroup::from_id(&format!("starmap:{id}"))?;
            groups.insert(group.id(), group);
        }
    }

    // Each local stats event partition (`stats/events/<device_id>/`) is an
    // independent group; invalid directory names are not treated as groups.
    let stats_events_root = root.join("stats/events");
    if stats_events_root.is_dir() {
        let entries = std::fs::read_dir(&stats_events_root).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "discover_local_app_data_groups: read {}: {e}",
                stats_events_root.display()
            )))
        })?;
        for entry in entries {
            let entry = entry.map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "discover_local_app_data_groups: enumerate {}: {e}",
                    stats_events_root.display()
                )))
            })?;
            if !entry
                .file_type()
                .map_err(|e| {
                    crate::Error::Io(std::io::Error::other(format!(
                        "discover_local_app_data_groups: metadata {}: {e}",
                        entry.path().display()
                    )))
                })?
                .is_dir()
            {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Ok(group) = AppDataGroup::from_id(&format!("stats_device:{name}")) {
                groups.insert(group.id(), group);
            }
        }
    }

    // 旧版单机统计库（#955）：升级后若自动同步先于统计页发生，
    // 也必须把真实本机设备纳入 StatsDevice 枚举，旧历史才能进入增量上传计划。
    // 这里只枚举目标：不迁移、不写盘（迁移由统计页或本组同步入口完成）。
    if crate::writing_stats::migration::legacy_events_present(root) {
        if let Ok(info) = crate::settings::load_device_info(root) {
            if let Ok(group) = AppDataGroup::from_id(&format!("stats_device:{}", info.device_id)) {
                groups.insert(group.id(), group);
            }
        }
    }

    // The aggregate index is a dependent publication target even when its local
    // file is missing; transfer must attempt recovery or report the damaged path.
    if groups
        .values()
        .any(|group| matches!(group, AppDataGroup::Starmap(_)))
    {
        let group = AppDataGroup::StarmapIndex;
        groups.insert(group.id(), group);
    }

    Ok(groups.into_values().collect())
}

fn group_from_storage_key(key: &str) -> Option<AppDataGroup> {
    match key {
        "global" => return Some(AppDataGroup::Global),
        "starmap-index" => return Some(AppDataGroup::StarmapIndex),
        _ => {}
    }
    if let Some(encoded) = key.strip_prefix("starmap-") {
        let id = decode_hex_segment(encoded)?;
        return AppDataGroup::from_id(&format!("starmap:{id}")).ok();
    }
    if let Some(encoded) = key.strip_prefix("stats-device-") {
        let id = decode_hex_segment(encoded)?;
        return AppDataGroup::from_id(&format!("stats_device:{id}")).ok();
    }
    None
}

fn decode_hex_segment(encoded: &str) -> Option<String> {
    let encoded = encoded.as_bytes();
    if encoded.is_empty() || !encoded.len().is_multiple_of(2) {
        return None;
    }
    let bytes: Option<Vec<u8>> = encoded
        .chunks_exact(2)
        .map(|pair| Some((hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?))
        .collect();
    String::from_utf8(bytes?).ok()
}

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn groups_from_index_bytes(bytes: &[u8]) -> crate::Result<Vec<AppDataGroup>> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| {
        crate::Error::Other(format!(
            "parse starmaps/index.json while discovering groups: {e}"
        ))
    })?;
    let mut ids = Vec::new();
    if let Some(current_ids) = value
        .get("starmapIds")
        .or_else(|| value.get("starmap_ids"))
        .and_then(serde_json::Value::as_array)
    {
        for id in current_ids {
            let id = id.as_str().ok_or_else(|| {
                crate::Error::Other("starmaps/index.json contains a non-string map id".into())
            })?;
            ids.push(id.to_string());
        }
    } else if let Some(legacy_maps) = value.get("starmaps").and_then(serde_json::Value::as_array) {
        for map in legacy_maps {
            let id = map
                .get("starmapId")
                .or_else(|| map.get("starmap_id"))
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    crate::Error::Other(
                        "legacy starmaps/index.json entry is missing starmapId".into(),
                    )
                })?;
            ids.push(id.to_string());
        }
    }

    let mut groups = std::collections::BTreeMap::new();
    for id in ids {
        let group = AppDataGroup::from_id(&format!("starmap:{id}"))?;
        groups.insert(group.id(), group);
    }
    Ok(groups.into_values().collect())
}

/// Derive stable group identities from known legacy App paths. Unknown paths are
/// ignored here and remain subject to the normal whitelist validation.
pub fn groups_from_manifest_records(records: &[ManifestFileRecord]) -> Vec<AppDataGroup> {
    let mut groups = std::collections::BTreeMap::<String, AppDataGroup>::new();
    for record in records {
        let path = record.path.replace('\\', "/");
        if path == "settings.sync.json" {
            let group = AppDataGroup::Global;
            groups.insert(group.id(), group);
        } else if path == "starmaps/index.json" {
            let group = AppDataGroup::StarmapIndex;
            groups.insert(group.id(), group);
        } else if let Some(device_id) = super::types::stats_device_id_for_event_path(&path) {
            let group = AppDataGroup::StatsDevice(device_id.to_string());
            groups.insert(group.id(), group);
        } else if let Some(id) = starmap_id_for_path(&path) {
            let group = AppDataGroup::Starmap(id);
            groups.insert(group.id(), group);
        }
    }
    groups.into_values().collect()
}

fn starmap_id_for_path(path: &str) -> Option<String> {
    let suffix = path.strip_prefix("starmaps/")?;
    let id = if let Some(directory_id) = suffix.split('/').next() {
        if suffix.contains('/') {
            directory_id
        } else if let Some(meta_id) = suffix.strip_suffix(".meta.json") {
            meta_id
        } else {
            return None;
        }
    } else {
        return None;
    };
    if id.is_empty() || id == "index.json" || id == "." || id == ".." {
        return None;
    }
    Some(id.to_string())
}

/// Resolve the source prefix selected for one group by the catalog.
pub fn resolve_group_source_prefix(
    catalog: Option<&AppDataGroupCatalog>,
    group: &AppDataGroup,
) -> crate::Result<String> {
    let Some(catalog) = catalog else {
        return Ok("app".to_string());
    };
    catalog.validate()?;
    let Some(record) = catalog.record(group) else {
        // A group not yet listed in a persisted catalog is not visible remotely.
        return Ok(String::new());
    };
    Ok(group_source_prefix(record, group))
}

fn group_source_prefix(record: &AppDataGroupRecord, group: &AppDataGroup) -> String {
    if record.tree_snapshot {
        return format!("app/__groups__/{}", group.storage_key());
    }
    match &record.active_generation {
        Some(generation) => format!(
            "app/__groups__/{}/__generations__/{}",
            group.storage_key(),
            generation
        ),
        None => "app".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats_record(path: &str, device_id: &str) -> ManifestFileRecord {
        ManifestFileRecord {
            path: path.to_string(),
            content_hash: "hash".to_string(),
            updated_at_ms: 0,
            deleted_at_ms: None,
            device_id: device_id.to_string(),
            op: "upsert".to_string(),
            schema_version: 2,
        }
    }

    #[test]
    fn test_stats_device_group_round_trip() {
        let group = AppDataGroup::StatsDevice("device-abc".to_string());
        assert_eq!(group.id(), "stats_device:device-abc");
        assert_eq!(group.storage_key(), "stats-device-6465766963652d616263");
        assert_eq!(AppDataGroup::from_id(&group.id()).unwrap(), group);
        assert!(group.owns_path("stats/events/device-abc/2024-05-01.events.jsonl"));
        assert!(!group.owns_path("stats/events/device-other/2024-05-01.events.jsonl"));
        assert!(!group.owns_path("stats/events/device-abc"));
    }

    #[test]
    fn test_group_from_stats_device_storage_key() {
        let key = AppDataGroup::StatsDevice("device-abc".to_string()).storage_key();
        assert_eq!(
            group_from_storage_key(&key),
            Some(AppDataGroup::StatsDevice("device-abc".to_string()))
        );
    }

    #[test]
    fn test_stats_event_path_maps_to_stats_device_group() {
        let Some(device_id) = crate::sync::types::stats_device_id_for_event_path(
            "stats/events/device-abc/2024-05-01.events.jsonl",
        ) else {
            panic!("valid stats event path must map to a device id");
        };
        assert_eq!(device_id, "device-abc");
        // 目录名、非法日期与非事件文件都不属于统计设备组。
        assert!(crate::sync::types::stats_device_id_for_event_path(
            "stats/events/device-abc/readme.txt"
        )
        .is_none());
        assert!(crate::sync::types::stats_device_id_for_event_path(
            "stats/events/device-abc/2024-13-01.events.jsonl"
        )
        .is_none());

        let records = [stats_record(
            "stats/events/device-abc/2024-05-01.events.jsonl",
            "device-abc",
        )];
        assert_eq!(
            groups_from_manifest_records(&records),
            vec![AppDataGroup::StatsDevice("device-abc".to_string())]
        );
    }

    #[test]
    fn test_discover_local_app_data_groups_includes_stats_devices() {
        let temp = tempfile::tempdir().unwrap();
        let partition = temp.path().join("stats/events/device-abc");
        std::fs::create_dir_all(&partition).unwrap();
        std::fs::write(partition.join("2024-05-01.events.jsonl"), "").unwrap();

        let groups = discover_local_app_data_groups(temp.path()).unwrap();
        assert!(groups.contains(&AppDataGroup::StatsDevice("device-abc".to_string())));
    }

    #[test]
    fn test_discover_local_app_data_groups_enumerates_legacy_stats_device() {
        let temp = tempfile::tempdir().unwrap();
        // 旧版单机事件库存在时，即使还没有 stats/events 分区，
        // 也必须枚举出本机统计设备组，旧历史才能进入增量上传计划。
        let legacy_dir = temp.path().join("app-meta/stats/events.local");
        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(legacy_dir.join("2024-05-01.events.jsonl"), "").unwrap();
        crate::settings::ensure_device_info(temp.path(), "", "", Some("device_1")).unwrap();

        let groups = discover_local_app_data_groups(temp.path()).unwrap();
        assert!(groups.contains(&AppDataGroup::StatsDevice("device_1".to_string())));
        // 只枚举目标，不触发迁移：不产生新的 stats/events 分区目录。
        assert!(!temp.path().join("stats/events").exists());
    }

    #[test]
    fn test_resolve_group_source_prefix_uses_stats_device_storage_key() {
        let group = AppDataGroup::StatsDevice("device-abc".to_string());
        let catalog = AppDataGroupCatalog {
            schema_version: APP_DATA_GROUP_CATALOG_SCHEMA_VERSION,
            groups: vec![AppDataGroupRecord {
                group_id: group.id(),
                active_generation: Some("a".repeat(40)),
                tree_snapshot: false,
            }],
        };
        let prefix = resolve_group_source_prefix(Some(&catalog), &group).unwrap();
        assert_eq!(
            prefix,
            format!(
                "app/__groups__/{}/__generations__/{}",
                group.storage_key(),
                "a".repeat(40)
            )
        );
    }

    use crate::sync::provider::memory::MemoryProvider;
    use crate::sync::provider::SyncProvider;

    fn catalog_snapshot(catalog: AppDataGroupCatalog) -> RemoteAppDataGroupCatalogSnapshot {
        RemoteAppDataGroupCatalogSnapshot {
            catalog,
            version: RemoteVersion::new("__nonexistent__"),
            catalog_present: false,
            layout: AppDataGroupLayout::default(),
            layout_version: RemoteVersion::new("__nonexistent__"),
            layout_present: false,
        }
    }

    #[test]
    fn test_catalog_wire_format_stays_schema_two_without_tree_marker() {
        let provider = MemoryProvider::new();
        let group = AppDataGroup::StarmapIndex;
        let mut catalog = AppDataGroupCatalog::default();
        catalog.upsert_tree(&group);

        let written =
            write_remote_app_data_group_catalog_once(&provider, &catalog_snapshot(catalog))
                .unwrap();

        let catalog_bytes = provider
            .read(APP_DATA_GROUP_CATALOG_REMOTE_PATH)
            .unwrap()
            .unwrap()
            .content;
        let catalog_json: serde_json::Value = serde_json::from_slice(&catalog_bytes).unwrap();
        assert_eq!(catalog_json["schema_version"].as_u64(), Some(2));
        assert!(catalog_json["groups"][0].get("tree_snapshot").is_none());

        let layout_bytes = provider
            .read(APP_DATA_GROUP_LAYOUT_REMOTE_PATH)
            .unwrap()
            .unwrap()
            .content;
        let layout_json: serde_json::Value = serde_json::from_slice(&layout_bytes).unwrap();
        assert_eq!(layout_json["schema_version"].as_u64(), Some(3));
        assert_eq!(
            layout_json["stable_groups"][0].as_str(),
            Some(group.id().as_str())
        );

        let reloaded = load_remote_app_data_group_catalog(&provider).unwrap();
        assert!(reloaded.catalog.record(&group).unwrap().tree_snapshot);
        assert_eq!(
            resolve_group_source_prefix(Some(&reloaded.catalog), &group).unwrap(),
            format!("app/__groups__/{}", group.storage_key())
        );
        assert_eq!(written.catalog.schema_version, 2);
    }

    #[test]
    fn test_sidecar_keeps_stable_marker_after_old_client_catalog_rewrite() {
        let provider = MemoryProvider::new();
        let group = AppDataGroup::StarmapIndex;
        let mut catalog = AppDataGroupCatalog::default();
        catalog.upsert_tree(&group);
        write_remote_app_data_group_catalog_once(&provider, &catalog_snapshot(catalog)).unwrap();

        // A schema-2 client rewrites the whole catalog and knows nothing about
        // the sidecar.  The stable-tree marker must survive.
        let old_client_catalog = serde_json::json!({
            "schema_version": 2,
            "groups": [{
                "group_id": group.id(),
                "active_generation": null
            }]
        });
        provider
            .write(
                APP_DATA_GROUP_CATALOG_REMOTE_PATH,
                &serde_json::to_vec(&old_client_catalog).unwrap(),
                WritePrecondition::Unconditional,
            )
            .unwrap();

        let reloaded = load_remote_app_data_group_catalog(&provider).unwrap();
        assert_eq!(reloaded.catalog.schema_version, 2);
        assert!(reloaded.catalog.record(&group).unwrap().tree_snapshot);
    }

    #[test]
    fn test_legacy_schema_three_catalog_moves_marker_to_sidecar() {
        let provider = MemoryProvider::new();
        let group = AppDataGroup::StarmapIndex;
        let legacy_catalog = serde_json::json!({
            "schema_version": 3,
            "groups": [{
                "group_id": group.id(),
                "active_generation": null,
                "tree_snapshot": true
            }]
        });
        provider
            .write(
                APP_DATA_GROUP_CATALOG_REMOTE_PATH,
                &serde_json::to_vec(&legacy_catalog).unwrap(),
                WritePrecondition::Unconditional,
            )
            .unwrap();

        let loaded = load_remote_app_data_group_catalog(&provider).unwrap();
        assert_eq!(loaded.catalog.schema_version, 2);
        assert!(loaded.catalog.record(&group).unwrap().tree_snapshot);

        let written = write_remote_app_data_group_catalog_once(&provider, &loaded).unwrap();
        assert!(written.layout_present);
        let catalog_bytes = provider
            .read(APP_DATA_GROUP_CATALOG_REMOTE_PATH)
            .unwrap()
            .unwrap()
            .content;
        let catalog_json: serde_json::Value = serde_json::from_slice(&catalog_bytes).unwrap();
        assert_eq!(catalog_json["schema_version"].as_u64(), Some(2));
        assert!(provider
            .read(APP_DATA_GROUP_LAYOUT_REMOTE_PATH)
            .unwrap()
            .is_some());
    }
}
