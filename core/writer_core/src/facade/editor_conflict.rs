//! 编辑会话冲突材料：只保存双方正文与身份，不发布正文、不参与版本 CAS。
//!
//! 与章节提交共用 `app-meta/chapter-commits/conflicts/<transaction_id>` 目录布局，
//! 但入口独立：空覆盖检查、expected/actual 版本比较和正文投影均不经过这里。

use crate::chapter::commit::{
    write_editor_conflict_material, EditorConflictMaterialReceipt, EditorConflictMaterialRequest,
};

impl super::WriterCore {
    /// 写入编辑会话冲突材料，返回实际 transactionId 与目录。
    pub fn write_editor_conflict_material(
        &self,
        request: &EditorConflictMaterialRequest,
    ) -> crate::error::Result<EditorConflictMaterialReceipt> {
        let project_root = self.project_root(&request.key.project_id);
        write_editor_conflict_material(&project_root, request)
    }
}
