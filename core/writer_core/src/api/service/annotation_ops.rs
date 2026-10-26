use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::annotations::{
    find_backlinks as find_annotation_backlinks, AnnotationKind, AnnotationMutation,
    AnnotationStore, LinkOrigin, LinkTarget, NamedEntry, ObjectLink, ObjectRef, Tag, TagBinding,
    TextAnchor, TextAnchorEdit, TextEditRange,
};
use crate::api::service::{ApiResult, WriterCoreApi};
use crate::api::types::*;
use crate::api::WriterError;

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
    safe_project_id(&value.project_id)?;
    if value.object_id.is_empty() {
        return Err(invalid_annotation("object id must be non-empty"));
    }
    let project_id = value.project_id;
    let object_id = value.object_id;
    Ok(match value.kind {
        AnnotationObjectKindDto::Project => ObjectRef::Project {
            project_id,
            object_id,
        },
        AnnotationObjectKindDto::Volume => ObjectRef::Volume {
            project_id,
            object_id,
        },
        AnnotationObjectKindDto::Chapter => ObjectRef::Chapter {
            project_id,
            object_id,
        },
        AnnotationObjectKindDto::TextAnchor => ObjectRef::TextAnchor {
            project_id,
            object_id,
        },
        AnnotationObjectKindDto::StarMap => ObjectRef::StarMap {
            project_id,
            object_id,
        },
        AnnotationObjectKindDto::StarMapNode => ObjectRef::StarMapNode {
            project_id,
            object_id,
        },
        AnnotationObjectKindDto::StarMapEmbed => ObjectRef::StarMapEmbed {
            project_id,
            object_id,
        },
        AnnotationObjectKindDto::NamedEntry => ObjectRef::NamedEntry {
            project_id,
            object_id,
        },
    })
}

fn object_ref_to_dto(value: ObjectRef) -> AnnotationObjectRefDto {
    let (kind, project_id, object_id) = match value {
        ObjectRef::Project {
            project_id,
            object_id,
        } => (AnnotationObjectKindDto::Project, project_id, object_id),
        ObjectRef::Volume {
            project_id,
            object_id,
        } => (AnnotationObjectKindDto::Volume, project_id, object_id),
        ObjectRef::Chapter {
            project_id,
            object_id,
        } => (AnnotationObjectKindDto::Chapter, project_id, object_id),
        ObjectRef::TextAnchor {
            project_id,
            object_id,
        } => (AnnotationObjectKindDto::TextAnchor, project_id, object_id),
        ObjectRef::StarMap {
            project_id,
            object_id,
        } => (AnnotationObjectKindDto::StarMap, project_id, object_id),
        ObjectRef::StarMapNode {
            project_id,
            object_id,
        } => (AnnotationObjectKindDto::StarMapNode, project_id, object_id),
        ObjectRef::StarMapEmbed {
            project_id,
            object_id,
        } => (AnnotationObjectKindDto::StarMapEmbed, project_id, object_id),
        ObjectRef::NamedEntry {
            project_id,
            object_id,
        } => (AnnotationObjectKindDto::NamedEntry, project_id, object_id),
    };
    AnnotationObjectRefDto {
        kind,
        project_id,
        object_id,
    }
}

fn link_target_from_dto(value: AnnotationTargetDto) -> ApiResult<LinkTarget> {
    match (value.object_ref, value.external_url) {
        (Some(object_ref), None) => Ok(LinkTarget::Object(object_ref_from_dto(object_ref)?)),
        (None, Some(url)) => {
            let url = url.trim();
            if url.is_empty() || !(url.starts_with("https://") || url.starts_with("http://")) {
                return Err(invalid_annotation(
                    "external link must be an HTTP or HTTPS URL",
                ));
            }
            Ok(LinkTarget::ExternalUrl(url.to_string()))
        }
        _ => Err(invalid_annotation(
            "link target must contain exactly one object reference or external URL",
        )),
    }
}

fn link_target_to_dto(value: LinkTarget) -> AnnotationTargetDto {
    match value {
        LinkTarget::Object(object_ref) => AnnotationTargetDto {
            object_ref: Some(object_ref_to_dto(object_ref)),
            external_url: None,
        },
        LinkTarget::ExternalUrl(url) => AnnotationTargetDto {
            object_ref: None,
            external_url: Some(url),
        },
    }
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
        text_revision: value.text_revision,
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
        updated_at_ms: value.updated_at_ms,
        deleted_at_ms: value.deleted_at_ms,
    }
}

impl WriterCoreApi {
    pub(super) fn annotation_store(&self, project_id: &str) -> ApiResult<AnnotationStore> {
        safe_project_id(project_id)?;
        Ok(AnnotationStore::new(
            &self.projects_root.join(project_id),
            project_id,
        ))
    }

    pub(crate) fn ensure_named_entries_migrated(&self, project_id: &str) -> ApiResult<()> {
        safe_project_id(project_id)?;
        let correction = crate::correction::CorrectionStore::open(&self.app_data_root)?;
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
        for entry in &dictionary_entries {
            if matches!(
                entry.kind,
                CorrectionDictionaryEntryKindDto::Person | CorrectionDictionaryEntryKindDto::Place
            ) {
                grouped.entry(entry.text.clone()).or_default();
            }
        }

        let store = self.annotation_store(project_id)?;
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
        if !paths.is_empty() {
            self.finish_annotation_write(project_id, paths, "migrate_correction_named_entries");
        }
        if !legacy_names.is_empty() {
            correction.clear_legacy_entity_name_records(project_id)?;
        }
        if has_typed_names {
            correction.clear_legacy_typed_entity_dictionary_entries(project_id)?;
        }
        self.sync_correction_named_entries(project_id)?;
        Ok(())
    }

    pub(crate) fn sync_correction_named_entries(&self, project_id: &str) -> ApiResult<()> {
        let entries = self
            .annotation_store(project_id)?
            .list_named_entries()
            .map_err(WriterError::from)?
            .into_iter()
            .map(named_entry_to_dto)
            .collect();
        crate::correction::CorrectionStore::open(&self.app_data_root)?
            .set_named_entries_for_project(project_id, entries)?;
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
        let project_root = self.projects_root.join(project_id);
        let existing_ids = self.annotation_search_object_ids_for_project(project_id);
        match crate::search::extractor::annotation_search_updates_for_project(
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

    pub fn list_tags(&self, project_id: String) -> ApiResult<Vec<AnnotationTagDto>> {
        self.annotation_store(&project_id)?
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
        let paths = store
            .apply_mutation(AnnotationMutation::UpsertTag(tag))
            .map_err(WriterError::from)?;
        self.finish_annotation_write(&project_id, paths, "create_annotation_tag");
        Ok(result)
    }

    pub fn rename_tag(
        &self,
        project_id: String,
        tag_id: String,
        name: String,
    ) -> ApiResult<AnnotationTagDto> {
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
        tag.name = name.to_string();
        tag.updated_at_ms = chrono::Utc::now().timestamp_millis();
        let result = tag_to_dto(tag.clone());
        let paths = store
            .apply_mutation(AnnotationMutation::UpsertTag(tag))
            .map_err(WriterError::from)?;
        self.finish_annotation_write(&project_id, paths, "rename_annotation_tag");
        Ok(result)
    }

    pub fn delete_tag(&self, project_id: String, tag_id: String) -> ApiResult<bool> {
        let store = self.annotation_store(&project_id)?;
        let paths = store
            .apply_mutation(AnnotationMutation::Delete {
                kind: AnnotationKind::Tag,
                project_id: project_id.clone(),
                object_id: tag_id,
                deleted_at_ms: chrono::Utc::now().timestamp_millis(),
            })
            .map_err(WriterError::from)?;
        let changed = !paths.is_empty();
        self.finish_annotation_write(&project_id, paths, "delete_annotation_tag");
        Ok(changed)
    }

    pub fn merge_tags(
        &self,
        project_id: String,
        source_tag_id: String,
        target_tag_id: String,
    ) -> ApiResult<bool> {
        let store = self.annotation_store(&project_id)?;
        let paths = store
            .merge_tags(
                &source_tag_id,
                &target_tag_id,
                chrono::Utc::now().timestamp_millis(),
            )
            .map_err(WriterError::from)?;
        let changed = !paths.is_empty();
        self.finish_annotation_write(&project_id, paths, "merge_annotation_tags");
        Ok(changed)
    }

    pub fn set_tag_bindings(
        &self,
        project_id: String,
        target: AnnotationObjectRefDto,
        tag_ids: Vec<String>,
    ) -> ApiResult<Vec<AnnotationTagBindingDto>> {
        let target = object_ref_from_dto(target)?;
        if target.project_id() != project_id {
            return Err(invalid_annotation(
                "tag bindings must target objects in their project",
            ));
        }
        let store = self.annotation_store(&project_id)?;
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
        let existing = store
            .list_bindings_for_target(&target)
            .map_err(WriterError::from)?;
        let existing_ids = existing
            .iter()
            .map(|binding| binding.tag_id.clone())
            .collect::<HashSet<_>>();
        let now = chrono::Utc::now().timestamp_millis();
        let mut changed_paths = Vec::new();
        for binding in existing {
            if !desired.contains(&binding.tag_id) {
                changed_paths.extend(
                    store
                        .apply_mutation(AnnotationMutation::Delete {
                            kind: AnnotationKind::Binding,
                            project_id: project_id.clone(),
                            object_id: binding.binding_id,
                            deleted_at_ms: now,
                        })
                        .map_err(WriterError::from)?,
                );
            }
        }
        for tag_id in desired {
            if existing_ids.contains(&tag_id) {
                continue;
            }
            let binding = TagBinding {
                binding_id: uuid::Uuid::new_v4().to_string(),
                project_id: project_id.clone(),
                tag_id,
                target: target.clone(),
                updated_at_ms: now,
                deleted_at_ms: None,
            };
            changed_paths.extend(
                store
                    .apply_mutation(AnnotationMutation::UpsertBinding(binding))
                    .map_err(WriterError::from)?,
            );
        }
        self.finish_annotation_write(&project_id, changed_paths, "set_annotation_tag_bindings");
        store
            .list_bindings_for_target(&target)
            .map(|values| values.into_iter().map(binding_to_dto).collect())
            .map_err(Into::into)
    }

    pub fn list_bindings_for_target(
        &self,
        project_id: String,
        target: AnnotationObjectRefDto,
    ) -> ApiResult<Vec<AnnotationTagBindingDto>> {
        let target = object_ref_from_dto(target)?;
        self.annotation_store(&project_id)?
            .list_bindings_for_target(&target)
            .map(|values| values.into_iter().map(binding_to_dto).collect())
            .map_err(Into::into)
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
        let anchor = TextAnchor {
            anchor_id: uuid::Uuid::new_v4().to_string(),
            project_id: project_id.clone(),
            chapter_id,
            start_byte: start,
            end_byte: end,
            text_revision,
            updated_at_ms: chrono::Utc::now().timestamp_millis(),
            deleted_at_ms: None,
        };
        let result = anchor_to_dto(anchor.clone())?;
        let paths = store
            .apply_mutation(AnnotationMutation::UpsertTextAnchor(anchor))
            .map_err(WriterError::from)?;
        self.finish_annotation_write(&project_id, paths, "create_annotation_text_anchor");
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
        let paths = store
            .apply_mutation(AnnotationMutation::UpsertNamedEntry(entry))
            .map_err(WriterError::from)?;
        self.sync_correction_named_entries(&project_id)?;
        self.finish_annotation_write(&project_id, paths, "create_named_entry");
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
        self.ensure_named_entries_migrated(&project_id)?;
        let (canonical_name, aliases, confirmed_misspellings) =
            validate_named_entry(canonical_name, aliases, confirmed_misspellings)?;
        let store = self.annotation_store(&project_id)?;
        let mut entry = store
            .get_named_entry(&entry_id)
            .map_err(WriterError::from)?
            .filter(|entry| entry.deleted_at_ms.is_none())
            .ok_or_else(|| invalid_annotation("named entry does not exist"))?;
        entry.canonical_name = canonical_name;
        entry.aliases = aliases;
        entry.confirmed_misspellings = confirmed_misspellings;
        entry.auto_link_enabled = auto_link_enabled;
        entry.updated_at_ms = chrono::Utc::now().timestamp_millis();
        let result = named_entry_to_dto(entry.clone());
        let paths = store
            .apply_mutation(AnnotationMutation::UpsertNamedEntry(entry))
            .map_err(WriterError::from)?;
        self.sync_correction_named_entries(&project_id)?;
        self.finish_annotation_write(&project_id, paths, "update_named_entry");
        Ok(result)
    }

    pub fn delete_named_entry(&self, project_id: String, entry_id: String) -> ApiResult<bool> {
        self.ensure_named_entries_migrated(&project_id)?;
        let store = self.annotation_store(&project_id)?;
        let paths = store
            .apply_mutation(AnnotationMutation::Delete {
                kind: AnnotationKind::NamedEntry,
                project_id: project_id.clone(),
                object_id: entry_id,
                deleted_at_ms: chrono::Utc::now().timestamp_millis(),
            })
            .map_err(WriterError::from)?;
        let changed = !paths.is_empty();
        self.sync_correction_named_entries(&project_id)?;
        self.finish_annotation_write(&project_id, paths, "delete_named_entry");
        Ok(changed)
    }

    pub fn create_link(
        &self,
        source: AnnotationObjectRefDto,
        target: AnnotationTargetDto,
    ) -> ApiResult<AnnotationObjectLinkDto> {
        let source = object_ref_from_dto(source)?;
        let target = link_target_from_dto(target)?;
        if !self.object_ref_exists(&source)? {
            return Err(invalid_annotation("link source does not exist"));
        }
        let project_id = source.project_id().to_string();
        let store = self.annotation_store(&project_id)?;
        let link = ObjectLink {
            link_id: uuid::Uuid::new_v4().to_string(),
            source,
            target,
            origin: LinkOrigin::Manual,
            updated_at_ms: chrono::Utc::now().timestamp_millis(),
            deleted_at_ms: None,
        };
        let result = link_to_dto(link.clone());
        let paths = store
            .apply_mutation(AnnotationMutation::UpsertLink(link))
            .map_err(WriterError::from)?;
        self.finish_annotation_write(&project_id, paths, "create_object_link");
        Ok(result)
    }

    pub fn update_link(
        &self,
        project_id: String,
        link_id: String,
        source: AnnotationObjectRefDto,
        target: AnnotationTargetDto,
    ) -> ApiResult<AnnotationObjectLinkDto> {
        safe_project_id(&project_id)?;
        let source = object_ref_from_dto(source)?;
        if source.project_id() != project_id {
            return Err(invalid_annotation(
                "link source must belong to its storage project",
            ));
        }
        if !self.object_ref_exists(&source)? {
            return Err(invalid_annotation("link source does not exist"));
        }
        let store = self.annotation_store(&project_id)?;
        let mut link = store
            .get_link(&link_id)
            .map_err(WriterError::from)?
            .filter(|link| link.deleted_at_ms.is_none())
            .ok_or_else(|| invalid_annotation("link does not exist"))?;
        link.source = source;
        link.target = link_target_from_dto(target)?;
        link.updated_at_ms = chrono::Utc::now().timestamp_millis();
        let result = link_to_dto(link.clone());
        let paths = store
            .apply_mutation(AnnotationMutation::UpsertLink(link))
            .map_err(WriterError::from)?;
        self.finish_annotation_write(&project_id, paths, "update_object_link");
        Ok(result)
    }

    pub fn delete_link(&self, project_id: String, link_id: String) -> ApiResult<bool> {
        let store = self.annotation_store(&project_id)?;
        let paths = store
            .apply_mutation(AnnotationMutation::Delete {
                kind: AnnotationKind::Link,
                project_id: project_id.clone(),
                object_id: link_id,
                deleted_at_ms: chrono::Utc::now().timestamp_millis(),
            })
            .map_err(WriterError::from)?;
        let changed = !paths.is_empty();
        self.finish_annotation_write(&project_id, paths, "delete_object_link");
        Ok(changed)
    }

    pub fn list_links_for_source(
        &self,
        source: AnnotationObjectRefDto,
    ) -> ApiResult<Vec<AnnotationObjectLinkDto>> {
        let source = object_ref_from_dto(source)?;
        self.annotation_store(source.project_id())?
            .list_links_for_source(&source)
            .map(|values| values.into_iter().map(link_to_dto).collect())
            .map_err(Into::into)
    }

    pub fn find_backlinks(
        &self,
        target: AnnotationTargetDto,
    ) -> ApiResult<Vec<AnnotationObjectLinkDto>> {
        let LinkTarget::Object(target) = link_target_from_dto(target)? else {
            return Ok(Vec::new());
        };
        find_annotation_backlinks(&self.projects_root, &target)
            .map(|values| values.into_iter().map(link_to_dto).collect())
            .map_err(Into::into)
    }

    pub fn resolve_link(
        &self,
        target: AnnotationTargetDto,
    ) -> ApiResult<AnnotationLinkResolutionDto> {
        let target = link_target_from_dto(target)?;
        let state = match &target {
            LinkTarget::ExternalUrl(_) => AnnotationLinkResolutionStateDto::ExternalUrl,
            LinkTarget::Object(object_ref) => {
                let project_root = self.projects_root.join(object_ref.project_id());
                if !project_root.join("project.json").exists() {
                    AnnotationLinkResolutionStateDto::ProjectNotDownloaded
                } else if self.object_ref_exists(object_ref)? {
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
        let project_id = target.project_id();
        let project_root = self.projects_root.join(project_id);
        if !project_root.join("project.json").is_file() {
            return Ok(false);
        }
        match target {
            ObjectRef::Project { object_id, .. } => {
                let project = crate::project::get_project(&self.projects_root, project_id)
                    .map_err(WriterError::from)?;
                Ok(project.id == *object_id)
            }
            ObjectRef::Volume { object_id, .. } => Ok(self
                .core_read()
                .list_volumes(project_id)
                .map_err(WriterError::from)?
                .iter()
                .any(|volume| volume.id == *object_id)),
            ObjectRef::Chapter { object_id, .. } => {
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
            ObjectRef::TextAnchor { object_id, .. } => Ok(self
                .annotation_store(project_id)?
                .get_text_anchor(object_id)
                .map_err(WriterError::from)?
                .is_some_and(|anchor| anchor.deleted_at_ms.is_none())),
            ObjectRef::NamedEntry { object_id, .. } => Ok(self
                .annotation_store(project_id)?
                .get_named_entry(object_id)
                .map_err(WriterError::from)?
                .is_some_and(|entry| entry.deleted_at_ms.is_none())),
            ObjectRef::StarMap { object_id, .. } => {
                let meta_path = self
                    .app_data_root
                    .join("starmaps")
                    .join(format!("{object_id}.meta.json"));
                if !meta_path.is_file() {
                    return Ok(false);
                }
                self.get_starmap(object_id)
                    .map(|meta| meta.project_id.as_deref() == Some(project_id))
            }
            ObjectRef::StarMapNode { object_id, .. } => {
                for meta in self.list_starmaps_for_project(project_id)? {
                    if self
                        .get_starmap_graph(&meta.starmap_id)?
                        .nodes
                        .iter()
                        .any(|node| node.id == *object_id)
                    {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            ObjectRef::StarMapEmbed { object_id, .. } => {
                for meta in self.list_starmaps_for_project(project_id)? {
                    if self
                        .get_starmap_graph(&meta.starmap_id)?
                        .embeds
                        .iter()
                        .any(|embed| embed.instance_id == *object_id)
                    {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
        }
    }

    pub fn get_chapter_annotations(
        &self,
        project_id: String,
        chapter_id: String,
        text_revision: u64,
        text: String,
    ) -> ApiResult<ChapterAnnotationsDto> {
        let store = self.annotation_store(&project_id)?;
        let anchors = store
            .list_text_anchors_for_chapter(&chapter_id)
            .map_err(WriterError::from)?
            .into_iter()
            .filter(|anchor| {
                anchor.start_byte < anchor.end_byte
                    && anchor.end_byte <= text.len()
                    && text.is_char_boundary(anchor.start_byte)
                    && text.is_char_boundary(anchor.end_byte)
            })
            .collect::<Vec<_>>();
        let mut bindings = Vec::new();
        let mut links = Vec::new();
        for anchor in &anchors {
            let target = anchor.object_ref();
            bindings.extend(
                store
                    .list_bindings_for_target(&target)
                    .map_err(WriterError::from)?,
            );
            links.extend(
                store
                    .list_links_for_source(&target)
                    .map_err(WriterError::from)?,
            );
        }
        let auto_matches =
            self.find_auto_name_links(project_id.clone(), chapter_id.clone(), text_revision, text)?;
        Ok(ChapterAnnotationsDto {
            project_id,
            chapter_id,
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
        let entries = store.list_named_entries().map_err(WriterError::from)?;
        let anchors = store
            .list_text_anchors_for_chapter(&chapter_id)
            .map_err(WriterError::from)?;
        let manual_ranges = anchors
            .iter()
            .filter(|anchor| anchor.deleted_at_ms.is_none())
            .map(|anchor| (anchor.start_byte, anchor.end_byte))
            .collect::<Vec<_>>();
        let snapshot = self.core_read().auto_link_matches(
            &project_id,
            &chapter_id,
            text_revision,
            &text,
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
                        project_id: project_id.clone(),
                        object_id: entry_id,
                    })
                    .collect(),
            })
            .collect())
    }

    pub fn apply_annotation_edits(
        &self,
        project_id: String,
        chapter_id: String,
        base_revision: u64,
        text_revision: u64,
        cause: EditorTransactionCauseDto,
        edits: Vec<AnnotationEditDto>,
    ) -> ApiResult<bool> {
        safe_project_id(&project_id)?;
        if cause == EditorTransactionCauseDto::Load {
            return Ok(false);
        }
        if chapter_id.is_empty() || base_revision == text_revision || edits.is_empty() {
            return Ok(false);
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
        let store = self.annotation_store(&project_id)?;
        let now = chrono::Utc::now().timestamp_millis();
        let key = (project_id.clone(), chapter_id.clone());

        if matches!(
            cause,
            EditorTransactionCauseDto::Undo | EditorTransactionCauseDto::Redo
        ) {
            let target = {
                let mut histories = self
                    .annotation_edit_history
                    .lock()
                    .map_err(|_| invalid_annotation("annotation edit history lock poisoned"))?;
                let history = histories.entry(key.clone()).or_default();
                match cause {
                    EditorTransactionCauseDto::Undo => history.undo.pop(),
                    EditorTransactionCauseDto::Redo => history.redo.pop(),
                    _ => None,
                }
            };
            if let Some(target) = target {
                let current = snapshot_chapter_annotations(&store, &chapter_id)?;
                let changed_paths = match restore_chapter_annotations(
                    &store,
                    &project_id,
                    &chapter_id,
                    &target,
                    text_revision,
                    now,
                ) {
                    Ok(paths) => paths,
                    Err(error) => {
                        let mut histories = self.annotation_edit_history.lock().map_err(|_| {
                            invalid_annotation("annotation edit history lock poisoned")
                        })?;
                        let history = histories.entry(key).or_default();
                        match cause {
                            EditorTransactionCauseDto::Undo => history.undo.push(target),
                            EditorTransactionCauseDto::Redo => history.redo.push(target),
                            _ => {}
                        }
                        return Err(error);
                    }
                };
                let mut histories = self
                    .annotation_edit_history
                    .lock()
                    .map_err(|_| invalid_annotation("annotation edit history lock poisoned"))?;
                let history = histories.entry(key).or_default();
                match cause {
                    EditorTransactionCauseDto::Undo => history.redo.push(current),
                    EditorTransactionCauseDto::Redo => history.undo.push(current),
                    _ => {}
                }
                self.finish_annotation_write(
                    &project_id,
                    changed_paths,
                    "restore_annotation_edit_history",
                );
                return Ok(true);
            }
        }

        let before = snapshot_chapter_annotations(&store, &chapter_id)?;
        let anchors = store
            .list_text_anchors_for_chapter(&chapter_id)
            .map_err(WriterError::from)?;
        let mut changed_paths = Vec::new();
        for anchor in anchors {
            if anchor.text_revision != base_revision {
                return Err(invalid_annotation(
                    "text anchor revision does not match edit base",
                ));
            }
            match crate::annotations::ranges::map_text_anchor_after_edits(
                &anchor,
                &edits,
                text_revision,
                now,
            )
            .map_err(WriterError::from)?
            {
                TextAnchorEdit::Updated(anchor) => changed_paths.extend(
                    store
                        .apply_mutation(AnnotationMutation::UpsertTextAnchor(anchor))
                        .map_err(WriterError::from)?,
                ),
                TextAnchorEdit::Deleted(anchor) => changed_paths.extend(
                    store
                        .apply_mutation(AnnotationMutation::Delete {
                            kind: AnnotationKind::TextAnchor,
                            project_id: project_id.clone(),
                            object_id: anchor.anchor_id,
                            deleted_at_ms: now,
                        })
                        .map_err(WriterError::from)?,
                ),
            }
        }

        let changed = !changed_paths.is_empty();
        if changed {
            if !matches!(
                cause,
                EditorTransactionCauseDto::Undo
                    | EditorTransactionCauseDto::Redo
                    | EditorTransactionCauseDto::Load
            ) {
                let mut histories = self
                    .annotation_edit_history
                    .lock()
                    .map_err(|_| invalid_annotation("annotation edit history lock poisoned"))?;
                let history = histories.entry(key).or_default();
                history.undo.push(before);
                if history.undo.len() > 512 {
                    history.undo.remove(0);
                }
                history.redo.clear();
            }
            self.finish_annotation_write(&project_id, changed_paths, "map_annotation_text_anchors");
        }
        Ok(changed)
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct AnnotationEditHistory {
    undo: Vec<ChapterAnnotationState>,
    redo: Vec<ChapterAnnotationState>,
}

#[derive(Debug, Clone, Default)]
struct ChapterAnnotationState {
    anchors: Vec<TextAnchor>,
    bindings: Vec<TagBinding>,
    links: Vec<ObjectLink>,
}

fn snapshot_chapter_annotations(
    store: &AnnotationStore,
    chapter_id: &str,
) -> ApiResult<ChapterAnnotationState> {
    let anchors = store
        .list_text_anchors_for_chapter(chapter_id)
        .map_err(WriterError::from)?;
    let anchor_ids = anchors
        .iter()
        .map(|anchor| anchor.anchor_id.as_str())
        .collect::<HashSet<_>>();
    let bindings = store
        .list_bindings()
        .map_err(WriterError::from)?
        .into_iter()
        .filter(|binding| match &binding.target {
            ObjectRef::TextAnchor { object_id, .. } => anchor_ids.contains(object_id.as_str()),
            _ => false,
        })
        .collect();
    let links = store
        .list_links()
        .map_err(WriterError::from)?
        .into_iter()
        .filter(|link| match &link.source {
            ObjectRef::TextAnchor { object_id, .. } => anchor_ids.contains(object_id.as_str()),
            _ => false,
        })
        .collect();
    Ok(ChapterAnnotationState {
        anchors,
        bindings,
        links,
    })
}

fn restore_chapter_annotations(
    store: &AnnotationStore,
    project_id: &str,
    chapter_id: &str,
    target: &ChapterAnnotationState,
    text_revision: u64,
    now: i64,
) -> ApiResult<Vec<String>> {
    let current = snapshot_chapter_annotations(store, chapter_id)?;
    let target_anchor_ids = target
        .anchors
        .iter()
        .map(|anchor| anchor.anchor_id.as_str())
        .collect::<HashSet<_>>();
    let mut changed_paths = Vec::new();
    for anchor in &current.anchors {
        if !target_anchor_ids.contains(anchor.anchor_id.as_str()) {
            changed_paths.extend(
                store
                    .apply_mutation(AnnotationMutation::Delete {
                        kind: AnnotationKind::TextAnchor,
                        project_id: project_id.to_string(),
                        object_id: anchor.anchor_id.clone(),
                        deleted_at_ms: now,
                    })
                    .map_err(WriterError::from)?,
            );
        }
    }
    let target_binding_ids = target
        .bindings
        .iter()
        .map(|binding| binding.binding_id.as_str())
        .collect::<HashSet<_>>();
    for binding in &current.bindings {
        if !target_binding_ids.contains(binding.binding_id.as_str()) {
            changed_paths.extend(
                store
                    .apply_mutation(AnnotationMutation::Delete {
                        kind: AnnotationKind::Binding,
                        project_id: project_id.to_string(),
                        object_id: binding.binding_id.clone(),
                        deleted_at_ms: now,
                    })
                    .map_err(WriterError::from)?,
            );
        }
    }
    let target_link_ids = target
        .links
        .iter()
        .map(|link| link.link_id.as_str())
        .collect::<HashSet<_>>();
    for link in &current.links {
        if !target_link_ids.contains(link.link_id.as_str()) {
            changed_paths.extend(
                store
                    .apply_mutation(AnnotationMutation::Delete {
                        kind: AnnotationKind::Link,
                        project_id: project_id.to_string(),
                        object_id: link.link_id.clone(),
                        deleted_at_ms: now,
                    })
                    .map_err(WriterError::from)?,
            );
        }
    }
    for mut anchor in target.anchors.clone() {
        anchor.text_revision = text_revision;
        anchor.updated_at_ms = now;
        anchor.deleted_at_ms = None;
        changed_paths.extend(
            store
                .apply_mutation(AnnotationMutation::UpsertTextAnchor(anchor))
                .map_err(WriterError::from)?,
        );
    }
    for mut binding in target.bindings.clone() {
        binding.updated_at_ms = now;
        binding.deleted_at_ms = None;
        changed_paths.extend(
            store
                .apply_mutation(AnnotationMutation::UpsertBinding(binding))
                .map_err(WriterError::from)?,
        );
    }
    for mut link in target.links.clone() {
        link.updated_at_ms = now;
        link.deleted_at_ms = None;
        changed_paths.extend(
            store
                .apply_mutation(AnnotationMutation::UpsertLink(link))
                .map_err(WriterError::from)?,
        );
    }
    Ok(changed_paths)
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
