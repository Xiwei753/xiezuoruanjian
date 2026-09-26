use crate::error::Result;
use crate::starmap::types::*;

use super::super::StarMapStore;

impl StarMapStore {
    pub fn upsert_hyperlink(&mut self, hl: StarMapHyperlink) {
        let hl_id = hl.hyperlink_id.clone();
        self.hyperlinks.insert(hl_id.clone(), hl);
        self.dirty_hyperlinks.insert(hl_id.clone());
        self.deleted_hyperlink_ids.remove(&hl_id);
        self.dirty_graph_meta = true;
    }

    pub fn remove_hyperlink(&mut self, hyperlink_id: &str) {
        self.hyperlinks.remove(hyperlink_id);
        self.dirty_hyperlinks.remove(hyperlink_id);
        self.deleted_hyperlink_ids.insert(hyperlink_id.to_string());
        self.dirty_graph_meta = true;
    }

    pub fn add_hyperlink(&mut self, hl: StarMapHyperlink) -> Result<StarMapHyperlink> {
        if self.hyperlinks.contains_key(&hl.hyperlink_id) {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Duplicate hyperlink_id",
            )));
        }
        let result = hl.clone();
        self.upsert_hyperlink(hl);
        Ok(result)
    }

    pub fn update_hyperlink(
        &mut self,
        hyperlink_id: &str,
        patch: &crate::starmap::types::StarMapHyperlinkPatch,
    ) -> Result<StarMapHyperlink> {
        if !self.hyperlinks.contains_key(hyperlink_id) {
            self.ensure_hyperlink_loaded(hyperlink_id)?;
        }
        let hl = self.hyperlinks.get_mut(hyperlink_id).ok_or_else(|| {
            crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Hyperlink not found",
            ))
        })?;
        if let Some(ref l) = patch.label {
            hl.label = l.clone();
        }
        if let Some(ref u) = patch.target_uri {
            validate_hyperlink_uri(u)?;
            hl.target_uri = u.clone();
        }
        if let Some(ref s) = patch.source {
            hl.source = s.clone();
        }
        hl.updated_at = crate::starmap::now_epoch();
        let updated = hl.clone();
        self.dirty_hyperlinks.insert(hyperlink_id.to_string());
        self.dirty_graph_meta = true;
        Ok(updated)
    }

    pub fn delete_hyperlink(&mut self, hyperlink_id: &str) -> Result<()> {
        if !self.hyperlinks.contains_key(hyperlink_id) {
            self.ensure_hyperlink_loaded(hyperlink_id)?;
        }
        if !self.hyperlinks.contains_key(hyperlink_id) {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Hyperlink not found",
            )));
        }
        self.remove_hyperlink(hyperlink_id);
        Ok(())
    }
}

/// 校验 hyperlink target_uri 的 scheme。
///
/// URI 必须有合法 scheme：以 `xxx:` 开头（xxx 非空且只含 ASCII 字母/数字/+/-/.）。
/// 不用 `contains("://")` 因为 `mailto:`、`tel:` 等没有 `//`。
fn validate_hyperlink_uri(uri: &str) -> Result<()> {
    let colon = uri.find(':').ok_or_else(|| {
        crate::error::Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "target_uri must have a scheme (e.g. 'https:', 'mailto:')",
        ))
    })?;
    let scheme = &uri[..colon];
    if scheme.is_empty() {
        return Err(crate::error::Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "target_uri scheme must not be empty",
        )));
    }
    for c in scheme.chars() {
        if !c.is_ascii_alphanumeric() && c != '+' && c != '-' && c != '.' {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("target_uri scheme contains invalid character: {c}"),
            )));
        }
    }
    Ok(())
}
