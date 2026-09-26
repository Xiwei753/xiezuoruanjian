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
        label: Option<&str>,
        target_uri: Option<&str>,
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
        if let Some(l) = label {
            hl.label = Some(l.to_string());
        }
        if let Some(u) = target_uri {
            hl.target_uri = u.to_string();
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
