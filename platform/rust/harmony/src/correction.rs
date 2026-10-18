//! 中文纠错 Core C-ABI facade for HarmonyOS.
//!
//! The correction rules and state remain in WriterCore. This module only re-exports
//! its stable C ABI so the Harmony NAPI bridge can link the same operations used by
//! the other platform adapters.

pub use writer_core::ffi::correction_ops::{
    writer_core_add_correction_dictionary_entry, writer_core_apply_correction,
    writer_core_check_correction_chapter, writer_core_check_correction_segment,
    writer_core_get_correction_chapter_snapshot, writer_core_ignore_correction,
    writer_core_set_correction_model_path,
};
