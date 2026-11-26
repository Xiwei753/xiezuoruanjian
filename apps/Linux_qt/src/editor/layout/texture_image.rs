//! 冻结行图的共享所有者与进程级纹理内存分项。
//!
//! canonical 排版、动画 raster、布局快照与纹理缓存都通过同一 `Arc<LineImage>`
//! 引用一张行图；缓存淘汰只有在 `Arc::strong_count == 1` 时才会真正移除条目，
//! 避免"淘汰了缓存条目但底层图像仍被快照/canonical 持有"。

use qmetaobject::QImage;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

pub(crate) struct LineImage {
    texture: QImage,
    bytes: usize,
}

impl LineImage {
    pub(crate) fn shared(texture: QImage) -> Arc<Self> {
        let bytes = estimate_texture_bytes(&texture);
        let image = Arc::new(Self { texture, bytes });
        LIVE_IMAGE_BYTES.fetch_add(bytes, Ordering::Relaxed);
        image
    }

    pub(crate) fn texture(&self) -> &QImage {
        &self.texture
    }

    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }
}

pub(crate) trait IntoLineImage {
    fn into_line_image(self) -> Arc<LineImage>;
}

impl IntoLineImage for QImage {
    fn into_line_image(self) -> Arc<LineImage> {
        LineImage::shared(self)
    }
}

impl IntoLineImage for Arc<LineImage> {
    fn into_line_image(self) -> Arc<LineImage> {
        self
    }
}

impl Drop for LineImage {
    fn drop(&mut self) {
        LIVE_IMAGE_BYTES.fetch_sub(self.bytes, Ordering::Relaxed);
    }
}

fn estimate_texture_bytes(texture: &QImage) -> usize {
    let size = texture.size();
    (size.width as usize)
        .saturating_mul(size.height as usize)
        .saturating_mul(4)
}

static LIVE_IMAGE_BYTES: AtomicUsize = AtomicUsize::new(0);
static CACHE_ONLY_BYTES: AtomicUsize = AtomicUsize::new(0);
static SHARED_BYTES: AtomicUsize = AtomicUsize::new(0);
static CACHE_LINES: AtomicUsize = AtomicUsize::new(0);
static ACTIVE_VISUAL_SNAPSHOTS: AtomicUsize = AtomicUsize::new(0);

/// 进程级行纹理内存分项，供 `process.resource` 诊断读取。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TextureMemoryMetrics {
    /// 当前仅由纹理缓存持有、可安全淘汰的字节数。
    pub cache_only_bytes: usize,
    /// 仍被布局/动画快照或 canonical 共享、当前不可淘汰的缓存条目字节数。
    pub shared_bytes: usize,
    /// 所有 `LineImage` 共享所有者的存活字节数（含快照/canonical 独占图）。
    ///
    /// `QImage` 的隐式共享底层存储只按一个 `LineImage` 所有者计数；不在
    /// `LineImage` 中的 Qt 临时副本不在此数字里。
    pub live_bytes: usize,
    /// 纹理缓存条目数。
    pub lines: usize,
    /// 当前活动视觉快照数量（由编辑管线发布）。
    pub active_visual_snapshots: usize,
}

pub(crate) fn texture_memory_metrics() -> TextureMemoryMetrics {
    TextureMemoryMetrics {
        cache_only_bytes: CACHE_ONLY_BYTES.load(Ordering::Relaxed),
        shared_bytes: SHARED_BYTES.load(Ordering::Relaxed),
        live_bytes: LIVE_IMAGE_BYTES.load(Ordering::Relaxed),
        lines: CACHE_LINES.load(Ordering::Relaxed),
        active_visual_snapshots: ACTIVE_VISUAL_SNAPSHOTS.load(Ordering::Relaxed),
    }
}

pub(crate) fn set_active_visual_snapshots(count: usize) {
    ACTIVE_VISUAL_SNAPSHOTS.store(count, Ordering::Relaxed);
}

pub(crate) fn publish_cache_metrics(cache_only_bytes: usize, shared_bytes: usize, lines: usize) {
    CACHE_ONLY_BYTES.store(cache_only_bytes, Ordering::Relaxed);
    SHARED_BYTES.store(shared_bytes, Ordering::Relaxed);
    CACHE_LINES.store(lines, Ordering::Relaxed);
}
