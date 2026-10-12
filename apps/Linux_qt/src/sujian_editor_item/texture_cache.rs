pub(crate) use super::layout_snapshot::LineSnapshotId;
use super::line_snapshot::LineTextureStore;

pub(crate) struct TextureCache {
    line_store: LineTextureStore,
    /// Issue #901 第十轮问题3：已 pin 的 snapshot id 集合。
    ///
    /// 当一个静态 glyph 即将转为动画 owner 时，调用 `prepare_and_pin_texture`
    /// 基于它的行 `QTextLayout` 正式准备并 pin 图像。成功后该 snapshot_id
    /// 进入此集合，`retain_active_snapshot_ids` 会自动保留 pin 的纹理，
    /// 不会在动画 owner 接管前被 sweep 掉。glyph 完成动画归还静态后，
    /// 调用 `unpin_texture` 移出此集合。
    pinned_ids: std::collections::HashSet<LineSnapshotId>,
}

impl TextureCache {
    pub fn new() -> Self {
        Self {
            line_store: LineTextureStore::new(),
            pinned_ids: std::collections::HashSet::new(),
        }
    }

    pub fn insert_line(&mut self, id: LineSnapshotId, texture: qmetaobject::QImage) {
        self.line_store.insert(id, texture);
    }

    pub fn get_line(&self, id: &LineSnapshotId) -> Option<&qmetaobject::QImage> {
        self.line_store.get(id)
    }

    pub fn contains_line(&self, id: &LineSnapshotId) -> bool {
        self.line_store.contains(id)
    }

    /// Issue #736 评论 5786231506: 按当前所有 active transaction 实际还引用的
    /// LineSnapshotId 来管理纹理生命周期。只保留在 active ids 里的纹理，
    /// 删除不在的。不再让 remove_for_transaction 无条件删共享 snapshot。
    ///
    /// Issue #901 第十轮问题3：pinned ids 永远保留，不被 sweep 掉。
    /// `retain_active_snapshot_ids` 的 active_ids 是当前帧引用的 snapshot，
    /// 但一个静态 glyph 即将转为动画 owner 时，它的纹理可能尚未进入 active_ids
    ///（下一帧才会成为动画 glyph）。pin 集合保证这种"过渡期"纹理不被误删。
    pub fn retain_active_snapshot_ids(&mut self, active_snapshot_ids: &[LineSnapshotId]) {
        let mut keep_ids: Vec<LineSnapshotId> = active_snapshot_ids.to_vec();
        for id in &self.pinned_ids {
            if !keep_ids.contains(id) {
                keep_ids.push(*id);
            }
        }
        self.line_store.retain(&keep_ids);
    }

    /// Issue #901 第十轮问题3：在 glyph 将转为动画 owner 前基于它的行
    /// `QTextLayout` 正式准备并 pin 图像。
    ///
    /// **语义**：将"这个静态 glyph 真的画出来了"和"它是否已持有可用于动画的
    /// texture"分两个状态。静态实际可见片段无论有无纹理都统一减去有效裁剪区；
    /// 但旧静态 glyph 转动画 owner 前，必须先调用此方法确认纹理已就绪并 pin，
    /// 成功后才允许转动画。失败（纹理缺失）时调用方应保持静态 owner，不转动画。
    ///
    /// **参数**：
    /// - `id`：行快照 id。
    /// - `texture`：基于该行 `QTextLayout` 正式准备的 `QImage`。调用方负责
    ///   从 `QTextLayout` 生成图像（与 `insert_line` 的图像来源一致）。
    ///
    /// **返回**：`true` 表示纹理已就绪并已 pin；`false` 表示 `texture` 为空
    ///（调用方传入空图像时，仍尝试用已缓存的纹理，若缓存也无则返回 false）。
    pub fn prepare_and_pin_texture(
        &mut self,
        id: LineSnapshotId,
        texture: Option<qmetaobject::QImage>,
    ) -> bool {
        if let Some(img) = texture {
            self.line_store.insert(id, img);
        }
        let ready = self.line_store.contains(&id);
        if ready {
            self.pinned_ids.insert(id);
        }
        ready
    }

    /// Issue #901 第十轮问题3：取消 pin，让纹理回归正常 retain 生命周期。
    ///
    /// glyph 完成动画归还静态后调用。下一帧 `retain_active_snapshot_ids`
    /// 会按正常 active_ids 决定是否保留此纹理。
    pub fn unpin_texture(&mut self, id: &LineSnapshotId) {
        self.pinned_ids.remove(id);
    }

    /// Issue #901 第十轮问题3：查询某 snapshot_id 是否已被 pin。
    pub fn is_pinned(&self, id: &LineSnapshotId) -> bool {
        self.pinned_ids.contains(id)
    }
}

impl Default for TextureCache {
    fn default() -> Self {
        Self::new()
    }
}
