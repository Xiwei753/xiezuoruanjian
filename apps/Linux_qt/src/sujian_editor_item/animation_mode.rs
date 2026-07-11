use serde::Serialize;
use writer_core::editor::{
    AnimationMode as CoreAnimationMode, CursorRect, GlyphRect, ReflowGlyphRect,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AnimationMode {
    GlyphAnimation,
    ClusterAnimation,
    RunAnimation,
    LineReflowAnimation,
    SystemSuppressed,
}

impl AnimationMode {
    pub fn from_core(mode: CoreAnimationMode) -> Self {
        match mode {
            CoreAnimationMode::GlyphAnimation => AnimationMode::GlyphAnimation,
            CoreAnimationMode::ClusterAnimation => AnimationMode::ClusterAnimation,
            CoreAnimationMode::RunAnimation => AnimationMode::RunAnimation,
            CoreAnimationMode::LineReflowAnimation => AnimationMode::LineReflowAnimation,
            CoreAnimationMode::SnapshotAnimation | CoreAnimationMode::SystemSuppressed => {
                AnimationMode::SystemSuppressed
            }
        }
    }

    pub fn should_create_transaction(&self) -> bool {
        !matches!(self, AnimationMode::SystemSuppressed)
    }
}
