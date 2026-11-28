use crate::api::{
    ChapterContentDto, ChapterDraftAckDto, ChapterMetaDto, ChapterSaveReceiptDto,
    ChapterVersionDto, EditorConflictMaterialReceiptDto, EditorConflictMaterialRequestDto,
    ProjectWorkspaceSnapshotDto, VolumeDto, WriterError,
};

impl super::WriterAppService {
    pub fn list_volumes(&self, project_id: String) -> Result<Vec<VolumeDto>, WriterError> {
        self.api.list_volumes(&project_id)
    }

    /// 一次返回作品的全部卷 章节 统计快照。
    pub fn get_project_workspace_snapshot(
        &self,
        project_id: String,
    ) -> Result<ProjectWorkspaceSnapshotDto, WriterError> {
        self.api.get_project_workspace_snapshot(&project_id)
    }

    pub fn create_volume(
        &self,
        project_id: String,
        title: String,
    ) -> Result<VolumeDto, WriterError> {
        self.api.create_volume(&project_id, &title)
    }

    /// 恢复/导入卷——使用 manifest 中的稳定 ID。
    pub fn create_volume_with_id(
        &self,
        project_id: String,
        id: String,
        title: String,
        order: i32,
    ) -> Result<VolumeDto, WriterError> {
        self.api
            .create_volume_with_id(&project_id, &id, &title, order)
    }

    pub fn rename_volume(
        &self,
        project_id: String,
        volume_id: String,
        new_title: String,
    ) -> Result<bool, WriterError> {
        self.api.rename_volume(&project_id, &volume_id, &new_title)
    }

    pub fn delete_volume(
        &self,
        project_id: String,
        volume_id: String,
    ) -> Result<bool, WriterError> {
        self.api.delete_volume(&project_id, &volume_id)
    }

    pub fn reorder_volumes(
        &self,
        project_id: String,
        ordered_volume_ids: Vec<String>,
    ) -> Result<bool, WriterError> {
        self.api.reorder_volumes(&project_id, &ordered_volume_ids)
    }

    pub fn list_chapters(
        &self,
        project_id: String,
        volume_id: String,
    ) -> Result<Vec<ChapterMetaDto>, WriterError> {
        self.api.list_chapters(&project_id, &volume_id)
    }

    pub fn create_chapter(
        &self,
        project_id: String,
        volume_id: String,
        title: String,
    ) -> Result<ChapterMetaDto, WriterError> {
        self.api.create_chapter(&project_id, &volume_id, &title)
    }

    /// 恢复/导入章节——使用 manifest 中的稳定 ID。
    pub fn create_chapter_with_id(
        &self,
        project_id: String,
        volume_id: String,
        id: String,
        title: String,
        order: i32,
    ) -> Result<ChapterMetaDto, WriterError> {
        self.api
            .create_chapter_with_id(&project_id, &volume_id, &id, &title, order)
    }

    pub fn create_chapter_in_project(
        &self,
        project_id: String,
        title: String,
    ) -> Result<ChapterMetaDto, WriterError> {
        self.api.create_chapter_in_project(&project_id, &title)
    }

    pub fn rename_chapter(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
        new_title: String,
    ) -> Result<bool, WriterError> {
        self.api
            .rename_chapter(&project_id, &volume_id, &chapter_id, &new_title)
    }

    pub fn delete_chapter(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
    ) -> Result<bool, WriterError> {
        self.api
            .delete_chapter(&project_id, &volume_id, &chapter_id)
    }

    pub fn reorder_chapters(
        &self,
        project_id: String,
        volume_id: String,
        ordered_chapter_ids: Vec<String>,
    ) -> Result<bool, WriterError> {
        self.api
            .reorder_chapters(&project_id, &volume_id, &ordered_chapter_ids)
    }

    /// Issue #1051：跨卷移动章节（目录迁移 + 两卷 order 重排，一次原子 Core 更新）。
    pub fn move_chapter_to_volume(
        &self,
        project_id: String,
        source_volume_id: String,
        target_volume_id: String,
        chapter_id: String,
        target_index: u32,
    ) -> Result<bool, WriterError> {
        self.api
            .move_chapter_to_volume(
                &project_id,
                &source_volume_id,
                &target_volume_id,
                &chapter_id,
                target_index,
            )
            .map(|outcome| outcome.moved)
    }

    pub fn open_chapter(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
    ) -> Result<ChapterContentDto, WriterError> {
        self.api.open_chapter(&project_id, &volume_id, &chapter_id)
    }

    pub fn save_chapter_content(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
        content: String,
    ) -> Result<ChapterSaveReceiptDto, WriterError> {
        self.api
            .save_chapter_content(&project_id, &volume_id, &chapter_id, &content)
    }

    pub fn save_chapter_content_with_options(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
        content: String,
        allow_empty_overwrite: bool,
    ) -> Result<ChapterSaveReceiptDto, WriterError> {
        self.api.save_chapter_content_with_options(
            &project_id,
            &volume_id,
            &chapter_id,
            &content,
            allow_empty_overwrite,
        )
    }

    pub fn save_chapter_content_versioned(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
        content: String,
        expected_version: Option<ChapterVersionDto>,
        base_content: Option<String>,
    ) -> Result<ChapterSaveReceiptDto, WriterError> {
        self.api.save_chapter_content_versioned(
            &project_id,
            &volume_id,
            &chapter_id,
            &content,
            expected_version,
            base_content,
        )
    }

    pub fn save_chapter_content_versioned_allow_empty(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
        content: String,
        expected_version: Option<ChapterVersionDto>,
        base_content: Option<String>,
    ) -> Result<ChapterSaveReceiptDto, WriterError> {
        self.api.save_chapter_content_versioned_allow_empty(
            &project_id,
            &volume_id,
            &chapter_id,
            &content,
            expected_version,
            base_content,
        )
    }

    /// 编辑会话冲突材料：只写材料，不发布正文。
    pub fn write_editor_conflict_material(
        &self,
        request: EditorConflictMaterialRequestDto,
    ) -> Result<EditorConflictMaterialReceiptDto, WriterError> {
        self.api.write_editor_conflict_material(request)
    }

    pub fn save_chapter_content_versioned_with_batch(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
        content: String,
        expected_version: Option<ChapterVersionDto>,
        base_content: Option<String>,
        proof_snapshot_revision: u64,
        proof_event_ids: Vec<String>,
        allow_empty_overwrite: bool,
    ) -> Result<ChapterSaveReceiptDto, WriterError> {
        self.api.save_chapter_content_versioned_with_snapshot(
            &project_id,
            &volume_id,
            &chapter_id,
            &content,
            expected_version,
            base_content,
            proof_snapshot_revision,
            &proof_event_ids,
            allow_empty_overwrite,
        )
    }

    pub fn clear_chapter_content_versioned_with_batch(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
        expected_version: Option<ChapterVersionDto>,
        base_content: Option<String>,
        proof_snapshot_revision: u64,
        proof_event_ids: Vec<String>,
    ) -> Result<ChapterSaveReceiptDto, WriterError> {
        self.api.clear_chapter_content_versioned_with_batch(
            &project_id,
            &volume_id,
            &chapter_id,
            expected_version,
            base_content,
            proof_snapshot_revision,
            &proof_event_ids,
        )
    }

    /// 显式原始证明批次保存（Issue #1062 1.2）。
    ///
    /// `draft_ack_required` 区分平台是否有独立持久化草稿：Qt 传 `true`，
    /// Harmony 无外部草稿传 `false`（Core 完成投影后自行清理 journal）。
    #[allow(clippy::too_many_arguments)]
    pub fn save_chapter_content_versioned_with_proof_batch(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
        content: String,
        expected_version: Option<ChapterVersionDto>,
        base_content: Option<String>,
        proof_snapshot_revision: u64,
        proof_batch_json: String,
        allow_empty_overwrite: bool,
        draft_ack_required: bool,
    ) -> Result<ChapterSaveReceiptDto, WriterError> {
        self.api.save_chapter_content_versioned_with_proof_batch(
            &project_id,
            &volume_id,
            &chapter_id,
            &content,
            expected_version,
            base_content,
            proof_snapshot_revision,
            &proof_batch_json,
            allow_empty_overwrite,
            draft_ack_required,
        )
    }

    /// 清空正文使用与保存相同的显式原始证明批次协议（Issue #1062 1.2）。
    pub fn clear_chapter_content_versioned_with_proof_batch(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
        expected_version: Option<ChapterVersionDto>,
        base_content: Option<String>,
        proof_snapshot_revision: u64,
        proof_batch_json: String,
    ) -> Result<ChapterSaveReceiptDto, WriterError> {
        self.api.clear_chapter_content_versioned_with_proof_batch(
            &project_id,
            &volume_id,
            &chapter_id,
            expected_version,
            base_content,
            proof_snapshot_revision,
            &proof_batch_json,
        )
    }

    /// 列出 Core journal 中仍需平台草稿确认的提交（Issue #1062）。
    pub fn list_pending_chapter_draft_acks(
        &self,
        project_id: String,
    ) -> Result<Vec<ChapterDraftAckDto>, WriterError> {
        self.api.list_pending_chapter_draft_acks(project_id)
    }

    /// 确认平台草稿已完成某笔提交的精确 ack（Issue #1062）。
    pub fn acknowledge_chapter_commit_draft(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
        transaction_id: String,
        event_ids: Vec<String>,
    ) -> Result<u32, WriterError> {
        self.api.acknowledge_chapter_commit_draft(
            project_id,
            volume_id,
            chapter_id,
            transaction_id,
            event_ids,
        )
    }

    pub fn clear_chapter_content(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
    ) -> Result<ChapterSaveReceiptDto, WriterError> {
        self.api
            .clear_chapter_content(&project_id, &volume_id, &chapter_id)
    }

    pub fn clear_chapter_content_versioned(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
        expected_version: Option<ChapterVersionDto>,
        base_content: Option<String>,
    ) -> Result<ChapterSaveReceiptDto, WriterError> {
        self.api.clear_chapter_content_versioned(
            &project_id,
            &volume_id,
            &chapter_id,
            expected_version,
            base_content,
        )
    }

    pub fn update_chapter_note(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
        note: String,
    ) -> Result<bool, WriterError> {
        self.api
            .update_chapter_note(&project_id, &volume_id, &chapter_id, &note)
    }
}
