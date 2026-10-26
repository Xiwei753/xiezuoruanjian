use crate::api::{
    AnnotationAutoMatchDto, AnnotationEditDto, AnnotationLinkResolutionDto,
    AnnotationNamedEntryDto, AnnotationObjectLinkDto, AnnotationObjectRefDto,
    AnnotationTagBindingDto, AnnotationTagDto, AnnotationTargetDto, AnnotationTextAnchorDto,
    ChapterAnnotationsDto, WriterError,
};

impl super::WriterAppService {
    pub fn list_tags(&self, project_id: String) -> Result<Vec<AnnotationTagDto>, WriterError> {
        self.api.list_tags(project_id)
    }

    pub fn create_tag(
        &self,
        project_id: String,
        name: String,
        color: Option<String>,
    ) -> Result<AnnotationTagDto, WriterError> {
        self.api.create_tag(project_id, name, color)
    }

    pub fn rename_tag(
        &self,
        project_id: String,
        tag_id: String,
        name: String,
    ) -> Result<AnnotationTagDto, WriterError> {
        self.api.rename_tag(project_id, tag_id, name)
    }

    pub fn delete_tag(&self, project_id: String, tag_id: String) -> Result<bool, WriterError> {
        self.api.delete_tag(project_id, tag_id)
    }

    pub fn merge_tags(
        &self,
        project_id: String,
        source_tag_id: String,
        target_tag_id: String,
    ) -> Result<bool, WriterError> {
        self.api
            .merge_tags(project_id, source_tag_id, target_tag_id)
    }

    pub fn set_tag_bindings(
        &self,
        project_id: String,
        target: AnnotationObjectRefDto,
        tag_ids: Vec<String>,
    ) -> Result<Vec<AnnotationTagBindingDto>, WriterError> {
        self.api.set_tag_bindings(project_id, target, tag_ids)
    }

    pub fn list_bindings_for_target(
        &self,
        project_id: String,
        target: AnnotationObjectRefDto,
    ) -> Result<Vec<AnnotationTagBindingDto>, WriterError> {
        self.api.list_bindings_for_target(project_id, target)
    }

    pub fn create_text_anchor(
        &self,
        project_id: String,
        chapter_id: String,
        start_byte: u32,
        end_byte: u32,
        text_revision: u64,
        text: String,
    ) -> Result<AnnotationTextAnchorDto, WriterError> {
        self.api.create_text_anchor(
            project_id,
            chapter_id,
            start_byte,
            end_byte,
            text_revision,
            text,
        )
    }

    pub fn create_named_entry(
        &self,
        project_id: String,
        canonical_name: String,
        aliases: Vec<String>,
        confirmed_misspellings: Vec<String>,
        auto_link_enabled: bool,
    ) -> Result<AnnotationNamedEntryDto, WriterError> {
        self.api.create_named_entry(
            project_id,
            canonical_name,
            aliases,
            confirmed_misspellings,
            auto_link_enabled,
        )
    }

    pub fn list_named_entries(
        &self,
        project_id: String,
    ) -> Result<Vec<AnnotationNamedEntryDto>, WriterError> {
        self.api.list_named_entries(project_id)
    }

    pub fn update_named_entry(
        &self,
        project_id: String,
        entry_id: String,
        canonical_name: String,
        aliases: Vec<String>,
        confirmed_misspellings: Vec<String>,
        auto_link_enabled: bool,
    ) -> Result<AnnotationNamedEntryDto, WriterError> {
        self.api.update_named_entry(
            project_id,
            entry_id,
            canonical_name,
            aliases,
            confirmed_misspellings,
            auto_link_enabled,
        )
    }

    pub fn delete_named_entry(
        &self,
        project_id: String,
        entry_id: String,
    ) -> Result<bool, WriterError> {
        self.api.delete_named_entry(project_id, entry_id)
    }

    pub fn create_link(
        &self,
        source: AnnotationObjectRefDto,
        target: AnnotationTargetDto,
    ) -> Result<AnnotationObjectLinkDto, WriterError> {
        self.api.create_link(source, target)
    }

    pub fn update_link(
        &self,
        project_id: String,
        link_id: String,
        source: AnnotationObjectRefDto,
        target: AnnotationTargetDto,
    ) -> Result<AnnotationObjectLinkDto, WriterError> {
        self.api.update_link(project_id, link_id, source, target)
    }

    pub fn delete_link(&self, project_id: String, link_id: String) -> Result<bool, WriterError> {
        self.api.delete_link(project_id, link_id)
    }

    pub fn resolve_link(
        &self,
        target: AnnotationTargetDto,
    ) -> Result<AnnotationLinkResolutionDto, WriterError> {
        self.api.resolve_link(target)
    }

    pub fn find_backlinks(
        &self,
        target: AnnotationTargetDto,
    ) -> Result<Vec<AnnotationObjectLinkDto>, WriterError> {
        self.api.find_backlinks(target)
    }

    pub fn list_links_for_source(
        &self,
        source: AnnotationObjectRefDto,
    ) -> Result<Vec<AnnotationObjectLinkDto>, WriterError> {
        self.api.list_links_for_source(source)
    }

    pub fn get_text_anchor(
        &self,
        project_id: String,
        anchor_id: String,
    ) -> Result<Option<AnnotationTextAnchorDto>, WriterError> {
        self.api.get_text_anchor(project_id, anchor_id)
    }

    pub fn get_chapter_annotations(
        &self,
        project_id: String,
        chapter_id: String,
        text_revision: u64,
        text: String,
    ) -> Result<ChapterAnnotationsDto, WriterError> {
        self.api
            .get_chapter_annotations(project_id, chapter_id, text_revision, text)
    }

    pub fn apply_annotation_edits(
        &self,
        project_id: String,
        chapter_id: String,
        baseline_id: String,
        base_revision: u64,
        text_revision: u64,
        cause: crate::api::EditorTransactionCauseDto,
        edits: Vec<AnnotationEditDto>,
        text: String,
    ) -> Result<bool, WriterError> {
        self.api.apply_annotation_edits(
            project_id,
            chapter_id,
            baseline_id,
            base_revision,
            text_revision,
            cause,
            edits,
            text,
        )
    }

    pub fn find_auto_name_links(
        &self,
        project_id: String,
        chapter_id: String,
        text_revision: u64,
        text: String,
    ) -> Result<Vec<AnnotationAutoMatchDto>, WriterError> {
        self.api
            .find_auto_name_links(project_id, chapter_id, text_revision, text)
    }
}
