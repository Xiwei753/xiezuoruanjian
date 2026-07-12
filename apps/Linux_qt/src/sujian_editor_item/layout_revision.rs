use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_REVISION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct LayoutRevision(pub u64);

impl LayoutRevision {
    pub fn next() -> Self {
        LayoutRevision(NEXT_REVISION.fetch_add(1, Ordering::Relaxed))
    }

    pub fn initial() -> Self {
        LayoutRevision(0)
    }
}

impl Default for LayoutRevision {
    fn default() -> Self {
        Self::initial()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CanonicalLayoutRevision {
    pub document_revision: u64,
    pub layout_revision: u64,
    pub viewport_width: f64,
    pub font_fingerprint: String,
    pub font_pixel_size: f64,
    pub font_weight: i32,
    pub line_spacing: f64,
    pub text_indent: f64,
    pub paragraph_spacing: f64,
    pub device_pixel_ratio: f64,
}

impl CanonicalLayoutRevision {
    pub fn new(
        document_revision: u64,
        layout_revision: u64,
        viewport_width: f64,
        font_fingerprint: &str,
        font_pixel_size: f64,
        font_weight: i32,
        line_spacing: f64,
        text_indent: f64,
        paragraph_spacing: f64,
        device_pixel_ratio: f64,
    ) -> Self {
        Self {
            document_revision,
            layout_revision,
            viewport_width,
            font_fingerprint: font_fingerprint.to_string(),
            font_pixel_size,
            font_weight,
            line_spacing,
            text_indent,
            paragraph_spacing,
            device_pixel_ratio,
        }
    }
}
