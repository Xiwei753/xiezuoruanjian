//! Versioned App data-group catalog.
//!
//! The catalog is deliberately separate from `targets.sync.json`: that file tracks
//! project lifecycle, while this one selects the active App-data layout. When it is
//! absent, the old flat `app/` manifest remains the sole source of truth. Once
//! present, its group records select the legacy flat prefix (`None`), an
//! immutable grouped generation (`Some(generation)`), or the stable grouped
//! tree (`tree_snapshot = true`).

use serde::{Deserialize, Serialize};

use crate::sync::provider::model::{RemoteVersion, WritePrecondition};
use crate::sync::provider::SyncProvider;
use crate::sync::types::{AppDataGroup, ManifestFileRecord, SyncManifest};

/// Root catalog path, relative to the remote repository root.
pub const APP_DATA_GROUP_CATALOG_REMOTE_PATH: &str = "app/app-meta/sync/app_groups.sync.json";

/// Legacy flat App manifest path, relative to the remote repository root.
pub const LEGACY_APP_MANIFEST_REMOTE_PATH: &str = "app/app-meta/sync/manifest.sync.json";

/// Current catalog wire version.
pub const APP_DATA_GROUP_CATALOG_SCHEMA_VERSION: u32 = 2;

/// One App group and the generation selected as its current remote snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppDataGroupRecord {
    pub group_id: String,
    /// Legacy generation pointer. `None` with `tree_snapshot = false` means the
    /// group still reads from the legacy flat `app/` layout.
    #[serde(default)]
    pub active_generation: Option<String>,
    /// New layout: the group is published as a stable, complete tree under
    /// `app/__groups__/<storage_key>/`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub tree_snapshot: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
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
        if self.schema_version != APP_DATA_GROUP_CATALOG_SCHEMA_VERSION {
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
            if record.tree_snapshot && record.active_generation.is_some() {
                return Err(crate::Error::Other(format!(
                    "app data group catalog record cannot select both a generation and a stable tree: {}",
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
    pub fn upsert_tree(&mut self, group: &AppDataGroup) {
        let group_id = group.id();
        if let Some(record) = self
            .groups
            .iter_mut()
            .find(|record| record.group_id == group_id)
        {
            record.active_generation = None;
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

/// Load the group catalog. Before v2 exists, seed it with legacy group ids that
/// remain mapped to the old flat `app/` source until each group is published.
///
/// This is a one-way client upgrade boundary: once the first v2 catalog is
/// published, every active client must understand the catalog. Older clients
/// continue writing the flat `app/` tree, and those writes are deliberately not
/// treated as a second live source for groups already pointing at generations.
/// Keep old clients paused until all of them have upgraded; legacy objects stay
/// available as migration input and recovery data.
pub fn load_remote_app_data_group_catalog(
    provider: &dyn SyncProvider,
) -> crate::Result<RemoteAppDataGroupCatalogSnapshot> {
    if let Some(object) = provider
        .read(APP_DATA_GROUP_CATALOG_REMOTE_PATH)
        .map_err(crate::Error::from)?
    {
        let catalog: AppDataGroupCatalog =
            serde_json::from_slice(&object.content).map_err(|e| {
                crate::Error::Other(format!(
                    "parse app data group catalog {}: {e}",
                    APP_DATA_GROUP_CATALOG_REMOTE_PATH
                ))
            })?;
        catalog.validate()?;
        return Ok(RemoteAppDataGroupCatalogSnapshot {
            catalog,
            version: object.version,
            catalog_present: true,
        });
    }

    let legacy_groups = discover_remote_app_data_groups(provider)?;
    let mut catalog = AppDataGroupCatalog::default();
    for group in legacy_groups {
        catalog.upsert(&group, None);
    }
    Ok(RemoteAppDataGroupCatalogSnapshot {
        catalog,
        version: RemoteVersion::new("__nonexistent__"),
        catalog_present: false,
    })
}

/// CAS-write one complete group catalog snapshot.
pub fn write_remote_app_data_group_catalog_once(
    provider: &dyn SyncProvider,
    snapshot: &RemoteAppDataGroupCatalogSnapshot,
) -> crate::Result<RemoteAppDataGroupCatalogSnapshot> {
    snapshot.catalog.validate()?;
    let content = serde_json::to_vec(&snapshot.catalog).map_err(|e| {
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
    })
}

/// Read and validate the remote App group catalog, or derive group ids from the
/// legacy App manifest when no catalog exists. This helper is read-only.
pub fn discover_remote_app_data_groups(
    provider: &dyn SyncProvider,
) -> crate::Result<Vec<AppDataGroup>> {
    let catalog_object = provider
        .read(APP_DATA_GROUP_CATALOG_REMOTE_PATH)
        .map_err(crate::Error::from)?;
    if let Some(object) = catalog_object {
        let catalog: AppDataGroupCatalog =
            serde_json::from_slice(&object.content).map_err(|e| {
                crate::Error::Other(format!(
                    "parse app data group catalog {}: {e}",
                    APP_DATA_GROUP_CATALOG_REMOTE_PATH
                ))
            })?;
        catalog.validate()?;
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
        // A group not yet listed in a v2 catalog is not visible remotely.
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
}
