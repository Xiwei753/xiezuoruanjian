use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{de::DeserializeOwned, Serialize};

use crate::object_ref::{ObjectRef, ObjectScope};

use super::model::{
    AnnotationKind, AnnotationMutation, LinkTarget, NamedEntry, ObjectLink, Tag, TagBinding,
    TextAnchor, DEFAULT_TAG_NAMES,
};

const COLLECTIONS: [&str; 5] = ["tags", "bindings", "entries", "anchors", "links"];

#[derive(Debug, Clone, Default)]
pub struct PreparedTextAnchorChanges {
    /// Project-relative JSON files staged with the chapter body save.
    pub files: Vec<(String, String)>,
    /// Anchor records as they will be stored after the transaction commits.
    pub committed_anchors: Vec<TextAnchor>,
}

/// Annotation persistence for one namespace.
///
/// Each record has its own JSON file and is written through the shared atomic
/// writer. The directory scan is the rebuildable index; no second persistent
/// index can overwrite records created on another device.
///
/// A store belongs to exactly one namespace: a project store lives at
/// `<project_root>/annotations/<collection>` and holds project tags and project
/// links; the global store lives at `<app_data_root>/annotations/<collection>`
/// and holds shared-asset tags and links owned by canvases without a project.
/// The two directories are never mixed.
#[derive(Debug, Clone)]
pub struct AnnotationStore {
    /// Project root for project stores, app data root for the global store.
    project_root: PathBuf,
    /// Tag/link namespace; an empty string means the global asset domain.
    project_id: String,
}

impl AnnotationStore {
    pub fn new(project_root: &Path, project_id: &str) -> Self {
        Self {
            project_root: project_root.to_path_buf(),
            project_id: project_id.to_string(),
        }
    }

    /// Store for the global asset domain: `<app_data_root>/annotations/<collection>`.
    pub fn global(app_data_root: &Path) -> Self {
        Self {
            project_root: app_data_root.to_path_buf(),
            project_id: String::new(),
        }
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    /// Namespace this store owns. Project records and global records never mix.
    pub fn scope(&self) -> ObjectScope {
        ObjectScope::from_project_id(Some(self.project_id.as_str()))
    }

    pub fn is_global(&self) -> bool {
        self.project_id.is_empty()
    }

    pub fn list_tags(&self) -> crate::Result<Vec<Tag>> {
        Ok(self
            .read_collection::<Tag>("tags")?
            .into_iter()
            .filter(|record| record.deleted_at_ms.is_none())
            .collect())
    }

    /// Read all tag records, including tombstones, for sync-aware projections.
    pub fn list_tag_records(&self) -> crate::Result<Vec<Tag>> {
        self.read_collection("tags")
    }

    pub fn get_tag(&self, tag_id: &str) -> crate::Result<Option<Tag>> {
        self.read_record("tags", tag_id)
    }

    pub fn list_bindings(&self) -> crate::Result<Vec<TagBinding>> {
        Ok(self
            .read_collection::<TagBinding>("bindings")?
            .into_iter()
            .filter(|record| record.deleted_at_ms.is_none())
            .collect())
    }

    /// Read all binding records, including tombstones, for sync-aware projections.
    pub fn list_binding_records(&self) -> crate::Result<Vec<TagBinding>> {
        self.read_collection("bindings")
    }

    pub fn list_bindings_for_target(&self, target: &ObjectRef) -> crate::Result<Vec<TagBinding>> {
        Ok(self
            .list_bindings()?
            .into_iter()
            .filter(|binding| &binding.target == target)
            .collect())
    }

    pub fn list_bindings_for_tag(&self, tag_id: &str) -> crate::Result<Vec<TagBinding>> {
        Ok(self
            .list_bindings()?
            .into_iter()
            .filter(|binding| binding.tag_id == tag_id)
            .collect())
    }

    /// Active bindings recorded in this store's namespace for one typed target.
    ///
    /// The namespace filter is what keeps the project catalogue and the global
    /// asset catalogue separate even when both describe the same shared asset.
    pub fn list_bindings_for_scope(&self, target: &ObjectRef) -> crate::Result<Vec<TagBinding>> {
        Ok(self
            .list_bindings()?
            .into_iter()
            .filter(|binding| binding.project_id == self.project_id && &binding.target == target)
            .collect())
    }

    /// Find the binding for one tag/target relation, including tombstones.
    ///
    /// The relation key is namespace + tag + typed target, so the same tag name
    /// in another namespace or the same object ID of another kind never collide.
    pub fn find_binding_by_relation(
        &self,
        tag: &Tag,
        target: &ObjectRef,
    ) -> crate::Result<Option<TagBinding>> {
        let relation_key = relation_key(&tag.project_id, &tag.tag_id, target);
        let mut tombstone = None;
        for binding in self.list_binding_records()? {
            if binding.relation_key() != relation_key {
                continue;
            }
            if binding.is_active() {
                return Ok(Some(binding));
            }
            tombstone.get_or_insert(binding);
        }
        Ok(tombstone)
    }

    /// Deterministic binding ID for one tag/target relation.
    ///
    /// Used for both durable attaches and bindings staged with an unsaved
    /// chapter, so the same relation converges on one ID everywhere.
    pub fn binding_id_for_relation(tag: &Tag, target: &ObjectRef) -> String {
        format!(
            "b{:016x}",
            fnv1a64(&relation_key(&tag.project_id, &tag.tag_id, target))
        )
    }

    /// Idempotently attach one tag to one typed object.
    ///
    /// A new relation gets a deterministic binding ID derived from its relation
    /// key, so two devices that attach the same relation concurrently converge on
    /// the same file instead of producing duplicate bindings. An existing active
    /// relation returns unchanged; a tombstoned relation is revived in place with
    /// its original binding ID so the delete/restore history stays traceable.
    pub fn attach_tag(
        &self,
        tag: &Tag,
        target: &ObjectRef,
        now_ms: i64,
    ) -> crate::Result<(TagBinding, Vec<String>)> {
        self.validate_project(&tag.project_id)?;
        let stored_tag = self
            .get_tag(&tag.tag_id)?
            .filter(|record| record.deleted_at_ms.is_none());
        if stored_tag.is_none() {
            return Err(invalid_annotation("tag does not exist in this namespace"));
        }
        let relation_key = relation_key(&tag.project_id, &tag.tag_id, target);
        if let Some(binding) = self.find_binding_by_relation(tag, target)? {
            if binding.is_active() {
                return Ok((binding, Vec::new()));
            }
            let mut revived = binding;
            revived.deleted_at_ms = None;
            revived.updated_at_ms = now_ms.max(revived.updated_at_ms.saturating_add(1));
            let changed =
                self.apply_mutation(AnnotationMutation::UpsertBinding(revived.clone()))?;
            return Ok((revived, changed));
        }
        let binding = TagBinding {
            binding_id: Self::binding_id_for_relation(tag, target),
            project_id: tag.project_id.clone(),
            tag_id: tag.tag_id.clone(),
            target: target.clone(),
            updated_at_ms: now_ms,
            deleted_at_ms: None,
        };
        // The deterministic ID is derived from the relation key; a record with a
        // different relation under the same ID cannot be overwritten.
        if let Some(existing) = self.get_record::<TagBinding>("bindings", &binding.binding_id)? {
            if existing.relation_key() == relation_key {
                return Ok((existing, Vec::new()));
            }
            return Err(invalid_annotation(
                "binding ID conflicts with another tag relation",
            ));
        }
        let changed = self.apply_mutation(AnnotationMutation::UpsertBinding(binding.clone()))?;
        Ok((binding, changed))
    }

    /// Idempotently remove one tag/target relation, keeping the binding as a
    /// tombstone so a later attach can revive the same stable ID.
    pub fn detach_tag(
        &self,
        tag: &Tag,
        target: &ObjectRef,
        now_ms: i64,
    ) -> crate::Result<Vec<String>> {
        self.validate_project(&tag.project_id)?;
        let Some(mut binding) = self.find_binding_by_relation(tag, target)? else {
            return Ok(Vec::new());
        };
        if !binding.is_active() {
            return Ok(Vec::new());
        }
        binding.updated_at_ms = now_ms.max(binding.updated_at_ms.saturating_add(1));
        binding.deleted_at_ms = Some(now_ms);
        self.apply_mutation(AnnotationMutation::UpsertBinding(binding))
    }

    pub fn list_named_entries(&self) -> crate::Result<Vec<NamedEntry>> {
        Ok(self
            .read_collection::<NamedEntry>("entries")?
            .into_iter()
            .filter(|record| record.deleted_at_ms.is_none())
            .collect())
    }

    /// Read all named-entry records, including tombstones, for sync-aware projections.
    pub fn list_named_entry_records(&self) -> crate::Result<Vec<NamedEntry>> {
        self.read_collection("entries")
    }

    /// Cheap version stamp of the named-entry source directory.
    ///
    /// Corrections keep a process cache of named entries. Sync and other MCP
    /// writers modify the same `annotations/entries/*.json` files without
    /// going through this process, so "already synced once" is not a valid
    /// cache key. The stamp observes file names, sizes and modification times
    /// (plus directory metadata for create/delete/rename) without parsing any
    /// JSON, so callers can detect an external rewrite on each correction
    /// request instead of re-reading every record.
    ///
    /// In-place overwrites are covered by the file-level stamp; atomic
    /// replace/rename is covered by the directory stamp plus the file's inode
    /// (Unix), so it is detected even when size and coarse mtime coincide.
    pub fn named_entries_source_stamp(&self) -> crate::Result<String> {
        let dir = self.collection_path("entries")?;
        let mut description = String::new();
        match fs::read_dir(&dir) {
            Ok(entries) => {
                let mut files = Vec::new();
                for entry in entries {
                    let entry = entry?;
                    if !entry.file_type()?.is_file()
                        || entry.path().extension().and_then(|ext| ext.to_str()) != Some("json")
                    {
                        continue;
                    }
                    let metadata = entry.metadata()?;
                    let modified = metadata
                        .modified()
                        .ok()
                        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|duration| duration.as_nanos())
                        .unwrap_or(0);
                    // Atomic replace creates a new inode even when the file
                    // size and a coarse mtime happen to match. It is only an
                    // extra discriminator; the stamp stays opaque per platform.
                    #[cfg(unix)]
                    let file_id = {
                        use std::os::unix::fs::MetadataExt;
                        metadata.ino()
                    };
                    #[cfg(not(unix))]
                    let file_id = 0u64;
                    files.push((
                        entry.file_name().to_string_lossy().into_owned(),
                        metadata.len(),
                        modified,
                        file_id,
                    ));
                }
                files.sort();
                for (name, len, modified, file_id) in files {
                    description.push_str(&name);
                    description.push(':');
                    description.push_str(&len.to_string());
                    description.push(':');
                    description.push_str(&modified.to_string());
                    description.push(':');
                    description.push_str(&file_id.to_string());
                    description.push(';');
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        // Directory mtime catches create/delete/rename even when the file list
        // happens to keep the same aggregate size and no file was rewritten.
        if let Ok(metadata) = fs::metadata(&dir) {
            if let Ok(modified) = metadata.modified() {
                if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                    description.push_str("dir:");
                    description.push_str(&duration.as_nanos().to_string());
                }
            }
        }
        Ok(format!("{:x}", md5::compute(description.as_bytes())))
    }

    pub fn get_named_entry(&self, entry_id: &str) -> crate::Result<Option<NamedEntry>> {
        self.read_record("entries", entry_id)
    }

    pub fn list_text_anchors_for_chapter(
        &self,
        chapter_id: &str,
    ) -> crate::Result<Vec<TextAnchor>> {
        Ok(self
            .read_collection::<TextAnchor>("anchors")?
            .into_iter()
            .filter(|record| record.deleted_at_ms.is_none() && record.chapter_id == chapter_id)
            .collect())
    }

    /// Read all text anchors, including tombstones, for sync-aware projections.
    pub fn list_text_anchor_records(&self) -> crate::Result<Vec<TextAnchor>> {
        self.read_collection("anchors")
    }

    pub fn get_text_anchor(&self, anchor_id: &str) -> crate::Result<Option<TextAnchor>> {
        self.read_record("anchors", anchor_id)
    }

    /// Prepare changed chapter anchors and cascaded tombstones without writing
    /// them. Callers add `files` to the chapter's existing save transaction.
    pub fn prepare_text_anchor_changes(
        &self,
        original_anchors: &[TextAnchor],
        current_anchors: &[TextAnchor],
        pending_bindings: &[TagBinding],
        pending_links: &[ObjectLink],
        chapter_text: &str,
        updated_at_ms: i64,
    ) -> crate::Result<PreparedTextAnchorChanges> {
        let originals = original_anchors
            .iter()
            .map(|anchor| (anchor.anchor_id.as_str(), anchor))
            .collect::<HashMap<_, _>>();
        let text_hash = format!("{:x}", md5::compute(chapter_text.as_bytes()));
        let mut records = HashMap::<String, String>::new();
        let mut committed_anchors = Vec::new();
        let mut deleted_anchor_ids = HashMap::<String, TextAnchor>::new();
        let mut restored_anchor_ids = HashMap::<String, i64>::new();

        for anchor in current_anchors {
            let original = originals.get(anchor.anchor_id.as_str());
            if original.is_none() {
                // A text anchor created during this editor session has no
                // durable record yet. Persist it with the chapter save, unless
                // the selected text was removed before that save.
                if anchor.deleted_at_ms.is_some() {
                    continue;
                }
                let mut committed = anchor.clone();
                committed.updated_at_ms = updated_at_ms;
                crate::annotations::ranges::capture_text_anchor_with_hash(
                    &mut committed,
                    chapter_text,
                    &text_hash,
                )?;
                committed.deleted_at_ms = None;
                let path = self.relative_record_path("anchors", &committed.anchor_id)?;
                records.insert(path, serde_json::to_string_pretty(&committed)?);
                committed_anchors.push(committed);
                continue;
            }
            let Some(original) = original else {
                continue;
            };
            if *original == anchor
                && (anchor.deleted_at_ms.is_some() || anchor.text_hash == text_hash)
            {
                continue;
            }
            let mut committed = anchor.clone();
            committed.updated_at_ms = updated_at_ms;
            let collection = "anchors";
            if committed.deleted_at_ms.is_some() {
                committed.deleted_at_ms = Some(updated_at_ms);
                deleted_anchor_ids.insert(committed.anchor_id.clone(), committed.clone());
            } else {
                if let Some(Some(deleted_at_ms)) = originals
                    .get(committed.anchor_id.as_str())
                    .map(|original| original.deleted_at_ms)
                {
                    restored_anchor_ids.insert(committed.anchor_id.clone(), deleted_at_ms);
                }
                crate::annotations::ranges::capture_text_anchor_with_hash(
                    &mut committed,
                    chapter_text,
                    &text_hash,
                )?;
                committed.deleted_at_ms = None;
            }
            let path = self.relative_record_path(collection, &committed.anchor_id)?;
            records.insert(path, serde_json::to_string_pretty(&committed)?);
            committed_anchors.push(committed);
        }

        if !deleted_anchor_ids.is_empty() {
            for binding in self.list_bindings()? {
                let ObjectRef::TextAnchor { object_id, .. } = &binding.target else {
                    continue;
                };
                if !deleted_anchor_ids.contains_key(object_id) {
                    continue;
                }
                let mut tombstone = binding;
                tombstone.updated_at_ms = updated_at_ms;
                tombstone.deleted_at_ms = Some(updated_at_ms);
                let path = self.relative_record_path("bindings", &tombstone.binding_id)?;
                records.insert(path, serde_json::to_string_pretty(&tombstone)?);
            }
            for link in self.list_links()? {
                let ObjectRef::TextAnchor { object_id, .. } = &link.source else {
                    continue;
                };
                if !deleted_anchor_ids.contains_key(object_id) {
                    continue;
                }
                let mut tombstone = link;
                tombstone.updated_at_ms = updated_at_ms;
                tombstone.deleted_at_ms = Some(updated_at_ms);
                let path = self.relative_record_path("links", &tombstone.link_id)?;
                records.insert(path, serde_json::to_string_pretty(&tombstone)?);
            }
        }

        if !restored_anchor_ids.is_empty() {
            for mut binding in self.list_binding_records()? {
                let ObjectRef::TextAnchor { object_id, .. } = &binding.target else {
                    continue;
                };
                let Some(deleted_at_ms) = restored_anchor_ids.get(object_id) else {
                    continue;
                };
                if binding.deleted_at_ms != Some(*deleted_at_ms) {
                    continue;
                }
                binding.deleted_at_ms = None;
                binding.updated_at_ms = updated_at_ms;
                let path = self.relative_record_path("bindings", &binding.binding_id)?;
                records.insert(path, serde_json::to_string_pretty(&binding)?);
            }
            for mut link in self.list_link_records()? {
                let ObjectRef::TextAnchor { object_id, .. } = &link.source else {
                    continue;
                };
                let Some(deleted_at_ms) = restored_anchor_ids.get(object_id) else {
                    continue;
                };
                if link.deleted_at_ms != Some(*deleted_at_ms) {
                    continue;
                }
                link.deleted_at_ms = None;
                link.updated_at_ms = updated_at_ms;
                let path = self.relative_record_path("links", &link.link_id)?;
                records.insert(path, serde_json::to_string_pretty(&link)?);
            }
        }

        let changed_to_deleted = committed_anchors
            .iter()
            .filter(|anchor| anchor.deleted_at_ms.is_some())
            .map(|anchor| anchor.anchor_id.as_str())
            .collect::<HashSet<_>>();
        let chapter_ids = original_anchors
            .iter()
            .chain(current_anchors)
            .filter(|anchor| anchor.project_id == self.project_id)
            .map(|anchor| anchor.chapter_id.as_str())
            .collect::<HashSet<_>>();
        let mut active_anchor_ids = current_anchors
            .iter()
            .filter(|anchor| anchor.project_id == self.project_id && anchor.deleted_at_ms.is_none())
            .map(|anchor| anchor.anchor_id.clone())
            .collect::<HashSet<_>>();
        for chapter_id in chapter_ids {
            for anchor in self.list_text_anchors_for_chapter(chapter_id)? {
                if anchor.deleted_at_ms.is_none()
                    && !changed_to_deleted.contains(anchor.anchor_id.as_str())
                {
                    active_anchor_ids.insert(anchor.anchor_id);
                }
            }
        }
        for binding in pending_bindings {
            let ObjectRef::TextAnchor {
                project_id,
                object_id,
            } = &binding.target
            else {
                continue;
            };
            if binding.project_id != self.project_id
                || project_id != &self.project_id
                || !active_anchor_ids.contains(object_id)
            {
                continue;
            }
            let path = self.relative_record_path("bindings", &binding.binding_id)?;
            records.insert(path, serde_json::to_string_pretty(binding)?);
        }
        for link in pending_links {
            let ObjectRef::TextAnchor {
                project_id,
                object_id,
            } = &link.source
            else {
                continue;
            };
            if project_id != &self.project_id || !active_anchor_ids.contains(object_id) {
                continue;
            }
            let path = self.relative_record_path("links", &link.link_id)?;
            records.insert(path, serde_json::to_string_pretty(link)?);
        }

        let mut files = records.into_iter().collect::<Vec<_>>();
        files.sort_by(|left, right| left.0.cmp(&right.0));
        committed_anchors.sort_by(|left, right| left.anchor_id.cmp(&right.anchor_id));
        Ok(PreparedTextAnchorChanges {
            files,
            committed_anchors,
        })
    }

    pub fn list_links(&self) -> crate::Result<Vec<ObjectLink>> {
        Ok(self
            .read_collection::<ObjectLink>("links")?
            .into_iter()
            .filter(|record| record.deleted_at_ms.is_none())
            .collect())
    }

    /// Read all links, including tombstones, for sync-aware projections.
    pub fn list_link_records(&self) -> crate::Result<Vec<ObjectLink>> {
        self.read_collection("links")
    }

    /// Directory that holds this store's link records.
    pub fn links_dir(&self) -> PathBuf {
        self.project_root.join("annotations").join("links")
    }

    /// Lightweight version fingerprint of the link collection: file name,
    /// modification time in nanoseconds, and file length.
    ///
    /// Canvas link projections use this to notice writes made by another
    /// process (MCP, sync, a second instance) without re-reading every record
    /// on each access. A missing directory is an empty fingerprint.
    pub fn links_fingerprint(&self) -> crate::Result<Vec<(String, u64, u64)>> {
        let dir = self.links_dir();
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut fingerprint = Vec::new();
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_file()
                || entry.path().extension().and_then(|ext| ext.to_str()) != Some("json")
            {
                continue;
            }
            let metadata = entry.metadata()?;
            let modified_nanos = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_nanos().min(u128::from(u64::MAX)) as u64)
                .unwrap_or(0);
            fingerprint.push((
                entry.file_name().to_string_lossy().to_string(),
                modified_nanos,
                metadata.len(),
            ));
        }
        fingerprint.sort();
        Ok(fingerprint)
    }

    pub fn list_links_for_source(&self, source: &ObjectRef) -> crate::Result<Vec<ObjectLink>> {
        Ok(self
            .list_links()?
            .into_iter()
            .filter(|link| &link.source == source)
            .collect())
    }

    pub fn get_link(&self, link_id: &str) -> crate::Result<Option<ObjectLink>> {
        self.read_record("links", link_id)
    }

    /// Active links whose source belongs to one canvas or whose canvas-navigation
    /// target starts from that canvas. This is the starmap graph view's read-only
    /// projection; the links themselves stay owned by the annotation store.
    pub fn list_links_for_starmap(&self, starmap_id: &str) -> crate::Result<Vec<ObjectLink>> {
        Ok(self
            .list_links()?
            .into_iter()
            .filter(|link| {
                link.source.starmap_id() == Some(starmap_id)
                    || matches!(
                        &link.target,
                        LinkTarget::CanvasNavigation(path) if path.starmap_id == starmap_id
                    )
            })
            .collect())
    }

    /// Find the link for one source/target pair, including tombstones.
    ///
    /// `(source, target)` is the idempotency key: only the source's owning store
    /// keeps the link, never the destination's.
    pub fn find_link_by_idempotency_key(
        &self,
        source: &ObjectRef,
        target: &LinkTarget,
    ) -> crate::Result<Option<ObjectLink>> {
        let mut tombstone = None;
        for link in self.list_link_records()? {
            if &link.source != source || &link.target != target {
                continue;
            }
            if link.deleted_at_ms.is_none() {
                return Ok(Some(link));
            }
            tombstone.get_or_insert(link);
        }
        Ok(tombstone)
    }

    /// Idempotently add one navigation link.
    ///
    /// An existing active `(source, target)` relation is returned unchanged. A
    /// tombstoned relation is revived with its original `link_id`. An unrelated
    /// record that already uses the requested ID is never overwritten.
    pub fn attach_link(&self, link: ObjectLink) -> crate::Result<(ObjectLink, Vec<String>)> {
        self.validate_link_record(&link)?;
        if let Some(existing) = self.find_link_by_idempotency_key(&link.source, &link.target)? {
            if existing.deleted_at_ms.is_none() {
                return Ok((existing, Vec::new()));
            }
            let mut revived = link;
            revived.link_id = existing.link_id;
            revived.deleted_at_ms = None;
            revived.updated_at_ms = revived
                .updated_at_ms
                .max(existing.updated_at_ms.saturating_add(1));
            let changed = self.apply_mutation(AnnotationMutation::UpsertLink(revived.clone()))?;
            return Ok((revived, changed));
        }
        if let Some(existing) = self.get_link(&link.link_id)? {
            if existing.source != link.source || existing.target != link.target {
                return Err(invalid_annotation(
                    "link ID is already used by another link",
                ));
            }
            if existing.deleted_at_ms.is_none() {
                return Ok((existing, Vec::new()));
            }
            let mut revived = link;
            revived.deleted_at_ms = None;
            revived.updated_at_ms = revived
                .updated_at_ms
                .max(existing.updated_at_ms.saturating_add(1));
            let changed = self.apply_mutation(AnnotationMutation::UpsertLink(revived.clone()))?;
            return Ok((revived, changed));
        }
        let changed = self.apply_mutation(AnnotationMutation::UpsertLink(link.clone()))?;
        Ok((link, changed))
    }

    /// Idempotently remove one navigation link by its idempotency key.
    pub fn detach_link(
        &self,
        source: &ObjectRef,
        target: &LinkTarget,
        now_ms: i64,
    ) -> crate::Result<Vec<String>> {
        let Some(mut link) = self.find_link_by_idempotency_key(source, target)? else {
            return Ok(Vec::new());
        };
        if link.deleted_at_ms.is_some() {
            return Ok(Vec::new());
        }
        link.updated_at_ms = now_ms.max(link.updated_at_ms.saturating_add(1));
        link.deleted_at_ms = Some(now_ms);
        self.apply_mutation(AnnotationMutation::UpsertLink(link))
    }

    /// Apply one record mutation and return every changed project-relative path.
    /// Cascaded tombstones for deleted tags and deleted text anchors are included.
    ///
    /// Upserts compare against the record already on disk and skip the write when
    /// nothing changed, so repeated idempotent requests do not rewrite files.
    pub fn apply_mutation(&self, mutation: AnnotationMutation) -> crate::Result<Vec<String>> {
        match mutation {
            AnnotationMutation::UpsertTag(record) => {
                self.validate_project(&record.project_id)?;
                Ok(self
                    .write_record_if_changed("tags", &record.tag_id, &record)?
                    .into_iter()
                    .collect())
            }
            AnnotationMutation::UpsertBinding(record) => {
                self.validate_project(&record.project_id)?;
                Ok(self
                    .write_record_if_changed("bindings", &record.binding_id, &record)?
                    .into_iter()
                    .collect())
            }
            AnnotationMutation::UpsertNamedEntry(record) => {
                self.validate_project(&record.project_id)?;
                Ok(self
                    .write_record_if_changed("entries", &record.entry_id, &record)?
                    .into_iter()
                    .collect())
            }
            AnnotationMutation::UpsertTextAnchor(record) => {
                self.validate_project(&record.project_id)?;
                if record.start_byte > record.end_byte {
                    return Err(invalid_annotation("text anchor start exceeds end"));
                }
                Ok(self
                    .write_record_if_changed("anchors", &record.anchor_id, &record)?
                    .into_iter()
                    .collect())
            }
            AnnotationMutation::UpsertLink(record) => {
                self.validate_link_record(&record)?;
                Ok(self
                    .write_record_if_changed("links", &record.link_id, &record)?
                    .into_iter()
                    .collect())
            }
            AnnotationMutation::Delete {
                kind,
                scope,
                object_id,
                deleted_at_ms,
            } => {
                if scope != self.scope() {
                    return Err(invalid_annotation(
                        "annotation delete targets another namespace",
                    ));
                }
                self.delete_record(kind, &object_id, deleted_at_ms)
            }
        }
    }

    /// Rename a tag without changing its stable ID or any binding IDs.
    pub fn rename_tag(
        &self,
        tag_id: &str,
        name: String,
        updated_at_ms: i64,
    ) -> crate::Result<Vec<String>> {
        let Some(mut tag) = self.get_tag(tag_id)? else {
            return Ok(Vec::new());
        };
        if tag.deleted_at_ms.is_some() {
            return Ok(Vec::new());
        }
        tag.name = name;
        tag.updated_at_ms = updated_at_ms;
        self.apply_mutation(AnnotationMutation::UpsertTag(tag))
    }

    /// Merge all source-tag bindings into the target tag, then tombstone the
    /// source. A duplicate target binding is retained and the redundant source
    /// binding is tombstoned, preserving stable IDs for surviving records.
    pub fn merge_tags(
        &self,
        source_tag_id: &str,
        target_tag_id: &str,
        updated_at_ms: i64,
    ) -> crate::Result<Vec<String>> {
        if source_tag_id == target_tag_id {
            return Ok(Vec::new());
        }
        let source = self.get_tag(source_tag_id)?;
        let target = self.get_tag(target_tag_id)?;
        let (Some(source), Some(target)) = (source, target) else {
            return Err(invalid_annotation("cannot merge a missing tag"));
        };
        if source.deleted_at_ms.is_some() || target.deleted_at_ms.is_some() {
            return Err(invalid_annotation("cannot merge a deleted tag"));
        }

        let target_bindings = self.list_bindings_for_tag(target_tag_id)?;
        let mut changed = Vec::new();
        for mut binding in self.list_bindings_for_tag(source_tag_id)? {
            if target_bindings
                .iter()
                .any(|existing| existing.target == binding.target)
            {
                changed.extend(self.apply_mutation(AnnotationMutation::Delete {
                    kind: AnnotationKind::Binding,
                    scope: self.scope(),
                    object_id: binding.binding_id,
                    deleted_at_ms: updated_at_ms,
                })?);
            } else {
                binding.tag_id = target_tag_id.to_string();
                binding.updated_at_ms = updated_at_ms;
                changed.extend(self.apply_mutation(AnnotationMutation::UpsertBinding(binding))?);
            }
        }
        changed.extend(self.apply_mutation(AnnotationMutation::Delete {
            kind: AnnotationKind::Tag,
            scope: self.scope(),
            object_id: source.tag_id,
            deleted_at_ms: updated_at_ms,
        })?);
        Ok(changed)
    }

    /// Add the 14 ordinary, flat preset tags to a newly-created project.
    /// This is called only from the project creation path, never on reads.
    pub fn initialize_default_tags(&self) -> crate::Result<Vec<String>> {
        let updated_at_ms = chrono::Utc::now().timestamp_millis();
        let mut changed = Vec::with_capacity(DEFAULT_TAG_NAMES.len());
        for name in DEFAULT_TAG_NAMES {
            let tag = Tag {
                tag_id: uuid::Uuid::new_v4().to_string(),
                project_id: self.project_id.clone(),
                name: name.to_string(),
                color: None,
                updated_at_ms,
                deleted_at_ms: None,
            };
            changed.extend(self.apply_mutation(AnnotationMutation::UpsertTag(tag))?);
        }
        Ok(changed)
    }

    fn delete_record(
        &self,
        kind: AnnotationKind,
        object_id: &str,
        deleted_at_ms: i64,
    ) -> crate::Result<Vec<String>> {
        let mut changed = Vec::new();
        match kind {
            AnnotationKind::Tag => {
                let Some(mut tag) = self.get_tag(object_id)? else {
                    return Ok(changed);
                };
                if tag.deleted_at_ms.is_some() {
                    return Ok(changed);
                }
                for binding in self.list_bindings_for_tag(object_id)? {
                    changed.extend(self.delete_record(
                        AnnotationKind::Binding,
                        &binding.binding_id,
                        deleted_at_ms,
                    )?);
                }
                tag.updated_at_ms = deleted_at_ms;
                tag.deleted_at_ms = Some(deleted_at_ms);
                changed.push(self.write_record("tags", &tag.tag_id, &tag)?);
            }
            AnnotationKind::Binding => {
                let Some(mut record) = self.get_record::<TagBinding>("bindings", object_id)? else {
                    return Ok(changed);
                };
                if record.deleted_at_ms.is_none() {
                    record.updated_at_ms = deleted_at_ms;
                    record.deleted_at_ms = Some(deleted_at_ms);
                    changed.push(self.write_record("bindings", &record.binding_id, &record)?);
                }
            }
            AnnotationKind::NamedEntry => {
                let Some(mut record) = self.get_named_entry(object_id)? else {
                    return Ok(changed);
                };
                if record.deleted_at_ms.is_none() {
                    record.updated_at_ms = deleted_at_ms;
                    record.deleted_at_ms = Some(deleted_at_ms);
                    changed.push(self.write_record("entries", &record.entry_id, &record)?);
                }
            }
            AnnotationKind::TextAnchor => {
                let Some(mut record) = self.get_text_anchor(object_id)? else {
                    return Ok(changed);
                };
                if record.deleted_at_ms.is_some() {
                    return Ok(changed);
                }
                let source = record.object_ref();
                for binding in self.list_bindings_for_target(&source)? {
                    changed.extend(self.delete_record(
                        AnnotationKind::Binding,
                        &binding.binding_id,
                        deleted_at_ms,
                    )?);
                }
                for link in self.list_links_for_source(&source)? {
                    changed.extend(self.delete_record(
                        AnnotationKind::Link,
                        &link.link_id,
                        deleted_at_ms,
                    )?);
                }
                record.updated_at_ms = deleted_at_ms;
                record.deleted_at_ms = Some(deleted_at_ms);
                changed.push(self.write_record("anchors", &record.anchor_id, &record)?);
            }
            AnnotationKind::Link => {
                let Some(mut record) = self.get_link(object_id)? else {
                    return Ok(changed);
                };
                if record.deleted_at_ms.is_none() {
                    record.updated_at_ms = deleted_at_ms;
                    record.deleted_at_ms = Some(deleted_at_ms);
                    changed.push(self.write_record("links", &record.link_id, &record)?);
                }
            }
            AnnotationKind::Project
            | AnnotationKind::Volume
            | AnnotationKind::Chapter
            | AnnotationKind::StarMap
            | AnnotationKind::NodeAsset
            | AnnotationKind::NodeInstance
            | AnnotationKind::EmbedInstance
            | AnnotationKind::CanvasAnchor => {
                return Err(invalid_annotation(
                    "cannot delete an object reference as an annotation record",
                ));
            }
        }
        Ok(changed)
    }

    /// A project store only accepts records of its own project; the global store
    /// only accepts global records. Project records must never be written into
    /// the global directory.
    fn validate_project(&self, project_id: &str) -> crate::Result<()> {
        if self.project_id.is_empty() {
            if project_id.is_empty() {
                Ok(())
            } else {
                Err(invalid_annotation(
                    "project annotation record cannot be stored in the global domain",
                ))
            }
        } else if project_id == self.project_id {
            Ok(())
        } else {
            Err(invalid_annotation(
                "annotation record belongs to another project",
            ))
        }
    }

    /// A link is stored once, under its source's ownership.
    ///
    /// Project-scoped sources must be written into their own project store.
    /// Canvas instances and global shared assets carry no project in their ID;
    /// whether their canvas belongs to a project is decided by the caller, which
    /// also chooses the matching store, so the store only rejects foreign
    /// project-owned sources here.
    fn validate_link_record(&self, link: &ObjectLink) -> crate::Result<()> {
        if let Some(source_project) = link.source.project_id() {
            if source_project != self.project_id {
                return Err(invalid_annotation(
                    "link source belongs to another namespace",
                ));
            }
        }
        if !self.is_global() {
            // 全局素材本身没有作品归属，链接必须留在全局域，否则换一个作品
            // 打开同一素材就看不到这条链接。
            if matches!(
                &link.source,
                ObjectRef::NodeAsset {
                    scope: ObjectScope::Global,
                    ..
                } | ObjectRef::StarMap {
                    scope: ObjectScope::Global,
                    ..
                }
            ) {
                return Err(invalid_annotation(
                    "global shared-asset links must be stored in the global domain",
                ));
            }
        }
        Ok(())
    }

    fn get_record<T: DeserializeOwned>(
        &self,
        collection: &str,
        id: &str,
    ) -> crate::Result<Option<T>> {
        let path = self.record_path(collection, id)?;
        match fs::read_to_string(path) {
            Ok(content) => {
                // 旧格式外链不是有效的对象链接记录：读取结果排除，文件删除只由
                // `migrate_legacy_external_link_records` 一次性执行。
                if collection == "links" && is_legacy_external_link_record(&content) {
                    return Ok(None);
                }
                Ok(Some(serde_json::from_str(&content)?))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn read_record<T: DeserializeOwned>(
        &self,
        collection: &str,
        id: &str,
    ) -> crate::Result<Option<T>> {
        self.get_record(collection, id)
    }

    fn read_collection<T: DeserializeOwned>(&self, collection: &str) -> crate::Result<Vec<T>> {
        let dir = self.collection_path(collection)?;
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut paths = Vec::new();
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_file()
                || entry.path().extension().and_then(|ext| ext.to_str()) != Some("json")
            {
                continue;
            }
            paths.push(entry.path());
        }
        paths.sort();

        let mut records = Vec::with_capacity(paths.len());
        for path in paths {
            let content = fs::read_to_string(&path)?;
            // 普通读取只读：旧格式外链被识别时从结果排除，不在读取期间删文件。
            if collection == "links" && is_legacy_external_link_record(&content) {
                continue;
            }
            records.push(serde_json::from_str(&content)?);
        }
        Ok(records)
    }

    /// 旧格式外链的一次性迁移入口。
    ///
    /// External-URL annotation links were removed with the external-binding
    /// feature. Older project data stored them as
    /// `{"target":{"kind":"externalUrl","value":"..."}}`; those files are not
    /// part of the object-only link model. This is the only place that deletes
    /// them: it returns the workspace-relative paths that were actually
    /// removed, so the caller can record the deletion fact. Normal reads
    /// ([`Self::read_collection`] / [`Self::get_link`]) only exclude such
    /// records and never delete files. Internal object links are untouched.
    pub fn migrate_legacy_external_link_records(&self) -> crate::Result<Vec<String>> {
        let dir = self.collection_path("links")?;
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut removed = Vec::new();
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_file()
                || entry.path().extension().and_then(|ext| ext.to_str()) != Some("json")
            {
                continue;
            }
            let path = entry.path();
            let content = fs::read_to_string(&path)?;
            if !is_legacy_external_link_record(&content) {
                continue;
            }
            fs::remove_file(&path)?;
            removed.push(
                path.strip_prefix(&self.project_root)
                    .map_err(|_| invalid_annotation("annotation path escaped its project"))?
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
        removed.sort();
        Ok(removed)
    }

    fn write_record<T: Serialize>(
        &self,
        collection: &str,
        id: &str,
        record: &T,
    ) -> crate::Result<String> {
        let path = self.record_path(collection, id)?;
        let content = serde_json::to_string_pretty(record)?;
        crate::storage::atomic_write_string(&path, &content)?;
        Ok(path
            .strip_prefix(&self.project_root)
            .map_err(|_| invalid_annotation("annotation path escaped its project"))?
            .to_string_lossy()
            .replace('\\', "/"))
    }

    /// Write an upsert only when it changes the record already on disk.
    fn write_record_if_changed<T>(
        &self,
        collection: &str,
        id: &str,
        record: &T,
    ) -> crate::Result<Option<String>>
    where
        T: Serialize + DeserializeOwned + PartialEq,
    {
        if let Some(existing) = self.get_record::<T>(collection, id)? {
            if &existing == record {
                return Ok(None);
            }
        }
        Ok(Some(self.write_record(collection, id, record)?))
    }

    fn collection_path(&self, collection: &str) -> crate::Result<PathBuf> {
        if !COLLECTIONS.contains(&collection) {
            return Err(invalid_annotation("unknown annotation collection"));
        }
        Ok(self.project_root.join("annotations").join(collection))
    }

    fn record_path(&self, collection: &str, id: &str) -> crate::Result<PathBuf> {
        if id.is_empty()
            || id == "."
            || id == ".."
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(invalid_annotation(
                "annotation ID is not a safe path component",
            ));
        }
        Ok(self.collection_path(collection)?.join(format!("{id}.json")))
    }

    fn relative_record_path(&self, collection: &str, id: &str) -> crate::Result<String> {
        let path = self.record_path(collection, id)?;
        Ok(path
            .strip_prefix(&self.project_root)
            .map_err(|_| invalid_annotation("annotation path escaped its project"))?
            .to_string_lossy()
            .replace('\\', "/"))
    }
}

/// Find local backlinks in projects that have a readable, valid `project.json`.
///
/// The global annotation store is scanned as well: links owned by a canvas
/// without a project (or by a global shared asset) live there, while their
/// target may still be a project object. Projects that are not downloaded have
/// no directory to scan and therefore do not produce guessed backlinks.
pub fn find_backlinks(
    app_data_root: &Path,
    projects_root: &Path,
    target: &LinkTarget,
) -> crate::Result<Vec<ObjectLink>> {
    let entries = match fs::read_dir(projects_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut projects = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let project_root = entry.path();
        let project_file = project_root.join("project.json");
        let project = match fs::read_to_string(&project_file) {
            Ok(content) => serde_json::from_str::<crate::project::Project>(&content)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => continue,
            Err(error) => return Err(error.into()),
        };
        if project.id != entry.file_name().to_string_lossy().as_ref() {
            continue;
        }
        projects.push((project_root, project.id));
    }
    projects.sort_by(|left, right| left.1.cmp(&right.1));

    let mut backlinks = Vec::new();
    for (project_root, project_id) in projects {
        let store = AnnotationStore::new(&project_root, &project_id);
        for link in store.list_links()? {
            if &link.target == target {
                backlinks.push(link);
            }
        }
    }
    for link in AnnotationStore::global(app_data_root).list_links()? {
        if &link.target == target {
            backlinks.push(link);
        }
    }
    Ok(backlinks)
}

/// The exact unique relation key shared with [`TagBinding::relation_key`].
fn relation_key(project_id: &str, tag_id: &str, target: &ObjectRef) -> String {
    format!("{project_id}|{tag_id}|{}", target.stable_key())
}

/// FNV-1a 64-bit hash used for deterministic binding IDs.
///
/// Implemented locally to avoid a new dependency; the only requirement is that
/// every device computes the same value for the same relation key.
fn fnv1a64(value: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// 旧格式外链记录（external-binding 功能移除前写入的链接）。
///
/// 普通读取用它把旧链接从结果里排除；只有迁移入口会实际删除文件。
fn is_legacy_external_link_record(content: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(content) else {
        return false;
    };
    let Some(target) = value.get("target") else {
        return false;
    };
    target.get("kind").and_then(serde_json::Value::as_str) == Some("externalUrl")
        || target
            .get("externalUrl")
            .is_some_and(|url| !url.is_null())
}

fn invalid_annotation(message: &str) -> crate::error::Error {
    crate::error::Error::Other(message.to_string())
}
