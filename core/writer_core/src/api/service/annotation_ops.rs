use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::annotations::ranges::{
    capture_text_anchor, capture_text_anchor_with_hash, map_text_anchor_after_edits,
    rebase_text_anchor_with_hash, resulting_text_len, TextAnchorEdit, TextEditRange,
};
use crate::annotations::{
    find_backlinks as find_annotation_backlinks, AnnotationKind, AnnotationMutation,
    AnnotationStore, LinkOrigin, LinkTarget, NamedEntry, ObjectLink, ObjectRef, Tag, TagBinding,
    TextAnchor,
};
use crate::api::service::{ApiResult, WriterCoreApi};
use crate::api::types::*;
use crate::api::WriterError;
use crate::object_ref::ObjectScope;

fn invalid_annotation(message: impl Into<String>) -> WriterError {
    WriterError::Other(message.into())
}

fn safe_project_id(project_id: &str) -> ApiResult<()> {
    if project_id.is_empty()
        || !project_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(invalid_annotation(
            "project id is not a safe path component",
        ));
    }
    Ok(())
}

fn object_ref_from_dto(value: AnnotationObjectRefDto) -> ApiResult<ObjectRef> {
    ObjectRef::try_from(value).map_err(|error| invalid_annotation(error.to_string()))
}

fn object_ref_to_dto(value: ObjectRef) -> AnnotationObjectRefDto {
    value.into()
}

fn link_target_from_dto(value: AnnotationTargetDto) -> ApiResult<LinkTarget> {
    LinkTarget::try_from(value).map_err(|error| invalid_annotation(error.to_string()))
}

fn link_target_to_dto(value: LinkTarget) -> AnnotationTargetDto {
    value.into()
}

fn tag_to_dto(value: Tag) -> AnnotationTagDto {
    AnnotationTagDto {
        tag_id: value.tag_id,
        project_id: value.project_id,
        name: value.name,
        color: value.color,
        updated_at_ms: value.updated_at_ms,
        deleted_at_ms: value.deleted_at_ms,
    }
}

fn binding_to_dto(value: TagBinding) -> AnnotationTagBindingDto {
    AnnotationTagBindingDto {
        binding_id: value.binding_id,
        project_id: value.project_id,
        tag_id: value.tag_id,
        target: object_ref_to_dto(value.target),
        updated_at_ms: value.updated_at_ms,
        deleted_at_ms: value.deleted_at_ms,
    }
}

fn anchor_to_dto(value: TextAnchor) -> ApiResult<AnnotationTextAnchorDto> {
    Ok(AnnotationTextAnchorDto {
        anchor_id: value.anchor_id,
        project_id: value.project_id,
        chapter_id: value.chapter_id,
        start_byte: u32::try_from(value.start_byte)
            .map_err(|error| invalid_annotation(error.to_string()))?,
        end_byte: u32::try_from(value.end_byte)
            .map_err(|error| invalid_annotation(error.to_string()))?,
        updated_at_ms: value.updated_at_ms,
        deleted_at_ms: value.deleted_at_ms,
    })
}

fn named_entry_to_dto(value: NamedEntry) -> AnnotationNamedEntryDto {
    AnnotationNamedEntryDto {
        entry_id: value.entry_id,
        project_id: value.project_id,
        canonical_name: value.canonical_name,
        aliases: value.aliases,
        confirmed_misspellings: value.confirmed_misspellings,
        auto_link_enabled: value.auto_link_enabled,
        updated_at_ms: value.updated_at_ms,
        deleted_at_ms: value.deleted_at_ms,
    }
}

fn link_to_dto(value: ObjectLink) -> AnnotationObjectLinkDto {
    AnnotationObjectLinkDto {
        link_id: value.link_id,
        source: object_ref_to_dto(value.source),
        target: link_target_to_dto(value.target),
        origin: match value.origin {
            LinkOrigin::Manual => AnnotationLinkOriginDto::Manual,
        },
        label: value.label,
        created_at_ms: value.created_at_ms,
        updated_at_ms: value.updated_at_ms,
        deleted_at_ms: value.deleted_at_ms,
    }
}

impl WriterCoreApi {
    pub(crate) fn annotation_store(&self, project_id: &str) -> ApiResult<AnnotationStore> {
        safe_project_id(project_id)?;
        Ok(AnnotationStore::new(
            &self.projects_root.join(project_id),
            project_id,
        ))
    }

    /// Global shared-asset annotation store (`<app_data_root>/annotations/...`).
    pub(crate) fn global_annotation_store(&self) -> AnnotationStore {
        AnnotationStore::global(&self.app_data_root)
    }

    /// Namespace store for tag records: empty namespace means the global domain.
    fn tag_namespace_store(&self, project_id: &str) -> ApiResult<AnnotationStore> {
        if project_id.is_empty() {
            Ok(self.global_annotation_store())
        } else {
            self.annotation_store(project_id)
        }
    }

    /// Record changed paths for a namespace store.
    ///
    /// Project stores return project-relative paths and also refresh the project
    /// search index; the global store returns workspace-relative paths and has no
    /// project index to refresh.
    fn finish_annotation_store_write(
        &self,
        project_id: &str,
        changed_paths: Vec<String>,
        operation: &str,
    ) {
        if project_id.is_empty() {
            if changed_paths.is_empty() {
                return;
            }
            let workspace_paths = changed_paths
                .into_iter()
                .map(PathBuf::from)
                .collect::<Vec<_>>();
            self.record_workspace_paths_history(&workspace_paths, operation);
            return;
        }
        self.finish_annotation_write(project_id, changed_paths, operation);
    }

    /// 项目标签只能绑定本项目作用域内的对象，或该项目有权访问的共享素材。
    ///
    /// 共享素材/画布实例的 `ObjectRef::scope()` 是全局作用域，但它们的归属
    /// 由库索引或星图 meta 决定：本项目的画布、归属本项目或全局库的素材可以
    /// 绑定；归属其他项目或无法判定时拒绝，不猜测。
    fn authorize_tag_binding_target(&self, project_id: &str, target: &ObjectRef) -> ApiResult<()> {
        if project_id.is_empty() {
            return match target.scope() {
                ObjectScope::Global => {
                    self.reject_project_owned_shared_asset(target)?;
                    Ok(())
                }
                ObjectScope::Project { .. } => Err(invalid_annotation(
                    "global tags can only be bound to global shared assets or canvases",
                )),
            };
        }
        match target.scope() {
            ObjectScope::Project {
                project_id: target_project,
            } => {
                if &target_project != project_id {
                    return Err(invalid_annotation(
                        "project tag can only be bound to objects of its own project",
                    ));
                }
                Ok(())
            }
            ObjectScope::Global => self.authorize_global_target_for_project(project_id, target),
        }
    }

    fn authorize_global_target_for_project(
        &self,
        project_id: &str,
        target: &ObjectRef,
    ) -> ApiResult<()> {
        match target {
            ObjectRef::NodeAsset { object_id, .. } => {
                self.authorize_shared_node_asset(project_id, object_id)
            }
            ObjectRef::StarMap { object_id, .. } => {
                let meta = self.get_starmap(object_id)?;
                match meta.project_id.as_deref() {
                    Some(owner) if owner == project_id => Ok(()),
                    Some(_) => Err(invalid_annotation("starmap belongs to another project")),
                    // 独立星图可被任意作品引用。
                    None => Ok(()),
                }
            }
            ObjectRef::NodeInstance { starmap_id, .. }
            | ObjectRef::EmbedInstance { starmap_id, .. }
            | ObjectRef::CanvasAnchor { starmap_id, .. } => {
                let meta = self.get_starmap(starmap_id)?;
                if meta.project_id.as_deref() == Some(project_id) {
                    Ok(())
                } else {
                    Err(invalid_annotation(
                        "canvas instance does not belong to this project",
                    ))
                }
            }
            _ => Err(invalid_annotation("unsupported tag binding target")),
        }
    }

    /// A shared node asset is accessible when its library entry belongs to this
    /// project or to the global library. Without an entry ownership is unknown,
    /// so the binding is rejected instead of guessed.
    fn authorize_shared_node_asset(&self, project_id: &str, asset_id: &str) -> ApiResult<()> {
        let index = crate::starmap::library::load_library_index(&self.app_data_root)?;
        let mut has_global_entry = false;
        let mut has_own_entry = false;
        let mut has_foreign_entry = false;
        for entry in &index.entries {
            if entry.target.kind != crate::starmap::types::StarMapAssetKind::Node
                || entry.target.asset_id != asset_id
            {
                continue;
            }
            match entry.scope.project_id() {
                None => has_global_entry = true,
                Some(owner) if owner == project_id => has_own_entry = true,
                Some(_) => has_foreign_entry = true,
            }
        }
        if has_own_entry || has_global_entry {
            return Ok(());
        }
        if has_foreign_entry {
            return Err(invalid_annotation(
                "shared node asset belongs to another project",
            ));
        }
        Err(invalid_annotation(
            "shared node asset ownership cannot be determined",
        ))
    }

    fn reject_project_owned_shared_asset(&self, target: &ObjectRef) -> ApiResult<()> {
        match target {
            ObjectRef::NodeAsset { object_id, .. } => {
                let index = crate::starmap::library::load_library_index(&self.app_data_root)?;
                if index.entries.iter().any(|entry| {
                    entry.target.kind == crate::starmap::types::StarMapAssetKind::Node
                        && entry.target.asset_id == *object_id
                        && entry.scope.project_id().is_some()
                }) {
                    return Err(invalid_annotation(
                        "global tags cannot be bound to a project-owned asset",
                    ));
                }
                Ok(())
            }
            ObjectRef::StarMap { object_id, .. } => {
                let meta = self.get_starmap(object_id)?;
                if meta.project_id.is_some() {
                    return Err(invalid_annotation(
                        "global tags cannot be bound to a project-owned starmap",
                    ));
                }
                Ok(())
            }
            ObjectRef::NodeInstance { starmap_id, .. }
            | ObjectRef::EmbedInstance { starmap_id, .. }
            | ObjectRef::CanvasAnchor { starmap_id, .. } => {
                let meta = self.get_starmap(starmap_id)?;
                if meta.project_id.is_some() {
                    return Err(invalid_annotation(
                        "global tags cannot be bound to a project canvas instance",
                    ));
                }
                Ok(())
            }
            _ => Err(invalid_annotation(
                "global tags can only be bound to shared assets or canvases",
            )),
        }
    }

    /// Store that owns a link: the source's namespace.
    ///
    /// Project objects and project-owned assets live in their project store.
    /// Canvas instances live in the store of the canvas that owns them; a canvas
    /// without a project uses the global store.
    fn link_store_for_source(&self, source: &ObjectRef) -> ApiResult<AnnotationStore> {
        if let Some(project_id) = source.project_id() {
            return self.annotation_store(project_id);
        }
        // 全局作用域的共享素材：项目归属在 `scope` 里，直接按它选仓库。
        if let ObjectRef::StarMap { scope, .. } | ObjectRef::NodeAsset { scope, .. } = source {
            return match scope {
                ObjectScope::Global => Ok(self.global_annotation_store()),
                ObjectScope::Project { project_id } => self.annotation_store(project_id),
            };
        }
        // 画布实例/嵌入/锚点继承其画布的归属：画布属于作品时写作品域，
        // 独立画布写全局域。
        if let Some(starmap_id) = source.starmap_id() {
            let meta = self.get_starmap(starmap_id)?;
            return match meta.project_id.as_deref() {
                Some(project_id) => self.annotation_store(project_id),
                None => Ok(self.global_annotation_store()),
            };
        }
        Ok(self.global_annotation_store())
    }

    fn saved_chapter_text_matches(
        &self,
        project_id: &str,
        chapter_id: &str,
        text: &str,
    ) -> ApiResult<bool> {
        // read_chapter 会在 pending journal 存在时恢复半提交投影，必须用写锁。
        let core = self.core_write();
        for volume in core.list_volumes(project_id).map_err(WriterError::from)? {
            let chapter_exists = core
                .list_chapters(project_id, &volume.id)
                .map_err(WriterError::from)?
                .iter()
                .any(|chapter| chapter.id == chapter_id);
            if !chapter_exists {
                continue;
            }
            let saved = core
                .read_chapter(project_id, &volume.id, chapter_id)
                .map_err(WriterError::from)?;
            return Ok(saved.content == text);
        }
        Ok(false)
    }

    pub(crate) fn ensure_named_entries_migrated(&self, project_id: &str) -> ApiResult<()> {
        safe_project_id(project_id)?;
        let correction = crate::correction::CorrectionStore::open(&self.app_data_root)?;
        let store = self.annotation_store(project_id)?;
        let source_stamp = store.named_entries_source_stamp()?;
        let legacy_names = correction.legacy_entity_name_records(project_id)?;
        let dictionary_entries = correction.dictionary_entry_records(project_id)?;
        let mut grouped: HashMap<String, (HashSet<String>, HashSet<String>)> = HashMap::new();
        for relation in &legacy_names {
            let group = grouped.entry(relation.canonical.clone()).or_default();
            match relation.kind {
                CorrectionEntityNameKindDto::Alias => {
                    group.0.insert(relation.name.clone());
                }
                CorrectionEntityNameKindDto::Misspelling => {
                    group.1.insert(relation.name.clone());
                }
            }
        }
        let has_typed_names = dictionary_entries.iter().any(|entry| {
            matches!(
                entry.kind,
                CorrectionDictionaryEntryKindDto::Person | CorrectionDictionaryEntryKindDto::Place
            )
        });
        // Read-only correction requests reach this function several times per
        // chapter open. The source stamp is the cache key: our own writes and
        // external sync/MCP writes to annotations/entries both change it.
        // Reparse every named-entry JSON only when the version changed.
        if legacy_names.is_empty() && !has_typed_names {
            if !correction.named_entries_initialized(project_id, &source_stamp)? {
                self.sync_correction_named_entries(project_id)?;
            }
            return Ok(());
        }
        for entry in &dictionary_entries {
            if matches!(
                entry.kind,
                CorrectionDictionaryEntryKindDto::Person | CorrectionDictionaryEntryKindDto::Place
            ) {
                grouped.entry(entry.text.clone()).or_default();
            }
        }

        let existing = store.list_named_entries().map_err(WriterError::from)?;
        let mut by_name = existing
            .into_iter()
            .map(|entry| (entry.canonical_name.clone(), entry))
            .collect::<HashMap<_, _>>();
        let mut paths = Vec::new();
        for (canonical_name, (aliases, misspellings)) in grouped {
            let aliases = aliases.into_iter().collect::<Vec<_>>();
            let misspellings = misspellings.into_iter().collect::<Vec<_>>();
            let now = chrono::Utc::now().timestamp_millis();
            if let Some(mut entry) = by_name.remove(&canonical_name) {
                let mut variants = entry
                    .aliases
                    .iter()
                    .chain(entry.confirmed_misspellings.iter())
                    .cloned()
                    .collect::<HashSet<_>>();
                let mut changed = false;
                for alias in aliases {
                    if alias != canonical_name && variants.insert(alias.clone()) {
                        entry.aliases.push(alias);
                        changed = true;
                    }
                }
                for misspelling in misspellings {
                    if misspelling != canonical_name && variants.insert(misspelling.clone()) {
                        entry.confirmed_misspellings.push(misspelling);
                        changed = true;
                    }
                }
                if changed {
                    entry.updated_at_ms = now;
                    paths.extend(
                        store
                            .apply_mutation(AnnotationMutation::UpsertNamedEntry(entry))
                            .map_err(WriterError::from)?,
                    );
                }
            } else {
                let (canonical_name, aliases, confirmed_misspellings) =
                    validate_named_entry(canonical_name, aliases, misspellings)?;
                let entry = NamedEntry {
                    entry_id: uuid::Uuid::new_v4().to_string(),
                    project_id: project_id.to_string(),
                    canonical_name,
                    aliases,
                    confirmed_misspellings,
                    auto_link_enabled: false,
                    updated_at_ms: now,
                    deleted_at_ms: None,
                };
                paths.extend(
                    store
                        .apply_mutation(AnnotationMutation::UpsertNamedEntry(entry))
                        .map_err(WriterError::from)?,
                );
            }
        }
        let wrote_named_entries = !paths.is_empty();
        if wrote_named_entries {
            self.finish_annotation_write(project_id, paths, "migrate_correction_named_entries");
        }
        if !legacy_names.is_empty() {
            correction.clear_legacy_entity_name_records(project_id)?;
        }
        if has_typed_names {
            correction.clear_legacy_typed_entity_dictionary_entries(project_id)?;
        }
        if wrote_named_entries
            || !correction.named_entries_initialized(project_id, &source_stamp)?
        {
            self.sync_correction_named_entries(project_id)?;
        }
        Ok(())
    }

    /// Publish AnnotationStore's current named entries to the correction cache.
    ///
    /// The data and the source stamp are read under the same consistency rule:
    /// if the directory changed between the version snapshots, the records may
    /// mix two states, so publication is skipped and the next request retries.
    pub(crate) fn sync_correction_named_entries(&self, project_id: &str) -> ApiResult<()> {
        let store = self.annotation_store(project_id)?;
        let correction = crate::correction::CorrectionStore::open(&self.app_data_root)?;
        let mut last_read_error = None;
        for _ in 0..3 {
            let before = store.named_entries_source_stamp()?;
            let entries = match store.list_named_entries() {
                Ok(entries) => entries,
                Err(error) => {
                    // A concurrent atomic replace can remove a file between the
                    // directory scan and its read; retry against the new state.
                    last_read_error = Some(error);
                    continue;
                }
            };
            let after = store.named_entries_source_stamp()?;
            if before != after {
                continue;
            }
            correction.set_named_entries_for_project(
                project_id,
                entries.into_iter().map(named_entry_to_dto).collect(),
                after,
            )?;
            return Ok(());
        }
        if let Some(error) = last_read_error {
            return Err(WriterError::from(error));
        }
        // The source kept changing while it was being read. Keep the previous
        // cache and its old stamp instead of labeling stale records with the
        // newest version; the next correction request retries.
        log::warn!(
            "[correction] named entry source changed while syncing; retrying on next request"
        );
        Ok(())
    }

    fn finish_annotation_write(
        &self,
        project_id: &str,
        changed_paths: Vec<String>,
        operation: &str,
    ) {
        if changed_paths.is_empty() {
            return;
        }
        let workspace_paths = changed_paths
            .into_iter()
            .map(|path| PathBuf::from("projects").join(project_id).join(path))
            .collect::<Vec<_>>();
        self.record_workspace_paths_history(&workspace_paths, operation);
        self.refresh_annotation_search_index(project_id, operation);
    }

    pub(crate) fn refresh_annotation_search_index(&self, project_id: &str, operation: &str) {
        let project_root = self.projects_root.join(project_id);
        let existing_ids = self.annotation_search_object_ids_for_project(project_id);
        match crate::search::extractor::annotation_search_updates_for_project(
            &self.app_data_root,
            &project_root,
            project_id,
            &existing_ids,
        ) {
            Ok(updates) => {
                for update in updates {
                    self.enqueue_search_index_update(update);
                }
            }
            Err(error) => log::warn!("annotation search update skipped after {operation}: {error}"),
        }
    }

    /// Project tag catalogue only; global asset tags never leak into it.
    pub fn list_tags(&self, project_id: String) -> ApiResult<Vec<AnnotationTagDto>> {
        self.annotation_store(&project_id)?
            .list_tags()
            .map(|values| values.into_iter().map(tag_to_dto).collect())
            .map_err(Into::into)
    }

    /// Global shared-asset tag catalogue.
    pub fn list_global_tags(&self) -> ApiResult<Vec<AnnotationTagDto>> {
        self.global_annotation_store()
            .list_tags()
            .map(|values| values.into_iter().map(tag_to_dto).collect())
            .map_err(Into::into)
    }

    pub fn create_tag(
        &self,
        project_id: String,
        name: String,
        color: Option<String>,
    ) -> ApiResult<AnnotationTagDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let name = name.trim();
        if name.is_empty() || name.chars().any(char::is_control) {
            return Err(invalid_annotation("tag name must be non-empty text"));
        }
        let store = self.annotation_store(&project_id)?;
        if store.list_tags()?.iter().any(|tag| tag.name == name) {
            return Err(invalid_annotation(
                "tag name already exists in this project",
            ));
        }
        let now = chrono::Utc::now().timestamp_millis();
        let tag = Tag {
            tag_id: uuid::Uuid::new_v4().to_string(),
            project_id: project_id.clone(),
            name: name.to_string(),
            color: color
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            updated_at_ms: now,
            deleted_at_ms: None,
        };
        let result = tag_to_dto(tag.clone());
        let after = Self::annotation_snapshot(tag.updated_at_ms.to_string(), tag.name.clone());
        let paths = store
            .apply_mutation(AnnotationMutation::UpsertTag(tag))
            .map_err(WriterError::from)?;
        self.finish_annotation_write(&project_id, paths, "create_annotation_tag");
        self.record_annotation_object(
            &project_id,
            crate::writing_stats::ContributionObjectType::Tag,
            &result.tag_id,
            crate::writing_stats::ContributionOperation::Create,
            None,
            Some(after),
        )?;
        Ok(result)
    }

    pub fn rename_tag(
        &self,
        project_id: String,
        tag_id: String,
        name: String,
    ) -> ApiResult<AnnotationTagDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let name = name.trim();
        if name.is_empty() || name.chars().any(char::is_control) {
            return Err(invalid_annotation("tag name must be non-empty text"));
        }
        let store = self.annotation_store(&project_id)?;
        if store
            .list_tags()?
            .iter()
            .any(|tag| tag.name == name && tag.tag_id != tag_id)
        {
            return Err(invalid_annotation(
                "tag name already exists in this project",
            ));
        }
        let mut tag = store
            .get_tag(&tag_id)
            .map_err(WriterError::from)?
            .filter(|tag| tag.deleted_at_ms.is_none())
            .ok_or_else(|| invalid_annotation("tag does not exist"))?;
        let before = Self::annotation_snapshot(tag.updated_at_ms.to_string(), tag.name.clone());
        tag.name = name.to_string();
        tag.updated_at_ms = chrono::Utc::now().timestamp_millis();
        let result = tag_to_dto(tag.clone());
        let after = Self::annotation_snapshot(tag.updated_at_ms.to_string(), tag.name.clone());
        let paths = store
            .apply_mutation(AnnotationMutation::UpsertTag(tag))
            .map_err(WriterError::from)?;
        self.finish_annotation_write(&project_id, paths, "rename_annotation_tag");
        self.record_annotation_object(
            &project_id,
            crate::writing_stats::ContributionObjectType::Tag,
            &result.tag_id,
            crate::writing_stats::ContributionOperation::Update,
            Some(before),
            Some(after),
        )?;
        Ok(result)
    }

    pub fn delete_tag(&self, project_id: String, tag_id: String) -> ApiResult<bool> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let store = self.annotation_store(&project_id)?;
        let before = store
            .get_tag(&tag_id)
            .map_err(WriterError::from)?
            .filter(|tag| tag.deleted_at_ms.is_none())
            .map(|tag| Self::annotation_snapshot(tag.updated_at_ms.to_string(), tag.name));
        let paths = store
            .apply_mutation(AnnotationMutation::Delete {
                kind: AnnotationKind::Tag,
                scope: ObjectScope::Project {
                    project_id: project_id.clone(),
                },
                object_id: tag_id.clone(),
                deleted_at_ms: chrono::Utc::now().timestamp_millis(),
            })
            .map_err(WriterError::from)?;
        let changed = !paths.is_empty();
        self.finish_annotation_write(&project_id, paths, "delete_annotation_tag");
        if changed {
            self.record_annotation_object(
                &project_id,
                crate::writing_stats::ContributionObjectType::Tag,
                &tag_id,
                crate::writing_stats::ContributionOperation::Delete,
                before,
                None,
            )?;
        }
        Ok(changed)
    }

    pub fn merge_tags(
        &self,
        project_id: String,
        source_tag_id: String,
        target_tag_id: String,
    ) -> ApiResult<bool> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let mut contribution_txn = self.begin_annotation_contribution_transaction(&project_id)?;
        let mut contribution_batch = self.begin_contribution_batch();
        let store = self.annotation_store(&project_id)?;
        let source_before = store
            .get_tag(&source_tag_id)
            .map_err(WriterError::from)?
            .filter(|tag| tag.deleted_at_ms.is_none())
            .map(|tag| Self::annotation_snapshot(tag.updated_at_ms.to_string(), tag.name.clone()));
        let target_before = store
            .get_tag(&target_tag_id)
            .map_err(WriterError::from)?
            .filter(|tag| tag.deleted_at_ms.is_none())
            .map(|tag| Self::annotation_snapshot(tag.updated_at_ms.to_string(), tag.name));
        let paths = store
            .merge_tags(
                &source_tag_id,
                &target_tag_id,
                chrono::Utc::now().timestamp_millis(),
            )
            .map_err(WriterError::from)?;
        let changed = !paths.is_empty();
        self.finish_annotation_write(&project_id, paths, "merge_annotation_tags");
        if changed {
            let target_after = store
                .get_tag(&target_tag_id)
                .map_err(WriterError::from)?
                .filter(|tag| tag.deleted_at_ms.is_none())
                .map(|tag| Self::annotation_snapshot(tag.updated_at_ms.to_string(), tag.name));
            let context = crate::writing_stats::current_mutation_context();
            self.record_annotation_object_with_context(
                &context,
                &project_id,
                crate::writing_stats::ContributionObjectType::Tag,
                &source_tag_id,
                crate::writing_stats::ContributionOperation::Delete,
                source_before,
                None,
            )?;
            self.record_annotation_object_with_context(
                &context,
                &project_id,
                crate::writing_stats::ContributionObjectType::Tag,
                &target_tag_id,
                crate::writing_stats::ContributionOperation::Update,
                target_before,
                target_after,
            )?;
        }
        self.commit_contribution_batch(&mut contribution_batch)?;
        contribution_txn.commit()?;
        Ok(changed)
    }

    /// Replace the tag set of one typed object by difference.
    ///
    /// Bindings are unique per (tag namespace, tag, typed target): existing rows
    /// are kept, removed rows become tombstones, and new rows use the store's
    /// deterministic binding ID. No full old set is written back, so a concurrent
    /// add on another device is not overwritten by a stale snapshot.
    ///
    /// An empty `project_id` means the global shared-asset catalogue; project
    /// tags can point at shared assets the project may access (its own or the
    /// global library), never at another project's objects.
    pub fn set_tag_bindings(
        &self,
        project_id: String,
        target: AnnotationObjectRefDto,
        tag_ids: Vec<String>,
    ) -> ApiResult<Vec<AnnotationTagBindingDto>> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        if !project_id.is_empty() {
            safe_project_id(&project_id)?;
        }
        let target = object_ref_from_dto(target)?;
        self.authorize_tag_binding_target(&project_id, &target)?;
        let mut contribution_txn = if project_id.is_empty() {
            None
        } else {
            Some(self.begin_annotation_contribution_transaction(&project_id)?)
        };
        let mut contribution_batch = self.begin_contribution_batch();
        let store = self.tag_namespace_store(&project_id)?;
        let tags = store.list_tags().map_err(WriterError::from)?;
        let tags_by_id = tags
            .into_iter()
            .map(|tag| (tag.tag_id.clone(), tag))
            .collect::<HashMap<_, _>>();
        let mut desired = HashSet::new();
        for tag_id in tag_ids {
            if !tags_by_id.contains_key(&tag_id) {
                return Err(invalid_annotation("tag binding refers to a missing tag"));
            }
            desired.insert(tag_id);
        }
        if !project_id.is_empty() {
            if let Some(anchor_id) = text_anchor_id(&target) {
                let mut baselines = self
                    .annotation_edit_baselines
                    .lock()
                    .map_err(|_| invalid_annotation("annotation edit baseline lock poisoned"))?;
                if let Some(baseline) =
                    find_session_created_anchor_baseline_mut(&mut baselines, &project_id, anchor_id)
                {
                    let existing = baseline
                        .pending_bindings
                        .values()
                        .filter(|binding| binding.target == target)
                        .cloned()
                        .map(|binding| (binding.tag_id.clone(), binding))
                        .collect::<HashMap<_, _>>();
                    baseline
                        .pending_bindings
                        .retain(|_, binding| binding.target != target);
                    let now = chrono::Utc::now().timestamp_millis();
                    let mut desired_ids = desired.into_iter().collect::<Vec<_>>();
                    desired_ids.sort();
                    let mut result = Vec::with_capacity(desired_ids.len());
                    for tag_id in desired_ids {
                        let Some(tag) = tags_by_id.get(&tag_id) else {
                            return Err(invalid_annotation("tag binding refers to a missing tag"));
                        };
                        let binding =
                            existing
                                .get(&tag_id)
                                .cloned()
                                .unwrap_or_else(|| TagBinding {
                                    binding_id: AnnotationStore::binding_id_for_relation(
                                        tag, &target,
                                    ),
                                    project_id: project_id.clone(),
                                    tag_id,
                                    target: target.clone(),
                                    updated_at_ms: now,
                                    deleted_at_ms: None,
                                });
                        baseline
                            .pending_bindings
                            .insert(binding.binding_id.clone(), binding.clone());
                        result.push(binding_to_dto(binding));
                    }
                    baseline.updated_at_ms = now;
                    return Ok(result);
                }
            }
        }
        let existing = store
            .list_bindings_for_target(&target)
            .map_err(WriterError::from)?;
        let existing_ids = existing
            .iter()
            .map(|binding| binding.tag_id.clone())
            .collect::<HashSet<_>>();
        let now = chrono::Utc::now().timestamp_millis();
        let mut changed_paths = Vec::new();
        let mut removed_bindings = Vec::new();
        let mut added_bindings = Vec::new();
        for binding in existing {
            if desired.contains(&binding.tag_id) {
                continue;
            }
            // A deleted tag's relation still has a stable key; tombstone the
            // binding even when the tag definition is already gone.
            let tag = tags_by_id.get(&binding.tag_id).cloned().unwrap_or(Tag {
                tag_id: binding.tag_id.clone(),
                project_id: binding.project_id.clone(),
                name: String::new(),
                color: None,
                updated_at_ms: binding.updated_at_ms,
                deleted_at_ms: None,
            });
            let binding_id = binding.binding_id.clone();
            let snapshot = Self::annotation_snapshot(
                binding.updated_at_ms.to_string(),
                binding.tag_id.clone(),
            );
            changed_paths.extend(
                store
                    .detach_tag(&tag, &target, now)
                    .map_err(WriterError::from)?,
            );
            removed_bindings.push((binding_id, snapshot));
        }
        for tag_id in desired {
            if existing_ids.contains(&tag_id) {
                continue;
            }
            let Some(tag) = tags_by_id.get(&tag_id) else {
                return Err(invalid_annotation("tag binding refers to a missing tag"));
            };
            let (binding, changed) = store
                .attach_tag(tag, &target, now)
                .map_err(WriterError::from)?;
            changed_paths.extend(changed);
            added_bindings.push((
                binding.binding_id.clone(),
                Self::annotation_snapshot(
                    binding.updated_at_ms.to_string(),
                    binding.tag_id.clone(),
                ),
            ));
        }
        self.finish_annotation_store_write(
            &project_id,
            changed_paths,
            "set_annotation_tag_bindings",
        );
        if !project_id.is_empty() {
            let context = crate::writing_stats::current_mutation_context();
            for (binding_id, snapshot) in removed_bindings {
                self.record_annotation_object_with_context(
                    &context,
                    &project_id,
                    crate::writing_stats::ContributionObjectType::TagBinding,
                    &binding_id,
                    crate::writing_stats::ContributionOperation::Unlink,
                    Some(snapshot),
                    None,
                )?;
            }
            for (binding_id, snapshot) in added_bindings {
                self.record_annotation_object_with_context(
                    &context,
                    &project_id,
                    crate::writing_stats::ContributionObjectType::TagBinding,
                    &binding_id,
                    crate::writing_stats::ContributionOperation::Link,
                    None,
                    Some(snapshot),
                )?;
            }
            self.commit_contribution_batch(&mut contribution_batch)?;
        }
        if let Some(mut contribution_txn) = contribution_txn.take() {
            contribution_txn.commit()?;
        }
        store
            .list_bindings_for_target(&target)
            .map(|values| values.into_iter().map(binding_to_dto).collect())
            .map_err(Into::into)
    }

    /// Idempotently attach one tag to one typed object.
    ///
    /// An empty `project_id` addresses the global shared-asset catalogue.
    pub fn attach_tag(
        &self,
        project_id: String,
        tag_id: String,
        target: AnnotationObjectRefDto,
    ) -> ApiResult<AnnotationTagBindingDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        if !project_id.is_empty() {
            safe_project_id(&project_id)?;
        }
        let target = object_ref_from_dto(target)?;
        self.authorize_tag_binding_target(&project_id, &target)?;
        let mut contribution_txn = if project_id.is_empty() {
            None
        } else {
            Some(self.begin_annotation_contribution_transaction(&project_id)?)
        };
        let mut contribution_batch = self.begin_contribution_batch();
        let store = self.tag_namespace_store(&project_id)?;
        let tag = store
            .get_tag(&tag_id)
            .map_err(WriterError::from)?
            .filter(|tag| tag.deleted_at_ms.is_none())
            .ok_or_else(|| invalid_annotation("tag does not exist"))?;
        let (binding, changed) = store
            .attach_tag(&tag, &target, chrono::Utc::now().timestamp_millis())
            .map_err(WriterError::from)?;
        let created = !changed.is_empty();
        self.finish_annotation_store_write(&project_id, changed, "attach_annotation_tag");
        if created && !project_id.is_empty() {
            let context = crate::writing_stats::current_mutation_context();
            self.record_annotation_object_with_context(
                &context,
                &project_id,
                crate::writing_stats::ContributionObjectType::TagBinding,
                &binding.binding_id,
                crate::writing_stats::ContributionOperation::Link,
                None,
                Some(Self::annotation_snapshot(
                    binding.updated_at_ms.to_string(),
                    binding.tag_id.clone(),
                )),
            )?;
            self.commit_contribution_batch(&mut contribution_batch)?;
        }
        if let Some(mut contribution_txn) = contribution_txn.take() {
            contribution_txn.commit()?;
        }
        Ok(binding_to_dto(binding))
    }

    /// Idempotently remove one tag/target relation, keeping the tombstone.
    pub fn detach_tag(
        &self,
        project_id: String,
        tag_id: String,
        target: AnnotationObjectRefDto,
    ) -> ApiResult<bool> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        if !project_id.is_empty() {
            safe_project_id(&project_id)?;
        }
        let target = object_ref_from_dto(target)?;
        self.authorize_tag_binding_target(&project_id, &target)?;
        let mut contribution_txn = if project_id.is_empty() {
            None
        } else {
            Some(self.begin_annotation_contribution_transaction(&project_id)?)
        };
        let mut contribution_batch = self.begin_contribution_batch();
        let store = self.tag_namespace_store(&project_id)?;
        let tag = store
            .get_tag(&tag_id)
            .map_err(WriterError::from)?
            .filter(|tag| tag.deleted_at_ms.is_none())
            .ok_or_else(|| invalid_annotation("tag does not exist"))?;
        let binding = store
            .find_binding_by_relation(&tag, &target)
            .map_err(WriterError::from)?;
        let changed = store
            .detach_tag(&tag, &target, chrono::Utc::now().timestamp_millis())
            .map_err(WriterError::from)?;
        let removed = !changed.is_empty();
        self.finish_annotation_store_write(&project_id, changed, "detach_annotation_tag");
        if removed && !project_id.is_empty() {
            let context = crate::writing_stats::current_mutation_context();
            if let Some(binding) = binding {
                self.record_annotation_object_with_context(
                    &context,
                    &project_id,
                    crate::writing_stats::ContributionObjectType::TagBinding,
                    &binding.binding_id,
                    crate::writing_stats::ContributionOperation::Unlink,
                    Some(Self::annotation_snapshot(
                        binding.updated_at_ms.to_string(),
                        binding.tag_id,
                    )),
                    None,
                )?;
            }
            self.commit_contribution_batch(&mut contribution_batch)?;
        }
        if let Some(mut contribution_txn) = contribution_txn.take() {
            contribution_txn.commit()?;
        }
        Ok(removed)
    }

    /// Read-only tag projection for one typed object.
    ///
    /// The catalogue is resolved from the object's own namespace: project
    /// objects and project-owned shared assets use the project catalogue; canvas
    /// instances use their canvas's project catalogue; global assets and global
    /// canvases use the global catalogue.
    pub fn list_tags_for_object(
        &self,
        target: AnnotationObjectRefDto,
    ) -> ApiResult<Vec<AnnotationTagDto>> {
        let target = object_ref_from_dto(target)?;
        let store = self.tag_catalogue_store_for_object(&target)?;
        let bindings = store
            .list_bindings_for_scope(&target)
            .map_err(WriterError::from)?;
        let tags = store
            .list_tags()
            .map_err(WriterError::from)?
            .into_iter()
            .map(|tag| (tag.tag_id.clone(), tag))
            .collect::<HashMap<_, _>>();
        let mut result = bindings
            .into_iter()
            .filter_map(|binding| tags.get(&binding.tag_id).cloned())
            .map(tag_to_dto)
            .collect::<Vec<_>>();
        result.sort_by(|left, right| left.tag_id.cmp(&right.tag_id));
        Ok(result)
    }

    fn tag_catalogue_store_for_object(&self, target: &ObjectRef) -> ApiResult<AnnotationStore> {
        if let Some(project_id) = target.project_id() {
            return self.annotation_store(project_id);
        }
        if let Some(starmap_id) = target.starmap_id() {
            let meta = self.get_starmap(starmap_id)?;
            if let Some(project_id) = meta.project_id.as_deref() {
                return self.annotation_store(project_id);
            }
        }
        Ok(self.global_annotation_store())
    }

    pub fn list_bindings_for_target(
        &self,
        project_id: String,
        target: AnnotationObjectRefDto,
    ) -> ApiResult<Vec<AnnotationTagBindingDto>> {
        let target = object_ref_from_dto(target)?;
        let mut bindings = self
            .annotation_store(&project_id)?
            .list_bindings_for_target(&target)
            .map_err(WriterError::from)?;
        if text_anchor_id(&target).is_some() {
            let baselines = self
                .annotation_edit_baselines
                .lock()
                .map_err(|_| invalid_annotation("annotation edit baseline lock poisoned"))?;
            for baseline in baselines
                .values()
                .filter(|baseline| baseline.project_id == project_id)
            {
                bindings.extend(
                    baseline
                        .pending_bindings
                        .values()
                        .filter(|binding| binding.target == target)
                        .cloned(),
                );
            }
        }
        bindings.sort_by(|left, right| left.binding_id.cmp(&right.binding_id));
        Ok(bindings.into_iter().map(binding_to_dto).collect())
    }

    pub fn create_text_anchor(
        &self,
        project_id: String,
        chapter_id: String,
        start_byte: u32,
        end_byte: u32,
        text_revision: u64,
        text: String,
    ) -> ApiResult<AnnotationTextAnchorDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        safe_project_id(&project_id)?;
        if chapter_id.is_empty() || start_byte >= end_byte {
            return Err(invalid_annotation(
                "text anchor must identify a non-empty chapter range",
            ));
        }
        if !self.object_ref_exists(&ObjectRef::Chapter {
            project_id: project_id.clone(),
            object_id: chapter_id.clone(),
        })? {
            return Err(invalid_annotation(
                "text anchor chapter does not exist in this project",
            ));
        }
        let start =
            usize::try_from(start_byte).map_err(|error| invalid_annotation(error.to_string()))?;
        let end =
            usize::try_from(end_byte).map_err(|error| invalid_annotation(error.to_string()))?;
        if end > text.len() || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            return Err(invalid_annotation(
                "text anchor range is not a valid UTF-8 byte range",
            ));
        }
        let store = self.annotation_store(&project_id)?;
        let mut anchor = TextAnchor {
            anchor_id: uuid::Uuid::new_v4().to_string(),
            project_id: project_id.clone(),
            chapter_id: chapter_id.clone(),
            start_byte: start,
            end_byte: end,
            text_hash: String::new(),
            anchored_text: String::new(),
            context_before: String::new(),
            context_after: String::new(),
            updated_at_ms: chrono::Utc::now().timestamp_millis(),
            deleted_at_ms: None,
        };
        capture_text_anchor(&mut anchor, &text).map_err(WriterError::from)?;
        let result = anchor_to_dto(anchor.clone())?;
        if self.saved_chapter_text_matches(&project_id, &chapter_id, &text)? {
            let anchor_id = anchor.anchor_id.clone();
            let snapshot = Self::annotation_snapshot(
                anchor.updated_at_ms.to_string(),
                format!("{chapter_id}:{start_byte}..{end_byte}"),
            );
            let paths = store
                .apply_mutation(AnnotationMutation::UpsertTextAnchor(anchor))
                .map_err(WriterError::from)?;
            self.finish_annotation_write(&project_id, paths, "create_annotation_text_anchor");
            self.record_annotation_object(
                &project_id,
                crate::writing_stats::ContributionObjectType::TextAnchor,
                &anchor_id,
                crate::writing_stats::ContributionOperation::Create,
                None,
                Some(snapshot),
            )?;
            return Ok(result);
        }
        let now = chrono::Utc::now().timestamp_millis();
        let mut baselines = self
            .annotation_edit_baselines
            .lock()
            .map_err(|_| invalid_annotation("annotation edit baseline lock poisoned"))?;
        let baseline = baselines
            .values_mut()
            .filter(|baseline| {
                baseline.project_id == project_id
                    && baseline.chapter_id == chapter_id
                    && baseline.text_revision == text_revision
                    && baseline.text_len == text.len()
            })
            .max_by_key(|baseline| baseline.updated_at_ms)
            .ok_or_else(|| {
                invalid_annotation(
                    "unsaved text anchor requires an active chapter annotation session",
                )
            })?;
        if baseline
            .anchors
            .iter()
            .any(|existing| existing.anchor_id == anchor.anchor_id)
        {
            return Err(invalid_annotation(
                "text anchor ID already exists in session",
            ));
        }
        baseline.anchors.push(anchor);
        baseline.updated_at_ms = now;
        Ok(result)
    }

    pub fn list_named_entries(
        &self,
        project_id: String,
    ) -> ApiResult<Vec<AnnotationNamedEntryDto>> {
        self.ensure_named_entries_migrated(&project_id)?;
        self.annotation_store(&project_id)?
            .list_named_entries()
            .map(|values| values.into_iter().map(named_entry_to_dto).collect())
            .map_err(Into::into)
    }

    pub fn create_named_entry(
        &self,
        project_id: String,
        canonical_name: String,
        aliases: Vec<String>,
        confirmed_misspellings: Vec<String>,
        auto_link_enabled: bool,
    ) -> ApiResult<AnnotationNamedEntryDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        self.ensure_named_entries_migrated(&project_id)?;
        let (canonical_name, aliases, confirmed_misspellings) =
            validate_named_entry(canonical_name, aliases, confirmed_misspellings)?;
        let store = self.annotation_store(&project_id)?;
        let entry = NamedEntry {
            entry_id: uuid::Uuid::new_v4().to_string(),
            project_id: project_id.clone(),
            canonical_name,
            aliases,
            confirmed_misspellings,
            auto_link_enabled,
            updated_at_ms: chrono::Utc::now().timestamp_millis(),
            deleted_at_ms: None,
        };
        let result = named_entry_to_dto(entry.clone());
        let snapshot = Self::annotation_snapshot(
            entry.updated_at_ms.to_string(),
            entry.canonical_name.clone(),
        );
        let paths = store
            .apply_mutation(AnnotationMutation::UpsertNamedEntry(entry))
            .map_err(WriterError::from)?;
        self.sync_correction_named_entries(&project_id)?;
        self.finish_annotation_write(&project_id, paths, "create_named_entry");
        self.record_annotation_object(
            &project_id,
            crate::writing_stats::ContributionObjectType::NamedEntry,
            &result.entry_id,
            crate::writing_stats::ContributionOperation::Create,
            None,
            Some(snapshot),
        )?;
        Ok(result)
    }

    pub fn update_named_entry(
        &self,
        project_id: String,
        entry_id: String,
        canonical_name: String,
        aliases: Vec<String>,
        confirmed_misspellings: Vec<String>,
        auto_link_enabled: bool,
    ) -> ApiResult<AnnotationNamedEntryDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        self.ensure_named_entries_migrated(&project_id)?;
        let (canonical_name, aliases, confirmed_misspellings) =
            validate_named_entry(canonical_name, aliases, confirmed_misspellings)?;
        let store = self.annotation_store(&project_id)?;
        let mut entry = store
            .get_named_entry(&entry_id)
            .map_err(WriterError::from)?
            .filter(|entry| entry.deleted_at_ms.is_none())
            .ok_or_else(|| invalid_annotation("named entry does not exist"))?;
        let before = Self::annotation_snapshot(
            entry.updated_at_ms.to_string(),
            entry.canonical_name.clone(),
        );
        entry.canonical_name = canonical_name;
        entry.aliases = aliases;
        entry.confirmed_misspellings = confirmed_misspellings;
        entry.auto_link_enabled = auto_link_enabled;
        entry.updated_at_ms = chrono::Utc::now().timestamp_millis();
        let result = named_entry_to_dto(entry.clone());
        let after = Self::annotation_snapshot(
            entry.updated_at_ms.to_string(),
            entry.canonical_name.clone(),
        );
        let paths = store
            .apply_mutation(AnnotationMutation::UpsertNamedEntry(entry))
            .map_err(WriterError::from)?;
        self.sync_correction_named_entries(&project_id)?;
        self.finish_annotation_write(&project_id, paths, "update_named_entry");
        self.record_annotation_object(
            &project_id,
            crate::writing_stats::ContributionObjectType::NamedEntry,
            &result.entry_id,
            crate::writing_stats::ContributionOperation::Update,
            Some(before),
            Some(after),
        )?;
        Ok(result)
    }

    pub fn delete_named_entry(&self, project_id: String, entry_id: String) -> ApiResult<bool> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        self.ensure_named_entries_migrated(&project_id)?;
        let store = self.annotation_store(&project_id)?;
        let before = store
            .get_named_entry(&entry_id)
            .map_err(WriterError::from)?
            .filter(|entry| entry.deleted_at_ms.is_none())
            .map(|entry| {
                Self::annotation_snapshot(entry.updated_at_ms.to_string(), entry.canonical_name)
            });
        let paths = store
            .apply_mutation(AnnotationMutation::Delete {
                kind: AnnotationKind::NamedEntry,
                scope: ObjectScope::Project {
                    project_id: project_id.clone(),
                },
                object_id: entry_id.clone(),
                deleted_at_ms: chrono::Utc::now().timestamp_millis(),
            })
            .map_err(WriterError::from)?;
        let changed = !paths.is_empty();
        self.sync_correction_named_entries(&project_id)?;
        self.finish_annotation_write(&project_id, paths, "delete_named_entry");
        if changed {
            self.record_annotation_object(
                &project_id,
                crate::writing_stats::ContributionObjectType::NamedEntry,
                &entry_id,
                crate::writing_stats::ContributionOperation::Delete,
                before,
                None,
            )?;
        }
        Ok(changed)
    }

    /// Create (or idempotently re-use) one navigation link.
    ///
    /// The link is stored once in the store that owns its source: project
    /// objects in their project, canvas instances in the project that owns the
    /// canvas (global store for an independent canvas), shared assets in their
    /// own namespace. It is never copied into the destination.
    pub fn create_link(
        &self,
        source: AnnotationObjectRefDto,
        target: AnnotationTargetDto,
    ) -> ApiResult<AnnotationObjectLinkDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let source = object_ref_from_dto(source)?;
        let target = link_target_from_dto(target)?;
        if !self.object_ref_exists(&source)? {
            return Err(invalid_annotation("link source does not exist"));
        }
        self.authorize_link_target(&target)?;
        let store = self.link_store_for_source(&source)?;
        let project_id = store.project_id().to_string();
        let now = chrono::Utc::now().timestamp_millis();
        let link = ObjectLink {
            link_id: uuid::Uuid::new_v4().to_string(),
            source,
            target,
            origin: LinkOrigin::Manual,
            label: None,
            created_at_ms: now,
            updated_at_ms: now,
            deleted_at_ms: None,
        };
        if let Some(anchor_id) = text_anchor_id(&link.source) {
            let mut baselines = self
                .annotation_edit_baselines
                .lock()
                .map_err(|_| invalid_annotation("annotation edit baseline lock poisoned"))?;
            if let Some(baseline) =
                find_session_created_anchor_baseline_mut(&mut baselines, &project_id, anchor_id)
            {
                let result = link_to_dto(link.clone());
                baseline.pending_links.insert(link.link_id.clone(), link);
                baseline.updated_at_ms = chrono::Utc::now().timestamp_millis();
                return Ok(result);
            }
        }
        let (link, paths) = store.attach_link(link).map_err(WriterError::from)?;
        let created = !paths.is_empty();
        self.finish_annotation_store_write(&project_id, paths, "create_object_link");
        if created && !project_id.is_empty() {
            self.record_annotation_object(
                &project_id,
                crate::writing_stats::ContributionObjectType::ObjectLink,
                &link.link_id,
                crate::writing_stats::ContributionOperation::Create,
                None,
                Some(Self::annotation_snapshot(
                    link.updated_at_ms.to_string(),
                    format!("{:?}", link.target),
                )),
            )?;
        }
        Ok(link_to_dto(link))
    }

    fn authorize_link_target(&self, target: &LinkTarget) -> ApiResult<()> {
        if let LinkTarget::CanvasNavigation(route) = target {
            if route.starmap_id.is_empty() {
                return Err(invalid_annotation("canvas route needs a starmapId"));
            }
            self.get_starmap(&route.starmap_id)?;
        }
        Ok(())
    }

    pub fn update_link(
        &self,
        project_id: String,
        link_id: String,
        source: AnnotationObjectRefDto,
        target: AnnotationTargetDto,
    ) -> ApiResult<AnnotationObjectLinkDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        safe_project_id(&project_id)?;
        let source = object_ref_from_dto(source)?;
        let store = self.link_store_for_source(&source)?;
        let store_project_id = store.project_id().to_string();
        if source.project_id().is_some() && store_project_id != project_id {
            return Err(invalid_annotation(
                "link source must belong to its storage project",
            ));
        }
        if !self.object_ref_exists(&source)? {
            return Err(invalid_annotation("link source does not exist"));
        }
        let target = link_target_from_dto(target)?;
        self.authorize_link_target(&target)?;
        let now = chrono::Utc::now().timestamp_millis();
        let pending_update = {
            let mut baselines = self
                .annotation_edit_baselines
                .lock()
                .map_err(|_| invalid_annotation("annotation edit baseline lock poisoned"))?;
            let origin_baseline_id = baselines
                .iter()
                .find(|(_, baseline)| {
                    baseline.project_id == project_id
                        && baseline.pending_links.contains_key(&link_id)
                })
                .map(|(id, _)| id.clone());
            if let Some(origin_baseline_id) = origin_baseline_id {
                let Some(mut link) = baselines
                    .get_mut(&origin_baseline_id)
                    .and_then(|baseline| baseline.pending_links.remove(&link_id))
                else {
                    return Err(invalid_annotation("pending link changed during update"));
                };
                link.source = source.clone();
                link.target = target.clone();

                let destination_baseline_id = match &link.source {
                    ObjectRef::TextAnchor {
                        project_id: source_project_id,
                        object_id,
                    } if source_project_id == &project_id => baselines
                        .iter()
                        .filter(|(_, baseline)| baseline.project_id == project_id)
                        .filter(|(_, baseline)| {
                            baseline.anchors.iter().any(|anchor| {
                                anchor.anchor_id == *object_id && anchor.deleted_at_ms.is_none()
                            })
                        })
                        .max_by_key(|(_, baseline)| baseline.updated_at_ms)
                        .map(|(id, _)| id.clone()),
                    _ => None,
                };

                if let Some(destination_baseline_id) = destination_baseline_id {
                    let Some(destination) = baselines.get(&destination_baseline_id) else {
                        return Err(invalid_annotation("destination annotation session expired"));
                    };
                    let latest_session_update = baselines
                        .values()
                        .filter(|baseline| {
                            baseline.project_id == destination.project_id
                                && baseline.chapter_id == destination.chapter_id
                        })
                        .map(|baseline| baseline.updated_at_ms)
                        .max()
                        .unwrap_or(now);
                    link.updated_at_ms = now.max(latest_session_update.saturating_add(1));
                    let Some(destination) = baselines.get_mut(&destination_baseline_id) else {
                        return Err(invalid_annotation("destination annotation session expired"));
                    };
                    destination
                        .pending_links
                        .insert(link_id.clone(), link.clone());
                    destination.updated_at_ms = link.updated_at_ms;
                    Some((Some(destination_baseline_id), link))
                } else {
                    // The replacement source is already durable and has no
                    // active chapter edit session. Persist it directly; keep
                    // the pending copy until the write succeeds.
                    link.updated_at_ms = now.max(link.updated_at_ms.saturating_add(1));
                    let Some(origin_baseline) = baselines.get_mut(&origin_baseline_id) else {
                        return Err(invalid_annotation("origin annotation session expired"));
                    };
                    origin_baseline
                        .pending_links
                        .insert(link_id.clone(), link.clone());
                    Some((None, link))
                }
            } else {
                None
            }
        };
        if let Some((Some(_destination_baseline_id), link)) = pending_update.as_ref() {
            return Ok(link_to_dto(link.clone()));
        }
        if let Some((None, link)) = pending_update {
            let paths = store
                .apply_mutation(AnnotationMutation::UpsertLink(link.clone()))
                .map_err(WriterError::from)?;
            self.finish_annotation_store_write(&project_id, paths, "update_object_link");
            self.record_annotation_object(
                &project_id,
                crate::writing_stats::ContributionObjectType::ObjectLink,
                &link.link_id,
                crate::writing_stats::ContributionOperation::Update,
                None,
                Some(Self::annotation_snapshot(
                    link.updated_at_ms.to_string(),
                    format!("{:?}", link.target),
                )),
            )?;
            let mut baselines = self
                .annotation_edit_baselines
                .lock()
                .map_err(|_| invalid_annotation("annotation edit baseline lock poisoned"))?;
            for baseline in baselines
                .values_mut()
                .filter(|baseline| baseline.project_id == project_id)
            {
                if baseline.pending_links.get(&link_id) == Some(&link) {
                    baseline.pending_links.remove(&link_id);
                    baseline.updated_at_ms = now;
                    break;
                }
            }
            return Ok(link_to_dto(link));
        }
        let mut link = store
            .get_link(&link_id)
            .map_err(WriterError::from)?
            .filter(|link| link.deleted_at_ms.is_none())
            .ok_or_else(|| invalid_annotation("link does not exist"))?;
        let before =
            Self::annotation_snapshot(link.updated_at_ms.to_string(), format!("{:?}", link.target));
        link.source = source;
        link.target = target;
        link.updated_at_ms = chrono::Utc::now().timestamp_millis();
        let result = link_to_dto(link.clone());
        let after =
            Self::annotation_snapshot(link.updated_at_ms.to_string(), format!("{:?}", link.target));
        let paths = store
            .apply_mutation(AnnotationMutation::UpsertLink(link))
            .map_err(WriterError::from)?;
        self.finish_annotation_store_write(&store_project_id, paths, "update_object_link");
        if !store_project_id.is_empty() {
            self.record_annotation_object(
                &store_project_id,
                crate::writing_stats::ContributionObjectType::ObjectLink,
                &result.link_id,
                crate::writing_stats::ContributionOperation::Update,
                Some(before),
                Some(after),
            )?;
        }
        Ok(result)
    }

    pub fn delete_link(&self, project_id: String, link_id: String) -> ApiResult<bool> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        safe_project_id(&project_id)?;
        {
            let mut baselines = self
                .annotation_edit_baselines
                .lock()
                .map_err(|_| invalid_annotation("annotation edit baseline lock poisoned"))?;
            for baseline in baselines
                .values_mut()
                .filter(|baseline| baseline.project_id == project_id)
            {
                if baseline.pending_links.remove(&link_id).is_some() {
                    baseline.updated_at_ms = chrono::Utc::now().timestamp_millis();
                    return Ok(true);
                }
            }
        }
        // Canvas-instance links are owned by their canvas's store, which may be
        // the global store even when the UI is working inside a project.
        // 选 store 的读取结果就是删除前的真实旧值，直接复用，不重新读一遍。
        let project_store = self.annotation_store(&project_id)?;
        let project_link = project_store
            .get_link(&link_id)
            .map_err(WriterError::from)?;
        let (store, existing_link) = if project_link.is_some() {
            (project_store, project_link)
        } else {
            let global_store = self.global_annotation_store();
            let global_link = global_store
                .get_link(&link_id)
                .map_err(WriterError::from)?;
            if global_link.is_some() {
                (global_store, global_link)
            } else {
                (project_store, None)
            }
        };
        let store_project_id = store.project_id().to_string();
        let before = existing_link
            .filter(|link| link.deleted_at_ms.is_none())
            .map(|link| {
                Self::annotation_snapshot(
                    link.updated_at_ms.to_string(),
                    format!("{:?}", link.target),
                )
            });
        let paths = store
            .apply_mutation(AnnotationMutation::Delete {
                kind: AnnotationKind::Link,
                scope: store.scope(),
                object_id: link_id.clone(),
                deleted_at_ms: chrono::Utc::now().timestamp_millis(),
            })
            .map_err(WriterError::from)?;
        let changed = !paths.is_empty();
        self.finish_annotation_store_write(&store_project_id, paths, "delete_object_link");
        if changed && !store_project_id.is_empty() {
            self.record_annotation_object(
                &store_project_id,
                crate::writing_stats::ContributionObjectType::ObjectLink,
                &link_id,
                crate::writing_stats::ContributionOperation::Delete,
                before,
                None,
            )?;
        }
        Ok(changed)
    }

    pub fn list_links_for_source(
        &self,
        source: AnnotationObjectRefDto,
    ) -> ApiResult<Vec<AnnotationObjectLinkDto>> {
        let source = object_ref_from_dto(source)?;
        let mut links = self
            .link_store_for_source(&source)?
            .list_links_for_source(&source)
            .map_err(WriterError::from)?;
        if matches!(source, ObjectRef::TextAnchor { .. }) {
            let baselines = self
                .annotation_edit_baselines
                .lock()
                .map_err(|_| invalid_annotation("annotation edit baseline lock poisoned"))?;
            for baseline in baselines
                .values()
                .filter(|baseline| Some(baseline.project_id.as_str()) == source.project_id())
            {
                links.extend(
                    baseline
                        .pending_links
                        .values()
                        .filter(|link| link.source == source)
                        .cloned(),
                );
            }
        }
        links.sort_by(|left, right| left.link_id.cmp(&right.link_id));
        Ok(links.into_iter().map(link_to_dto).collect())
    }

    pub fn find_backlinks(
        &self,
        target: AnnotationTargetDto,
    ) -> ApiResult<Vec<AnnotationObjectLinkDto>> {
        let target = link_target_from_dto(target)?;
        let mut links_by_id =
            find_annotation_backlinks(&self.app_data_root, &self.projects_root, &target)
                .map_err(WriterError::from)?
                .into_iter()
                .map(|link| (link.link_id.clone(), link))
                .collect::<HashMap<_, _>>();
        let baselines = self
            .annotation_edit_baselines
            .lock()
            .map_err(|_| invalid_annotation("annotation edit baseline lock poisoned"))?;
        for link in baselines
            .values()
            .flat_map(|baseline| baseline.pending_links.values())
        {
            // A pending update replaces the disk version by ID, including
            // when it has moved away from this target. Active draft links
            // appear immediately in backlinks before their chapter is saved.
            links_by_id.remove(&link.link_id);
            if link.deleted_at_ms.is_none() && link.target == target {
                links_by_id.insert(link.link_id.clone(), link.clone());
            }
        }
        let mut links = links_by_id.into_values().collect::<Vec<_>>();
        links.sort_by(|left, right| left.link_id.cmp(&right.link_id));
        Ok(links.into_iter().map(link_to_dto).collect())
    }

    pub fn get_text_anchor(
        &self,
        project_id: String,
        anchor_id: String,
    ) -> ApiResult<Option<AnnotationTextAnchorDto>> {
        safe_project_id(&project_id)?;
        if anchor_id.is_empty() {
            return Ok(None);
        }
        {
            let baselines = self
                .annotation_edit_baselines
                .lock()
                .map_err(|_| invalid_annotation("annotation edit baseline lock poisoned"))?;
            if let Some(baseline) = baselines.values().find(|baseline| {
                baseline.project_id == project_id
                    && baseline.anchors.iter().any(|anchor| {
                        anchor.anchor_id == anchor_id && anchor.deleted_at_ms.is_none()
                    })
                    && !baseline
                        .original_anchors
                        .iter()
                        .any(|anchor| anchor.anchor_id == anchor_id)
            }) {
                if let Some(anchor) = baseline
                    .anchors
                    .iter()
                    .find(|anchor| anchor.anchor_id == anchor_id)
                {
                    return anchor_to_dto(anchor.clone()).map(Some);
                }
            }
        }
        self.annotation_store(&project_id)?
            .get_text_anchor(&anchor_id)
            .map_err(WriterError::from)?
            .filter(|anchor| anchor.deleted_at_ms.is_none())
            .map(anchor_to_dto)
            .transpose()
    }

    pub fn list_text_anchors_for_chapter(
        &self,
        project_id: String,
        chapter_id: String,
    ) -> ApiResult<Vec<AnnotationTextAnchorDto>> {
        safe_project_id(&project_id)?;
        if chapter_id.is_empty() {
            return Err(invalid_annotation("chapter id must be non-empty"));
        }
        self.annotation_store(&project_id)?
            .list_text_anchors_for_chapter(&chapter_id)
            .map_err(WriterError::from)?
            .into_iter()
            .map(anchor_to_dto)
            .collect()
    }

    pub fn resolve_link(
        &self,
        target: AnnotationTargetDto,
    ) -> ApiResult<AnnotationLinkResolutionDto> {
        let target = link_target_from_dto(target)?;
        let state = match &target {
            LinkTarget::Object(object_ref) => {
                if let Some(project_id) = object_ref.project_id() {
                    let project_root = self.projects_root.join(project_id);
                    if !project_root.join("project.json").exists() {
                        AnnotationLinkResolutionStateDto::ProjectNotDownloaded
                    } else if self.object_ref_exists(object_ref)? {
                        AnnotationLinkResolutionStateDto::Exists
                    } else {
                        AnnotationLinkResolutionStateDto::Missing
                    }
                } else if self.object_ref_exists(object_ref)? {
                    AnnotationLinkResolutionStateDto::Exists
                } else {
                    AnnotationLinkResolutionStateDto::Missing
                }
            }
            LinkTarget::CanvasNavigation(route) => {
                let meta_path = self
                    .app_data_root
                    .join("starmaps")
                    .join(format!("{}.meta.json", route.starmap_id));
                if !route.starmap_id.is_empty() && meta_path.is_file() {
                    AnnotationLinkResolutionStateDto::Exists
                } else {
                    AnnotationLinkResolutionStateDto::Missing
                }
            }
        };
        Ok(AnnotationLinkResolutionDto {
            state,
            target: link_target_to_dto(target),
        })
    }

    fn object_ref_exists(&self, target: &ObjectRef) -> ApiResult<bool> {
        if let ObjectRef::TextAnchor {
            project_id,
            object_id,
        } = target
        {
            let baselines = self
                .annotation_edit_baselines
                .lock()
                .map_err(|_| invalid_annotation("annotation edit baseline lock poisoned"))?;
            if baselines
                .values()
                .any(|baseline| is_session_created_anchor(baseline, project_id, object_id))
            {
                return Ok(true);
            }
        }
        match target {
            ObjectRef::Project {
                project_id,
                object_id,
            } => {
                if !self
                    .projects_root
                    .join(project_id)
                    .join("project.json")
                    .is_file()
                {
                    return Ok(false);
                }
                let project = crate::project::get_project(&self.projects_root, project_id)
                    .map_err(WriterError::from)?;
                Ok(project.id == *object_id)
            }
            ObjectRef::Volume {
                project_id,
                object_id,
            } => {
                if !self.project_is_downloaded(project_id) {
                    return Ok(false);
                }
                Ok(self
                    .core_read()
                    .list_volumes(project_id)
                    .map_err(WriterError::from)?
                    .iter()
                    .any(|volume| volume.id == *object_id))
            }
            ObjectRef::Chapter {
                project_id,
                object_id,
            } => {
                if !self.project_is_downloaded(project_id) {
                    return Ok(false);
                }
                let core = self.core_read();
                let volumes = core.list_volumes(project_id).map_err(WriterError::from)?;
                for volume in volumes {
                    if core
                        .list_chapters(project_id, &volume.id)
                        .map_err(WriterError::from)?
                        .iter()
                        .any(|chapter| chapter.id == *object_id)
                    {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            ObjectRef::TextAnchor {
                project_id,
                object_id,
            } => {
                if !self.project_is_downloaded(project_id) {
                    return Ok(false);
                }
                Ok(self
                    .annotation_store(project_id)?
                    .get_text_anchor(object_id)
                    .map_err(WriterError::from)?
                    .is_some_and(|anchor| anchor.deleted_at_ms.is_none()))
            }
            ObjectRef::NamedEntry {
                project_id,
                object_id,
            } => {
                if !self.project_is_downloaded(project_id) {
                    return Ok(false);
                }
                Ok(self
                    .annotation_store(project_id)?
                    .get_named_entry(object_id)
                    .map_err(WriterError::from)?
                    .is_some_and(|entry| entry.deleted_at_ms.is_none()))
            }
            ObjectRef::StarMap { scope, object_id } => {
                let meta_path = self
                    .app_data_root
                    .join("starmaps")
                    .join(format!("{object_id}.meta.json"));
                if !meta_path.is_file() {
                    return Ok(false);
                }
                self.get_starmap(object_id)
                    .map(|meta| meta.project_id.as_deref() == scope.project_id())
            }
            ObjectRef::NodeAsset { object_id, .. } => Ok(
                crate::starmap::library::load_node_definition(&self.app_data_root, object_id)?
                    .is_some(),
            ),
            ObjectRef::NodeInstance {
                starmap_id,
                object_id,
            } => {
                if !self.starmap_meta_exists(starmap_id) {
                    return Ok(false);
                }
                Ok(self
                    .get_starmap_graph(starmap_id)?
                    .nodes
                    .iter()
                    .any(|node| node.id == *object_id))
            }
            ObjectRef::EmbedInstance {
                starmap_id,
                object_id,
            } => {
                if !self.starmap_meta_exists(starmap_id) {
                    return Ok(false);
                }
                Ok(self
                    .get_starmap_graph(starmap_id)?
                    .embeds
                    .iter()
                    .any(|embed| embed.instance_id == *object_id))
            }
            ObjectRef::CanvasAnchor {
                starmap_id,
                object_id,
                anchor_id,
            } => {
                if !self.starmap_meta_exists(starmap_id) {
                    return Ok(false);
                }
                // 锚点属于共享素材内容，多个画布实例共享同一端口定义。
                Ok(
                    crate::starmap::library::load_node_definition(&self.app_data_root, object_id)?
                        .is_some_and(|definition| {
                            definition
                                .anchors
                                .iter()
                                .any(|anchor| anchor.anchor_id == *anchor_id)
                        }),
                )
            }
        }
    }

    fn project_is_downloaded(&self, project_id: &str) -> bool {
        self.projects_root
            .join(project_id)
            .join("project.json")
            .is_file()
    }

    fn starmap_meta_exists(&self, starmap_id: &str) -> bool {
        self.app_data_root
            .join("starmaps")
            .join(format!("{starmap_id}.meta.json"))
            .is_file()
    }

    pub fn get_chapter_annotations(
        &self,
        project_id: String,
        chapter_id: String,
        text_revision: u64,
        text: String,
        baseline_id: String,
    ) -> ApiResult<ChapterAnnotationsDto> {
        let store = self.annotation_store(&project_id)?;
        let now = chrono::Utc::now().timestamp_millis();
        let stored_anchor_records = store
            .list_text_anchor_records()
            .map_err(WriterError::from)?
            .into_iter()
            .filter(|anchor| anchor.chapter_id == chapter_id)
            .collect::<Vec<_>>();
        let stored_anchors = stored_anchor_records
            .iter()
            .filter(|anchor| anchor.deleted_at_ms.is_none())
            .cloned()
            .collect::<Vec<_>>();
        let text_hash = format!("{:x}", md5::compute(text.as_bytes()));
        let use_session_baseline = !baseline_id.is_empty();
        let baseline_id = if use_session_baseline {
            baseline_id
        } else {
            uuid::Uuid::new_v4().to_string()
        };
        let mut projected_anchors = Vec::new();
        let mut session_bindings = Vec::new();
        let mut session_links = Vec::new();
        if use_session_baseline {
            let mut baselines = self
                .annotation_edit_baselines
                .lock()
                .map_err(|_| invalid_annotation("annotation edit baseline lock poisoned"))?;
            let baseline = baselines.get_mut(&baseline_id).ok_or_else(|| {
                invalid_annotation("annotation edit baseline expired; reload chapter annotations")
            })?;
            if baseline.project_id != project_id
                || baseline.chapter_id != chapter_id
                || baseline.text_revision != text_revision
                || baseline.text_len != text.len()
            {
                return Err(invalid_annotation(
                    "annotation edit baseline does not match the current chapter snapshot",
                ));
            }
            for original in &baseline.original_anchors {
                if !stored_anchor_records
                    .iter()
                    .any(|stored| stored == original)
                {
                    return Err(invalid_annotation(
                        "annotation anchors changed outside this edit session; reload chapter annotations",
                    ));
                }
            }
            for stored in &stored_anchors {
                if baseline
                    .original_anchors
                    .iter()
                    .any(|original| original.anchor_id == stored.anchor_id)
                {
                    continue;
                }
                if let Some(projected) = rebase_text_anchor_with_hash(stored, &text, &text_hash)
                    .map_err(WriterError::from)?
                {
                    baseline.original_anchors.push(stored.clone());
                    baseline.anchors.push(projected);
                }
            }
            projected_anchors = baseline
                .anchors
                .iter()
                .filter(|anchor| anchor.deleted_at_ms.is_none())
                .cloned()
                .collect();
            for anchor in &mut projected_anchors {
                capture_text_anchor_with_hash(anchor, &text, &text_hash)
                    .map_err(WriterError::from)?;
            }
            session_bindings = baseline.pending_bindings.values().cloned().collect();
            session_links = baseline.pending_links.values().cloned().collect();
        } else {
            for anchor in &stored_anchors {
                let Some(current) = rebase_text_anchor_with_hash(anchor, &text, &text_hash)
                    .map_err(WriterError::from)?
                else {
                    continue;
                };
                projected_anchors.push(current);
            }
            let mut baselines = self
                .annotation_edit_baselines
                .lock()
                .map_err(|_| invalid_annotation("annotation edit baseline lock poisoned"))?;
            if baselines.len() >= 2_048 {
                if let Some(oldest) = baselines
                    .iter()
                    .min_by_key(|(_, baseline)| baseline.updated_at_ms)
                    .map(|(id, _)| id.clone())
                {
                    baselines.remove(&oldest);
                }
            }
            baselines.insert(
                baseline_id.clone(),
                AnnotationEditBaseline {
                    project_id: project_id.clone(),
                    chapter_id: chapter_id.clone(),
                    text_revision,
                    text_len: text.len(),
                    original_anchors: stored_anchors,
                    anchors: projected_anchors.clone(),
                    pending_bindings: HashMap::new(),
                    pending_links: HashMap::new(),
                    history: AnnotationEditHistory::default(),
                    updated_at_ms: now,
                },
            );
        }
        let anchors = projected_anchors;

        // Read each project collection once. Per-anchor list calls would rescan
        // the same directories for every mark in a chapter.
        let all_bindings = store.list_bindings().map_err(WriterError::from)?;
        let all_links = store.list_links().map_err(WriterError::from)?;
        let mut bindings_by_anchor = HashMap::<String, Vec<TagBinding>>::new();
        for binding in all_bindings {
            if let ObjectRef::TextAnchor {
                project_id: binding_project_id,
                object_id: anchor_id,
            } = &binding.target
            {
                if binding_project_id == &project_id {
                    bindings_by_anchor
                        .entry(anchor_id.clone())
                        .or_default()
                        .push(binding);
                }
            }
        }
        let mut links_by_anchor = HashMap::<String, Vec<ObjectLink>>::new();
        for link in all_links {
            if let ObjectRef::TextAnchor {
                project_id: source_project_id,
                object_id: anchor_id,
            } = &link.source
            {
                if source_project_id == &project_id {
                    links_by_anchor
                        .entry(anchor_id.clone())
                        .or_default()
                        .push(link);
                }
            }
        }
        for binding in session_bindings {
            if let ObjectRef::TextAnchor {
                project_id: binding_project_id,
                object_id: anchor_id,
            } = &binding.target
            {
                if binding_project_id == &project_id {
                    bindings_by_anchor
                        .entry(anchor_id.clone())
                        .or_default()
                        .retain(|stored| stored.binding_id != binding.binding_id);
                    bindings_by_anchor
                        .entry(anchor_id.clone())
                        .or_default()
                        .push(binding);
                }
            }
        }
        for link in session_links {
            if let ObjectRef::TextAnchor {
                project_id: source_project_id,
                object_id: anchor_id,
            } = &link.source
            {
                if source_project_id == &project_id {
                    links_by_anchor
                        .entry(anchor_id.clone())
                        .or_default()
                        .retain(|stored| stored.link_id != link.link_id);
                    links_by_anchor
                        .entry(anchor_id.clone())
                        .or_default()
                        .push(link);
                }
            }
        }
        let mut bindings = Vec::new();
        let mut links = Vec::new();
        for anchor in &anchors {
            if let Some(anchor_bindings) = bindings_by_anchor.remove(&anchor.anchor_id) {
                bindings.extend(anchor_bindings);
            }
            if let Some(anchor_links) = links_by_anchor.remove(&anchor.anchor_id) {
                links.extend(anchor_links);
            }
        }
        let auto_matches = self.auto_matches_for_chapter(
            &project_id,
            &chapter_id,
            text_revision,
            &text,
            &store,
            &anchors,
            &links,
        )?;

        Ok(ChapterAnnotationsDto {
            project_id,
            chapter_id,
            baseline_id,
            text_revision,
            anchors: anchors
                .into_iter()
                .map(anchor_to_dto)
                .collect::<ApiResult<Vec<_>>>()?,
            bindings: bindings.into_iter().map(binding_to_dto).collect(),
            links: links.into_iter().map(link_to_dto).collect(),
            auto_matches,
        })
    }

    pub fn find_auto_name_links(
        &self,
        project_id: String,
        chapter_id: String,
        text_revision: u64,
        text: String,
    ) -> ApiResult<Vec<AnnotationAutoMatchDto>> {
        self.ensure_named_entries_migrated(&project_id)?;
        let store = self.annotation_store(&project_id)?;
        let anchors = store
            .list_text_anchors_for_chapter(&chapter_id)
            .map_err(WriterError::from)?;
        let text_hash = format!("{:x}", md5::compute(text.as_bytes()));
        let mut rebased_anchors = Vec::new();
        for anchor in &anchors {
            if let Some(rebased) = rebase_text_anchor_with_hash(anchor, &text, &text_hash)
                .map_err(WriterError::from)?
            {
                rebased_anchors.push(rebased);
            }
        }
        let links = store.list_links().map_err(WriterError::from)?;
        self.auto_matches_for_chapter(
            &project_id,
            &chapter_id,
            text_revision,
            &text,
            &store,
            &rebased_anchors,
            &links,
        )
    }

    fn auto_matches_for_chapter(
        &self,
        project_id: &str,
        chapter_id: &str,
        text_revision: u64,
        text: &str,
        store: &AnnotationStore,
        anchors: &[TextAnchor],
        links: &[ObjectLink],
    ) -> ApiResult<Vec<AnnotationAutoMatchDto>> {
        let entries = store.list_named_entries().map_err(WriterError::from)?;
        let manual_ranges = anchors
            .iter()
            .filter(|anchor| {
                links.iter().any(|link| {
                    link.origin == LinkOrigin::Manual && link.source == anchor.object_ref()
                })
            })
            .map(|anchor| (anchor.start_byte, anchor.end_byte))
            .collect::<Vec<_>>();
        let snapshot = self.core_read().auto_link_matches(
            project_id,
            chapter_id,
            text_revision,
            text,
            &entries,
            &manual_ranges,
        );
        Ok(snapshot
            .matches
            .into_iter()
            .map(|matched| AnnotationAutoMatchDto {
                start_byte: matched.start_byte as u32,
                end_byte: matched.end_byte as u32,
                matched_text: matched.matched_text,
                entry_ids: matched.candidate_entry_ids.clone(),
                candidates: matched
                    .candidate_entry_ids
                    .into_iter()
                    .map(|entry_id| AnnotationObjectRefDto {
                        kind: AnnotationObjectKindDto::NamedEntry,
                        project_id: Some(project_id.to_string()),
                        starmap_id: None,
                        object_id: entry_id,
                        anchor_id: None,
                    })
                    .collect(),
            })
            .collect())
    }

    pub fn apply_annotation_edits(
        &self,
        project_id: String,
        chapter_id: String,
        baseline_id: String,
        base_revision: u64,
        text_revision: u64,
        cause: EditorTransactionCauseDto,
        edits: Vec<AnnotationEditDto>,
    ) -> ApiResult<bool> {
        safe_project_id(&project_id)?;
        if cause == EditorTransactionCauseDto::Load {
            return Ok(false);
        }
        if chapter_id.is_empty() {
            return Ok(false);
        }
        if base_revision == text_revision && edits.is_empty() {
            return Ok(false);
        }
        if base_revision == text_revision {
            return Err(invalid_annotation(
                "annotation edit ranges changed text without advancing the editor revision",
            ));
        }
        if text_revision < base_revision {
            return Err(invalid_annotation(
                "new text revision precedes the edit base",
            ));
        }
        let mut edits = edits
            .into_iter()
            .map(|edit| TextEditRange {
                old_start_byte: edit.old_start_byte as usize,
                old_end_byte: edit.old_end_byte as usize,
                new_start_byte: edit.new_start_byte as usize,
                new_end_byte: edit.new_end_byte as usize,
            })
            .collect::<Vec<_>>();
        edits.sort_by_key(|edit| (edit.old_start_byte, edit.old_end_byte, edit.new_start_byte));
        let now = chrono::Utc::now().timestamp_millis();
        let mut baselines = self
            .annotation_edit_baselines
            .lock()
            .map_err(|_| invalid_annotation("annotation edit baseline lock poisoned"))?;
        let baseline = baselines.get_mut(&baseline_id).ok_or_else(|| {
            invalid_annotation("annotation edit baseline expired; reload chapter annotations")
        })?;
        if baseline.project_id != project_id
            || baseline.chapter_id != chapter_id
            || baseline.text_revision != base_revision
        {
            return Err(invalid_annotation(
                "annotation edit does not match its loaded text baseline",
            ));
        }
        let next_text_len =
            resulting_text_len(baseline.text_len, &edits).map_err(WriterError::from)?;
        if edits.is_empty() {
            baseline.text_revision = text_revision;
            baseline.updated_at_ms = now;
            return Ok(false);
        }
        let previous_len = baseline.text_len;
        let (next_anchors, changed) = match cause {
            EditorTransactionCauseDto::Undo => {
                let history = baseline.history.undo.pop();
                if let Some(entry) = history {
                    if entry.text_len != next_text_len {
                        baseline.history.undo.push(entry);
                        return Err(invalid_annotation(
                            "annotation undo history does not match the editor transaction",
                        ));
                    }
                    let current_anchors = entry.anchors.as_ref().map(|_| baseline.anchors.clone());
                    baseline.history.redo.push(AnnotationEditHistoryEntry {
                        anchors: current_anchors,
                        text_len: previous_len,
                    });
                    let next = entry.anchors.unwrap_or_else(|| baseline.anchors.clone());
                    let changed = next != baseline.anchors;
                    (next, changed)
                } else {
                    map_session_anchors(&baseline.anchors, &edits, now)?
                }
            }
            EditorTransactionCauseDto::Redo => {
                let history = baseline.history.redo.pop();
                if let Some(entry) = history {
                    if entry.text_len != next_text_len {
                        baseline.history.redo.push(entry);
                        return Err(invalid_annotation(
                            "annotation redo history does not match the editor transaction",
                        ));
                    }
                    let current_anchors = entry.anchors.as_ref().map(|_| baseline.anchors.clone());
                    baseline.history.undo.push(AnnotationEditHistoryEntry {
                        anchors: current_anchors,
                        text_len: previous_len,
                    });
                    let next = entry.anchors.unwrap_or_else(|| baseline.anchors.clone());
                    let changed = next != baseline.anchors;
                    (next, changed)
                } else {
                    map_session_anchors(&baseline.anchors, &edits, now)?
                }
            }
            _ => {
                let (next, changed) = map_session_anchors(&baseline.anchors, &edits, now)?;
                baseline.history.undo.push(AnnotationEditHistoryEntry {
                    anchors: changed.then(|| baseline.anchors.clone()),
                    text_len: previous_len,
                });
                if baseline.history.undo.len() > 512 {
                    baseline.history.undo.remove(0);
                }
                baseline.history.redo.clear();
                (next, changed)
            }
        };
        baseline.anchors = next_anchors;
        baseline.text_len = next_text_len;
        baseline.text_revision = text_revision;
        baseline.updated_at_ms = now;
        Ok(changed)
    }

    pub(crate) fn prepare_annotation_edit_save(
        &self,
        project_id: &str,
        chapter_id: &str,
        content: &str,
    ) -> ApiResult<Option<PreparedAnnotationEditSave>> {
        let text_hash = format!("{:x}", md5::compute(content.as_bytes()));
        let baseline = {
            let baselines = self
                .annotation_edit_baselines
                .lock()
                .map_err(|_| invalid_annotation("annotation edit baseline lock poisoned"))?;
            baselines
                .iter()
                .filter(|(_, baseline)| {
                    baseline.project_id == project_id
                        && baseline.chapter_id == chapter_id
                        && annotation_baseline_is_dirty(baseline, &text_hash)
                })
                .max_by_key(|(_, baseline)| baseline.updated_at_ms)
                .map(|(id, baseline)| (id.clone(), baseline.clone()))
        };
        let Some((baseline_id, baseline)) = baseline else {
            return self.prepare_saved_text_anchor_rebase(project_id, chapter_id, content);
        };
        if baseline.text_len != content.len() {
            return Err(invalid_annotation(
                "chapter save does not match the active annotation edit session",
            ));
        }
        let store = self.annotation_store(project_id)?;
        let stored_records = store
            .list_text_anchor_records()
            .map_err(WriterError::from)?
            .into_iter()
            .filter(|anchor| anchor.chapter_id == chapter_id)
            .collect::<Vec<_>>();
        for original in &baseline.original_anchors {
            if !stored_records.iter().any(|stored| stored == original) {
                return Err(invalid_annotation(
                    "annotation anchors changed outside this edit session; reload chapter annotations",
                ));
            }
        }
        let mut original_anchors = baseline.original_anchors.clone();
        let mut current_anchors = baseline.anchors.clone();
        for stored in stored_records
            .iter()
            .filter(|anchor| anchor.deleted_at_ms.is_none())
        {
            if original_anchors
                .iter()
                .any(|original| original.anchor_id == stored.anchor_id)
            {
                continue;
            }
            original_anchors.push(stored.clone());
            if let Some(projected) = rebase_text_anchor_with_hash(stored, content, &text_hash)
                .map_err(WriterError::from)?
            {
                current_anchors.push(projected);
            }
        }
        let prepared = store
            .prepare_text_anchor_changes(
                &original_anchors,
                &current_anchors,
                &baseline
                    .pending_bindings
                    .values()
                    .cloned()
                    .collect::<Vec<_>>(),
                &baseline.pending_links.values().cloned().collect::<Vec<_>>(),
                content,
                chrono::Utc::now().timestamp_millis(),
            )
            .map_err(WriterError::from)?;
        if prepared.files.is_empty() {
            return Ok(None);
        }
        Ok(Some(PreparedAnnotationEditSave {
            baseline_id: Some(baseline_id),
            prepared,
            text_revision: baseline.text_revision,
            chapter_text_hash: text_hash,
            pending_bindings: baseline.pending_bindings.values().cloned().collect(),
            pending_links: baseline.pending_links.values().cloned().collect(),
        }))
    }

    fn prepare_saved_text_anchor_rebase(
        &self,
        project_id: &str,
        chapter_id: &str,
        content: &str,
    ) -> ApiResult<Option<PreparedAnnotationEditSave>> {
        let store = self.annotation_store(project_id)?;
        let original_anchors = store
            .list_text_anchors_for_chapter(chapter_id)
            .map_err(WriterError::from)?;
        if original_anchors.is_empty() {
            return Ok(None);
        }
        let text_hash = format!("{:x}", md5::compute(content.as_bytes()));
        let mut current_anchors = Vec::new();
        for anchor in &original_anchors {
            if let Some(anchor) = rebase_text_anchor_with_hash(anchor, content, &text_hash)
                .map_err(WriterError::from)?
            {
                current_anchors.push(anchor);
            }
        }
        let prepared = store
            .prepare_text_anchor_changes(
                &original_anchors,
                &current_anchors,
                &[],
                &[],
                content,
                chrono::Utc::now().timestamp_millis(),
            )
            .map_err(WriterError::from)?;
        if prepared.files.is_empty() {
            return Ok(None);
        }
        Ok(Some(PreparedAnnotationEditSave {
            baseline_id: None,
            prepared,
            text_revision: 0,
            chapter_text_hash: text_hash,
            pending_bindings: Vec::new(),
            pending_links: Vec::new(),
        }))
    }

    pub(crate) fn finish_annotation_edit_save(
        &self,
        pending: &PreparedAnnotationEditSave,
    ) -> ApiResult<()> {
        let Some(baseline_id) = pending.baseline_id.as_deref() else {
            return Ok(());
        };
        let mut baselines = self.annotation_edit_baselines.lock().map_err(|_| {
            invalid_annotation("annotation edit baseline lock poisoned after chapter save")
        })?;
        let Some(session_key) = baselines
            .get(baseline_id)
            .map(|baseline| (baseline.project_id.clone(), baseline.chapter_id.clone()))
        else {
            return Ok(());
        };
        baselines.retain(|id, baseline| {
            id == baseline_id
                || baseline.project_id.as_str() != session_key.0.as_str()
                || baseline.chapter_id.as_str() != session_key.1.as_str()
                || annotation_baseline_is_dirty(baseline, &pending.chapter_text_hash)
        });
        let Some(baseline) = baselines.get_mut(baseline_id) else {
            return Ok(());
        };
        let context = crate::writing_stats::current_mutation_context();
        let project_id = session_key.0.clone();
        let mut committed_objects = Vec::new();
        for committed in &pending.prepared.committed_anchors {
            let original = baseline
                .original_anchors
                .iter()
                .find(|anchor| anchor.anchor_id == committed.anchor_id);
            let operation = if original.is_none() {
                crate::writing_stats::ContributionOperation::Create
            } else if committed.deleted_at_ms.is_some() {
                crate::writing_stats::ContributionOperation::Delete
            } else {
                crate::writing_stats::ContributionOperation::Update
            };
            let before = original.map(|anchor| {
                Self::annotation_snapshot(
                    anchor.updated_at_ms.to_string(),
                    format!(
                        "{}:{}..{}",
                        anchor.chapter_id, anchor.start_byte, anchor.end_byte
                    ),
                )
            });
            let after = committed.deleted_at_ms.is_none().then(|| {
                Self::annotation_snapshot(
                    committed.updated_at_ms.to_string(),
                    format!(
                        "{}:{}..{}",
                        committed.chapter_id, committed.start_byte, committed.end_byte
                    ),
                )
            });
            committed_objects.push((
                crate::writing_stats::ContributionObjectType::TextAnchor,
                committed.anchor_id.clone(),
                operation,
                before,
                after,
            ));
        }
        for binding in &pending.pending_bindings {
            committed_objects.push((
                crate::writing_stats::ContributionObjectType::TagBinding,
                binding.binding_id.clone(),
                crate::writing_stats::ContributionOperation::Link,
                None,
                Some(Self::annotation_snapshot(
                    binding.updated_at_ms.to_string(),
                    binding.tag_id.clone(),
                )),
            ));
        }
        for link in &pending.pending_links {
            committed_objects.push((
                crate::writing_stats::ContributionObjectType::ObjectLink,
                link.link_id.clone(),
                crate::writing_stats::ContributionOperation::Create,
                None,
                Some(Self::annotation_snapshot(
                    link.updated_at_ms.to_string(),
                    format!("{:?}", link.target),
                )),
            ));
        }
        for committed in &pending.prepared.committed_anchors {
            if let Some(original) = baseline
                .original_anchors
                .iter_mut()
                .find(|anchor| anchor.anchor_id == committed.anchor_id)
            {
                *original = committed.clone();
            } else {
                baseline.original_anchors.push(committed.clone());
            }
            if baseline.text_revision == pending.text_revision {
                if let Some(current) = baseline
                    .anchors
                    .iter_mut()
                    .find(|anchor| anchor.anchor_id == committed.anchor_id)
                {
                    *current = committed.clone();
                } else {
                    baseline.anchors.push(committed.clone());
                }
            }
        }
        for binding in &pending.pending_bindings {
            if baseline.pending_bindings.get(&binding.binding_id) == Some(binding) {
                baseline.pending_bindings.remove(&binding.binding_id);
            }
        }
        for link in &pending.pending_links {
            if baseline.pending_links.get(&link.link_id) == Some(link) {
                baseline.pending_links.remove(&link.link_id);
            }
        }
        drop(baselines);
        let mut contribution_batch = self.begin_contribution_batch();
        for (object_type, object_id, operation, before, after) in committed_objects {
            self.record_annotation_object_with_context(
                &context,
                &project_id,
                object_type,
                &object_id,
                operation,
                before,
                after,
            )?;
        }
        self.commit_contribution_batch(&mut contribution_batch)?;
        Ok(())
    }
}

fn text_anchor_id(target: &ObjectRef) -> Option<&str> {
    match target {
        ObjectRef::TextAnchor { object_id, .. } => Some(object_id),
        _ => None,
    }
}

fn is_session_created_anchor(
    baseline: &AnnotationEditBaseline,
    project_id: &str,
    anchor_id: &str,
) -> bool {
    baseline.project_id == project_id
        && baseline
            .anchors
            .iter()
            .any(|anchor| anchor.anchor_id == anchor_id && anchor.deleted_at_ms.is_none())
        && !baseline
            .original_anchors
            .iter()
            .any(|anchor| anchor.anchor_id == anchor_id)
}

fn find_session_created_anchor_baseline_mut<'a>(
    baselines: &'a mut HashMap<String, AnnotationEditBaseline>,
    project_id: &str,
    anchor_id: &str,
) -> Option<&'a mut AnnotationEditBaseline> {
    baselines
        .values_mut()
        .find(|baseline| is_session_created_anchor(baseline, project_id, anchor_id))
}

#[derive(Debug, Clone, Default)]
pub(super) struct AnnotationEditHistory {
    undo: Vec<AnnotationEditHistoryEntry>,
    redo: Vec<AnnotationEditHistoryEntry>,
}

#[derive(Debug, Clone)]
struct AnnotationEditHistoryEntry {
    anchors: Option<Vec<TextAnchor>>,
    text_len: usize,
}

#[derive(Debug, Clone)]
pub(super) struct AnnotationEditBaseline {
    project_id: String,
    chapter_id: String,
    text_revision: u64,
    text_len: usize,
    original_anchors: Vec<TextAnchor>,
    anchors: Vec<TextAnchor>,
    pending_bindings: HashMap<String, TagBinding>,
    pending_links: HashMap<String, ObjectLink>,
    history: AnnotationEditHistory,
    updated_at_ms: i64,
}

pub(crate) struct PreparedAnnotationEditSave {
    pub(crate) baseline_id: Option<String>,
    pub(crate) prepared: crate::annotations::PreparedTextAnchorChanges,
    text_revision: u64,
    chapter_text_hash: String,
    pending_bindings: Vec<TagBinding>,
    pending_links: Vec<ObjectLink>,
}

fn annotation_baseline_is_dirty(
    baseline: &AnnotationEditBaseline,
    chapter_text_hash: &str,
) -> bool {
    !baseline.pending_bindings.is_empty()
        || !baseline.pending_links.is_empty()
        || baseline.anchors.iter().any(|anchor| {
            if let Some(original) = baseline
                .original_anchors
                .iter()
                .find(|original| original.anchor_id == anchor.anchor_id)
            {
                original != anchor
                    || (anchor.deleted_at_ms.is_none() && anchor.text_hash != chapter_text_hash)
            } else {
                anchor.deleted_at_ms.is_none()
            }
        })
}

fn map_session_anchors(
    anchors: &[TextAnchor],
    edits: &[TextEditRange],
    updated_at_ms: i64,
) -> ApiResult<(Vec<TextAnchor>, bool)> {
    let mut mapped = Vec::with_capacity(anchors.len());
    let mut changed = false;
    for anchor in anchors {
        if anchor.deleted_at_ms.is_some() {
            mapped.push(anchor.clone());
            continue;
        }
        let next = match map_text_anchor_after_edits(anchor, edits, updated_at_ms)
            .map_err(WriterError::from)?
        {
            TextAnchorEdit::Updated(anchor) | TextAnchorEdit::Deleted(anchor) => anchor,
        };
        changed |= next != *anchor;
        mapped.push(next);
    }
    Ok((mapped, changed))
}

fn validate_named_entry(
    canonical_name: String,
    aliases: Vec<String>,
    confirmed_misspellings: Vec<String>,
) -> ApiResult<(String, Vec<String>, Vec<String>)> {
    let canonical_name = canonical_name.trim().to_string();
    if canonical_name.is_empty() || canonical_name.chars().any(char::is_control) {
        return Err(invalid_annotation("canonical name must be non-empty text"));
    }
    let mut seen = HashSet::new();
    seen.insert(canonical_name.clone());
    let aliases = normalize_name_variants(aliases, &mut seen)?;
    let confirmed_misspellings = normalize_name_variants(confirmed_misspellings, &mut seen)?;
    Ok((canonical_name, aliases, confirmed_misspellings))
}

fn normalize_name_variants(
    names: Vec<String>,
    seen: &mut HashSet<String>,
) -> ApiResult<Vec<String>> {
    let mut normalized = Vec::with_capacity(names.len());
    for name in names {
        let name = name.trim().to_string();
        if name.is_empty() || name.chars().any(char::is_control) {
            return Err(invalid_annotation("name variants must be non-empty text"));
        }
        if seen.insert(name.clone()) {
            normalized.push(name);
        }
    }
    Ok(normalized)
}
