use writer_core::editor::text_edit_session::{TextEditSessionId, TextEditSessionRegistry};
use std::collections::HashMap;

pub(crate) struct EditableTextTarget {
    pub target_id: String,
    pub is_persistent: bool,
    pub current_text: String,
}

pub(crate) struct LinuxTextEditorCoordinator {
    targets: HashMap<String, EditableTextTarget>,
    registry: TextEditSessionRegistry,
    persistent_session_ids: HashMap<String, u64>,
    active_target_id: Option<String>,
    active_session_id: Option<u64>,
}

impl LinuxTextEditorCoordinator {
    pub fn new() -> Self {
        Self {
            targets: HashMap::new(),
            registry: TextEditSessionRegistry::new(),
            persistent_session_ids: HashMap::new(),
            active_target_id: None,
            active_session_id: None,
        }
    }

    pub fn register_target(&mut self, target: EditableTextTarget) {
        self.targets.insert(target.target_id.clone(), target);
    }

    pub fn unregister_target(&mut self, target_id: &str) {
        if self.active_target_id.as_deref() == Some(target_id) {
            self.cancel_active_edit();
        }
        if let Some(session_id_raw) = self.persistent_session_ids.remove(target_id) {
            self.registry.close_session(TextEditSessionId(session_id_raw));
        }
        self.targets.remove(target_id);
    }

    pub fn begin_edit(&mut self, target_id: &str) -> bool {
        if !self.targets.contains_key(target_id) {
            return false;
        }

        if self.active_target_id.as_deref() == Some(target_id) {
            return true;
        }

        if self.active_target_id.is_some() {
            self.commit_active_edit();
        }

        let text = self.targets.get(target_id).map(|t| t.current_text.clone()).unwrap_or_default();
        let cursor = text.len();
        let is_persistent = self.targets.get(target_id).map(|t| t.is_persistent).unwrap_or(false);

        let session_id_raw = if is_persistent {
            if let Some(&existing_raw) = self.persistent_session_ids.get(target_id) {
                if self.registry.session_exists(TextEditSessionId(existing_raw)) {
                    existing_raw
                } else {
                    self.persistent_session_ids.remove(target_id);
                    self.registry.close_session(TextEditSessionId(existing_raw));
                    match self.registry.create_session(target_id.to_string(), text, cursor, true) {
                        Ok(id) => {
                            let raw = id.as_u64();
                            self.persistent_session_ids.insert(target_id.to_string(), raw);
                            raw
                        }
                        Err(_) => return false,
                    }
                }
            } else {
                match self.registry.create_session(target_id.to_string(), text, cursor, true) {
                    Ok(id) => {
                        let raw = id.as_u64();
                        self.persistent_session_ids.insert(target_id.to_string(), raw);
                        raw
                    }
                    Err(_) => return false,
                }
            }
        } else {
            match self.registry.create_session(target_id.to_string(), text, cursor, false) {
                Ok(id) => id.as_u64(),
                Err(_) => return false,
            }
        };

        self.active_target_id = Some(target_id.to_string());
        self.active_session_id = Some(session_id_raw);
        true
    }

    pub fn commit_active_edit(&mut self) -> bool {
        let target_id = match self.active_target_id.take() {
            Some(id) => id,
            None => return false,
        };
        let session_id_raw = match self.active_session_id.take() {
            Some(id) => id,
            None => return false,
        };

        let is_persistent = self.targets.get(&target_id).map(|t| t.is_persistent).unwrap_or(false);

        if !is_persistent {
            self.persistent_session_ids.remove(&target_id);
            self.registry.close_session(TextEditSessionId(session_id_raw));
        }

        true
    }

    pub fn cancel_active_edit(&mut self) -> bool {
        let target_id = match self.active_target_id.take() {
            Some(id) => id,
            None => return false,
        };
        let session_id_raw = match self.active_session_id.take() {
            Some(id) => id,
            None => return false,
        };

        self.registry.close_session(TextEditSessionId(session_id_raw));
        self.persistent_session_ids.remove(&target_id);
        true
    }

    pub fn get_active_session(&self) -> Option<&writer_core::editor::text_edit_session::TextEditSession> {
        let session_id_raw = self.active_session_id?;
        self.registry.get_session(TextEditSessionId(session_id_raw))
    }

    pub fn get_active_session_mut(&mut self) -> Option<&mut writer_core::editor::text_edit_session::TextEditSession> {
        let session_id_raw = self.active_session_id?;
        self.registry.get_session_mut(TextEditSessionId(session_id_raw))
    }

    pub fn active_target_id(&self) -> Option<&str> {
        self.active_target_id.as_deref()
    }

    pub fn update_target_text(&mut self, target_id: &str, text: String) {
        if let Some(target) = self.targets.get_mut(target_id) {
            target.current_text = text;
        }
    }
}
