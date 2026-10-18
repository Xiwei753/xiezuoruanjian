// SPDX-License-Identifier: GPL-3.0-only
//! The file-backed, read-only model mapping.

use std::{fs::File, path::Path};

use memmap2::{Mmap, MmapOptions};

use super::KenlmError;

pub(super) struct MappedModel {
    // Retaining the handle keeps the mapped file object alive for the mapping's
    // entire lifetime. The field is intentionally otherwise unused.
    _file: File,
    bytes: Mmap,
}

impl MappedModel {
    pub(super) fn open(path: impl AsRef<Path>) -> Result<Self, KenlmError> {
        let file = File::open(path)?;
        let metadata = file.metadata()?;
        if metadata.len() == 0 {
            return Err(KenlmError::InvalidFormat(
                "the model file is empty".to_owned(),
            ));
        }

        // SAFETY: this reader exposes only immutable slices and never writes to
        // the mapping. Its documented contract requires the backing model file
        // to remain unchanged while the model is alive; callers must not edit
        // or truncate a mapped model in place.
        let bytes = unsafe { MmapOptions::new().map(&file)? };
        if bytes.len() as u64 != metadata.len() {
            return Err(KenlmError::InvalidFormat(
                "the model file changed while it was being mapped".to_owned(),
            ));
        }

        Ok(Self { _file: file, bytes })
    }

    pub(super) fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}
