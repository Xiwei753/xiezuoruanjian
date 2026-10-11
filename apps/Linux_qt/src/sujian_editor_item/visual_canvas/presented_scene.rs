//! Issue #901：已提交画面的视觉真相。
//!
//! `PresentedScene` 记录 Qt `afterFrameEnd` 确认的最近一帧实际视觉图形。
//! 它保存的是**像素贡献及裁剪面积**，不是旧操作类型、旧动画时钟、旧组装队列。
//!
//! ## 核心语义
//!
//! - 输入后又删除，如果字符 A 从未呈现，则 scene 里没有 A 的任何像素贡献。
//! - 呈现 40% 的字符，scene 只有 40% 的图形贡献（`visible_clip` 裁剪后）。
//! - `source_revision` 是这份画面对应的 canonical revision；新编辑的 source
//!   永远来自最新 `PresentedScene`，用 `RevisionChain` 精确映射到最新 revision。
//!
//! ## 纯 Rust
//!
//! 本模块不直接依赖 Qt C++ 类型。所有几何类型都是纯 Rust f64，由调用方从
//! Qt 排版结果转换而来。

use writer_core::editor::EditorRevision;

use crate::sujian_editor_item::snapshot_id::LineSnapshotId;

// ── 纯 Rust 几何类型 ──

/// 画布二维向量 / 点（纯 Rust，文档坐标或视口坐标由调用方契约决定）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CanvasVec2 {
    pub x: f64,
    pub y: f64,
}

impl CanvasVec2 {
    pub(crate) const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// 线性插值：`self * (1 - t) + other * t`。
    #[inline]
    pub(crate) fn lerp(self, other: Self, t: f64) -> Self {
        Self {
            x: self.x + (other.x - self.x) * t,
            y: self.y + (other.y - self.y) * t,
        }
    }
}

/// 画布矩形（左上角 + 宽高，纯 Rust）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CanvasRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl CanvasRect {
    pub(crate) const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    /// 零面积矩形，用于空裁剪。
    pub(crate) const fn zero() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        }
    }
}

/// 2×3 仿射变换矩阵。
///
/// 表示 `x' = a·x + b·y + tx`，`y' = c·x + d·y + ty`。
/// 恒等变换为 `a = d = 1`，其余为 0。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CanvasAffine2 {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub tx: f64,
    pub ty: f64,
}

impl CanvasAffine2 {
    /// 恒等变换。
    pub(crate) const fn identity() -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            tx: 0.0,
            ty: 0.0,
        }
    }

    /// 将一个点变换到新坐标系。
    #[inline]
    pub(crate) fn transform_point(self, p: CanvasVec2) -> CanvasVec2 {
        CanvasVec2 {
            x: self.a * p.x + self.b * p.y + self.tx,
            y: self.c * p.x + self.d * p.y + self.ty,
        }
    }
}

impl Default for CanvasAffine2 {
    fn default() -> Self {
        Self::identity()
    }
}

// ── 像素贡献 ──

/// 单个绘制对象在已提交帧中的像素贡献。
///
/// 记录的是**纹理事实和裁剪事实**，不是操作类型或动画进度。
/// 一个字符呈现 40% 时，`visible_clip` 只覆盖源纹理 40% 的区域；
/// 从未呈现的字符不产生任何 `PixelContribution`。
///
/// `byte_range` 记录该贡献对应的 Core 逻辑 UTF-8 byte range，
/// `SceneTransition` 据此通过 `RevisionChain` 判断每个贡献是存活、删除
/// 还是新插入，从而实现吞字/吐字/存活字移动而非全文淡入淡出。
///
/// Issue #901 问题1：`source_revision` 携带该贡献自己的来源 Core revision。
/// 旧贡献（存活字/删除字）标 `presented.source_revision`，新贡献（新增字）
/// 标 target revision。`controller.rs` 据此判断每条贡献的真实来源，
/// 混合 old/new 的半成品帧不能简单用单一 `scene.source_revision` 表示
/// 整张画面的字节身份。
///
/// Issue #901 问题2：`snapshot_id` 携带完整 `LineSnapshotId`，而非 u64 hash。
/// 动画层通过完整 `LineSnapshotId` 查询 `TextureCache`，不再从 u64 反解。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PixelContribution {
    /// 源纹理的完整行快照 id。
    pub snapshot_id: LineSnapshotId,
    /// 源纹理中的 UV 区域（纹理局部坐标，已乘 DPR）。
    pub source_uv: CanvasRect,
    /// 文档坐标系中的目标矩形（不含滚动偏移）。
    pub document_rect: CanvasRect,
    /// 实际可见的裁剪区域（文档坐标，已与 viewport / clip 求交）。
    pub visible_clip: CanvasRect,
    /// 该绘制对象的透明度（0.0 完全透明 – 1.0 完全不透明）。
    pub opacity: f64,
    /// 该贡献对应的 Core 逻辑 UTF-8 byte range `(start, end)`（半开区间）。
    ///
    /// Issue #901 阻塞 2：`SceneTransition` 用此字段通过 `RevisionChain`
    /// 判断该贡献在最新 revision 中是存活、删除还是新插入。
    pub byte_range: (usize, usize),
    /// Issue #901 问题1：该贡献对应的 Core source revision。
    ///
    /// 旧贡献（存活字/删除字）标 `presented.source_revision`，
    /// 新贡献（新增字）标 target revision。
    /// `controller.rs` 据此判断每条贡献的真实来源，混合 old/new 的半成品帧
    /// 不能简单用单一 `scene.source_revision` 表示整张画面的字节身份。
    pub source_revision: EditorRevision,
}

impl PixelContribution {
    /// 该贡献是否有非零可见面积。
    pub(crate) fn has_visible_area(&self) -> bool {
        self.visible_clip.w > 0.0 && self.visible_clip.h > 0.0 && self.opacity > 0.0
    }
}

// ── 可见光标 ──

/// 已提交帧中实际绘制的光标位置和状态。
///
/// 这是**视觉事实**：光标在这一帧的像素位置、是否可见、尺寸。
/// 不携带动画时钟或目标位置——那些由 `FrameClock` 统一管理。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PresentedCaret {
    /// 光标左上角位置（视口坐标）。
    pub position: CanvasVec2,
    /// 光标是否实际绘制（闪烁 off 阶段为 false）。
    pub visible: bool,
    /// 光标宽度（像素）。
    pub width: f64,
    /// 光标高度（像素）。
    pub height: f64,
}

// ── 可见滚动 ──

/// 已提交帧中实际的滚动位置。
///
/// 记录的是**视口事实**：这一帧文档相对于视口左上角的偏移量。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PresentedScroll {
    /// 水平滚动偏移（文档坐标）。
    pub x: f64,
    /// 垂直滚动偏移（文档坐标）。
    pub y: f64,
}

impl Default for PresentedScroll {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
}

// ── PresentedScene ──

/// Qt `afterFrameEnd` 确认的最近一帧视觉图形。
///
/// 这是三份真相之一（`CoreDocument` / `TargetLayout` / `PresentedScene`），
/// 保存**已提交画面的像素贡献及裁剪面积**。新编辑的 source 永远来自最新
/// `PresentedScene`；尚没有新帧回执时，维持同一份 presented source，用
/// `RevisionChain` 将 presented revision 持续映射到最新 revision。
///
/// **不可变**：一旦创建不再修改。新帧到来时替换整个 `PresentedScene`。
#[derive(Clone, Debug)]
pub(crate) struct PresentedScene {
    /// 这份画面对应的 canonical revision。
    pub source_revision: EditorRevision,
    /// 这份画面对应的 document session generation。
    pub document_session: u64,
    /// Qt `afterFrameEnd` 确认的 render frame id。
    pub frame_id: u64,
    /// 该画面对应的 geometry id（排版几何版本）。
    pub geometry_id: u64,

    /// Issue #901 问题3：该帧提交时的真实过渡进度（0.0 – 1.0）。
    ///
    /// 由渲染层在 `afterFrameEnd` 时填入该帧实际对应的 `VisualPose.progress`。
    /// `on_frame_committed` 用此值判断画面是否真正到达终点（而非从
    /// `frame_clock.frame_now` 估算理论进度）。`1.0` 表示画面已完全到达目标。
    pub pose_progress: f64,

    /// 整体裁剪区域（视口或 clip node 的可见矩形）。
    pub clip: CanvasRect,
    /// 整体位置偏移（画布坐标系）。
    pub position: CanvasVec2,
    /// 整体透明度（0.0 – 1.0）。
    pub opacity: f64,
    /// 整体仿射变换。
    pub transform: CanvasAffine2,

    /// 所有绘制对象的像素贡献列表。
    ///
    /// 从未呈现的字符不在此列表中；呈现 40% 的字符只有 40% 的
    /// `visible_clip` 贡献。
    pub pixel_contributions: Vec<PixelContribution>,

    /// 可见光标（光标不可见时为 `None`）。
    pub caret: Option<PresentedCaret>,
    /// 可见滚动位置。
    pub scroll: PresentedScroll,
}

impl PresentedScene {
    /// 该帧是否有任何可见像素贡献。
    pub(crate) fn has_visible_content(&self) -> bool {
        self.opacity > 0.0
            && self
                .pixel_contributions
                .iter()
                .any(PixelContribution::has_visible_area)
    }

    /// 该帧中可见像素贡献的数量。
    pub(crate) fn visible_contribution_count(&self) -> usize {
        self.pixel_contributions
            .iter()
            .filter(|c| c.has_visible_area())
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_contribution(clip_w: f64, clip_h: f64, opacity: f64) -> PixelContribution {
        PixelContribution {
            snapshot_id: LineSnapshotId::new(1, 0, 0),
            source_uv: CanvasRect::new(0.0, 0.0, 100.0, 100.0),
            document_rect: CanvasRect::new(10.0, 10.0, 100.0, 100.0),
            visible_clip: CanvasRect::new(10.0, 10.0, clip_w, clip_h),
            opacity,
            byte_range: (0, 100),
            source_revision: EditorRevision::initial(),
        }
    }

    #[test]
    fn pixel_contribution_visible_area() {
        let full = sample_contribution(100.0, 100.0, 1.0);
        assert!(full.has_visible_area());

        let zero_w = sample_contribution(0.0, 100.0, 1.0);
        assert!(!zero_w.has_visible_area());

        let zero_opacity = sample_contribution(100.0, 100.0, 0.0);
        assert!(!zero_opacity.has_visible_area());
    }

    #[test]
    fn presented_scene_visible_content() {
        let scene = PresentedScene {
            source_revision: EditorRevision::initial(),
            document_session: 1,
            frame_id: 1,
            geometry_id: 1,
            pose_progress: 1.0,
            clip: CanvasRect::new(0.0, 0.0, 800.0, 600.0),
            position: CanvasVec2::new(0.0, 0.0),
            opacity: 1.0,
            transform: CanvasAffine2::identity(),
            pixel_contributions: vec![sample_contribution(100.0, 100.0, 1.0)],
            caret: None,
            scroll: PresentedScroll::default(),
        };
        assert!(scene.has_visible_content());
        assert_eq!(scene.visible_contribution_count(), 1);
    }

    #[test]
    fn presented_scene_no_content_when_never_presented() {
        // 输入后又删除，字符从未呈现 → pixel_contributions 为空
        let scene = PresentedScene {
            source_revision: EditorRevision::initial(),
            document_session: 1,
            frame_id: 1,
            geometry_id: 1,
            pose_progress: 1.0,
            clip: CanvasRect::new(0.0, 0.0, 800.0, 600.0),
            position: CanvasVec2::new(0.0, 0.0),
            opacity: 1.0,
            transform: CanvasAffine2::identity(),
            pixel_contributions: vec![],
            caret: None,
            scroll: PresentedScroll::default(),
        };
        assert!(!scene.has_visible_content());
        assert_eq!(scene.visible_contribution_count(), 0);
    }

    #[test]
    fn affine2_identity_transform() {
        let id = CanvasAffine2::identity();
        let p = CanvasVec2::new(3.0, 5.0);
        assert_eq!(id.transform_point(p), p);
    }

    #[test]
    fn vec2_lerp_endpoints() {
        let a = CanvasVec2::new(0.0, 0.0);
        let b = CanvasVec2::new(10.0, 20.0);
        assert_eq!(CanvasVec2::lerp(a, b, 0.0), a);
        assert_eq!(CanvasVec2::lerp(a, b, 1.0), b);
        assert_eq!(CanvasVec2::lerp(a, b, 0.5), CanvasVec2::new(5.0, 10.0));
    }
}
