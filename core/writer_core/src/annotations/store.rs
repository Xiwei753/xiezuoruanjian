use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{de::DeserializeOwned, Serialize};

use super::model::{
    AnnotationKind, AnnotationMutation, LinkTarget, NamedEntry, ObjectLink, ObjectRef, Tag,
    TagBinding, TextAnchor, DEFAULT_TAG_NAMES,
};

const COLLECTIONS: [&str; 5] = ["tags", "bindings", "entries", "anchors", "links"];

#[derive(Debug, Clone, Default)]
pub struct PreparedTextAnchorChanges {
    /// Project-relative JSON files staged with the chapter body save.
    pub files: Vec<(String, String)>,
    /// Anchor records as they will be stored after the transaction commits.
    pub committed_anchors: Vec<TextAnchor>,
}

/// Project-local annotation persistence.
///
/// Each record has its own JSON file and is written through the shared atomic
/// writer. The directory scan is the rebuildable index; no second persistent
/// index can overwrite records created on another device.
#[derive(Debug, Clone)]
pub struct AnnotationStore {
    project_root: PathBuf,
    project_id: String,
}

impl AnnotationStore {
    pub fn new(project_root: &Path, project_id: &str) -> Self {
        Self {
            project_root: project_root.to_path_buf(),
            project_id: project_id.to_string(),
        }
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
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

    /// Apply one record mutation and return every changed project-relative path.
    /// Cascaded tombstones for deleted tags and deleted text anchors are included.
    pub fn apply_mutation(&self, mutation: AnnotationMutation) -> crate::Result<Vec<String>> {
        match mutation {
            AnnotationMutation::UpsertTag(record) => {
                self.validate_project(&record.project_id)?;
                Ok(vec![self.write_record("tags", &record.tag_id, &record)?])
            }
            AnnotationMutation::UpsertBinding(record) => {
                self.validate_project(&record.project_id)?;
                Ok(vec![self.write_record(
                    "bindings",
                    &record.binding_id,
                    &record,
                )?])
            }
            AnnotationMutation::UpsertNamedEntry(record) => {
                self.validate_project(&record.project_id)?;
                Ok(vec![self.write_record(
                    "entries",
                    &record.entry_id,
                    &record,
                )?])
            }
            AnnotationMutation::UpsertTextAnchor(record) => {
                self.validate_project(&record.project_id)?;
                if record.start_byte > record.end_byte {
                    return Err(invalid_annotation("text anchor start exceeds end"));
                }
                Ok(vec![self.write_record(
                    "anchors",
                    &record.anchor_id,
                    &record,
                )?])
            }
            AnnotationMutation::UpsertLink(record) => {
                self.validate_project(record.source.project_id())?;
                Ok(vec![self.write_record(
                    "links",
                    &record.link_id,
                    &record,
                )?])
            }
            AnnotationMutation::Delete {
                kind,
                project_id,
                object_id,
                deleted_at_ms,
            } => {
                self.validate_project(&project_id)?;
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
                    project_id: self.project_id.clone(),
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
            project_id: self.project_id.clone(),
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
            | AnnotationKind::StarMapNode
            | AnnotationKind::StarMapEmbed => {
                return Err(invalid_annotation(
                    "cannot delete an object reference as an annotation record",
                ));
            }
        }
        Ok(changed)
    }

    fn validate_project(&self, project_id: &str) -> crate::Result<()> {
        if project_id == self.project_id {
            Ok(())
        } else {
            Err(invalid_annotation(
                "annotation record belongs to another project",
            ))
        }
    }

    fn get_record<T: DeserializeOwned>(
        &self,
        collection: &str,
        id: &str,
    ) -> crate::Result<Option<T>> {
        let path = self.record_path(collection, id)?;
        match fs::read_to_string(path) {
            Ok(content) => Ok(Some(serde_json::from_str(&content)?)),
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

        if collection == "links" {
            self.remove_legacy_external_link_records(&paths)?;
        }

        let mut records = Vec::with_capacity(paths.len());
        for path in paths {
            let content = fs::read_to_string(path)?;
            records.push(serde_json::from_str(&content)?);
        }
        Ok(records)
    }

    /// External-URL annotation links were removed with the external-binding
    /// feature. Older project data stored them as
    /// `{"target":{"kind":"externalUrl","value":"..."}}`; remove those records
    /// before deserializing the object-only link model. Internal object links
    /// are left untouched.
    fn remove_legacy_external_link_records(&self, paths: &[PathBuf]) -> crate::Result<()> {
        for path in paths {
            let content = fs::read_to_string(path)?;
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
                continue;
            };
            let target = value.get("target");
            let is_external = target
                .and_then(|target| target.get("kind"))
                .and_then(|kind| kind.as_str())
                == Some("externalUrl")
                || target
                    .and_then(|target| target.get("externalUrl"))
                    .is_some_and(|url| !url.is_null());
            if is_external {
                fs::remove_file(path)?;
            }
        }
        Ok(())
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
/// Projects that are not downloaded have no directory to scan and therefore do
/// not produce name-based or guessed backlinks.
pub fn find_backlinks(projects_root: &Path, target: &ObjectRef) -> crate::Result<Vec<ObjectLink>> {
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
            if matches!(&link.target, LinkTarget::Object(link_target) if link_target == target) {
                backlinks.push(link);
            }
        }
    }
    Ok(backlinks)
}

fn invalid_annotation(message: &str) -> crate::error::Error {
    crate::error::Error::Other(message.to_string())
}
