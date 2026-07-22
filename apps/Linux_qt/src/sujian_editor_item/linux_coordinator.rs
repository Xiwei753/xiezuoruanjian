use writer_core::editor::text_edit_session::{TextEditSessionId, TextEditSessionRegistry};
use std::collections::HashMap;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum TextInputType {
    #[default]
    Text,
    MultiLine,
    Number,
    Email,
    Password,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum SecretPolicy {
    #[default]
    None,
    MaskAndClearOnCommit,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum AutocorrectPolicy {
    #[default]
    Default,
    Disabled,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum CapitalizationPolicy {
    #[default]
    None,
    Characters,
    Words,
    Sentences,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum CopyPolicy {
    #[default]
    Allow,
    Block,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum PastePolicy {
    #[default]
    Allow,
    Block,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum SelectionPolicy {
    #[default]
    Allow,
    CursorOnly,
}

#[derive(Clone, Debug)]
pub(crate) struct TextEditorProfile {
    pub single_line: bool,
    pub input_type: TextInputType,
    pub autocorrect_policy: AutocorrectPolicy,
    pub capitalization_policy: CapitalizationPolicy,
    pub selection_policy: SelectionPolicy,
    pub copy_policy: CopyPolicy,
    pub paste_policy: PastePolicy,
    pub secret_policy: SecretPolicy,
    pub commit_on_focus_loss: bool,
}

impl Default for TextEditorProfile {
    fn default() -> Self {
        Self {
            single_line: false,
            input_type: TextInputType::MultiLine,
            autocorrect_policy: AutocorrectPolicy::Default,
            capitalization_policy: CapitalizationPolicy::None,
            selection_policy: SelectionPolicy::Allow,
            copy_policy: CopyPolicy::Allow,
            paste_policy: PastePolicy::Allow,
            secret_policy: SecretPolicy::None,
            commit_on_focus_loss: true,
        }
    }
}

impl TextEditorProfile {
    pub fn document_body() -> Self {
        Self {
            single_line: false,
            input_type: TextInputType::MultiLine,
            commit_on_focus_loss: false,
            ..Self::default()
        }
    }

    pub fn short_title() -> Self {
        Self {
            single_line: true,
            input_type: TextInputType::Text,
            ..Self::default()
        }
    }

    pub fn search_query() -> Self {
        Self {
            single_line: true,
            input_type: TextInputType::Text,
            autocorrect_policy: AutocorrectPolicy::Disabled,
            ..Self::default()
        }
    }

    pub fn replace_query() -> Self {
        Self {
            single_line: true,
            input_type: TextInputType::Text,
            autocorrect_policy: AutocorrectPolicy::Disabled,
            ..Self::default()
        }
    }

    pub fn canvas_label() -> Self {
        Self {
            single_line: true,
            input_type: TextInputType::Text,
            ..Self::default()
        }
    }

    pub fn repository_url() -> Self {
        Self {
            single_line: true,
            input_type: TextInputType::Text,
            autocorrect_policy: AutocorrectPolicy::Disabled,
            ..Self::default()
        }
    }

    pub fn branch_name() -> Self {
        Self {
            single_line: true,
            input_type: TextInputType::Text,
            autocorrect_policy: AutocorrectPolicy::Disabled,
            ..Self::default()
        }
    }

    pub fn secret_token() -> Self {
        Self {
            single_line: true,
            input_type: TextInputType::Password,
            selection_policy: SelectionPolicy::CursorOnly,
            copy_policy: CopyPolicy::Block,
            paste_policy: PastePolicy::Allow,
            secret_policy: SecretPolicy::MaskAndClearOnCommit,
            ..Self::default()
        }
    }

    pub fn is_secret(&self) -> bool {
        self.secret_policy == SecretPolicy::MaskAndClearOnCommit
    }
}

pub(crate) struct EditableTextTarget {
    pub target_id: String,
    pub is_persistent: bool,
    pub current_text: String,
    pub profile: TextEditorProfile,
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

    pub fn take_active_session_kernel(&mut self) -> Option<writer_core::editor::EditorKernel> {
        let session_id_raw = self.active_session_id?;
        let session = self.registry.get_session_mut(TextEditSessionId(session_id_raw))?;
        Some(std::mem::replace(&mut session.kernel, writer_core::editor::EditorKernel::new()))
    }

    pub fn return_kernel_to_active_session(&mut self, kernel: writer_core::editor::EditorKernel) -> bool {
        let session_id_raw = match self.active_session_id {
            Some(id) => id,
            None => return false,
        };
        let session = match self.registry.get_session_mut(TextEditSessionId(session_id_raw)) {
            Some(s) => s,
            None => return false,
        };
        session.kernel = kernel;
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
            if let Some(target) = self.targets.get_mut(&target_id) {
                if target.profile.is_secret() {
                    target.current_text.clear();
                }
            }
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

    pub fn active_session_id(&self) -> Option<u64> {
        self.active_session_id
    }

    pub fn active_profile(&self) -> Option<&TextEditorProfile> {
        self.active_target_id.as_ref().and_then(|id| self.targets.get(id).map(|t| &t.profile))
    }

    pub fn is_active_single_line(&self) -> bool {
        self.active_target_id.as_ref().and_then(|id| self.targets.get(id).map(|t| t.profile.single_line)).unwrap_or(false)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn register_and_begin_edit() {
        let mut coord = LinuxTextEditorCoordinator::new();
        coord.register_target(EditableTextTarget {
            target_id: "test-target".to_string(),
            is_persistent: false,
            current_text: "hello".to_string(),
            profile: TextEditorProfile::default(),
        });
        assert!(coord.begin_edit("test-target"));
        assert_eq!(coord.active_target_id(), Some("test-target"));
    }

    #[test]
    fn begin_edit_unknown_target_fails() {
        let mut coord = LinuxTextEditorCoordinator::new();
        assert!(!coord.begin_edit("nonexistent"));
    }

    #[test]
    fn begin_edit_same_target_is_idempotent() {
        let mut coord = LinuxTextEditorCoordinator::new();
        coord.register_target(EditableTextTarget {
            target_id: "t1".to_string(),
            is_persistent: false,
            current_text: "abc".to_string(),
            profile: TextEditorProfile::default(),
        });
        assert!(coord.begin_edit("t1"));
        assert!(coord.begin_edit("t1"));
    }

    #[test]
    fn commit_closes_non_persistent_session() {
        let mut coord = LinuxTextEditorCoordinator::new();
        coord.register_target(EditableTextTarget {
            target_id: "t1".to_string(),
            is_persistent: false,
            current_text: "abc".to_string(),
            profile: TextEditorProfile::default(),
        });
        assert!(coord.begin_edit("t1"));
        assert!(coord.commit_active_edit());
        assert!(coord.active_target_id().is_none());
    }

    #[test]
    fn commit_keeps_persistent_session() {
        let mut coord = LinuxTextEditorCoordinator::new();
        coord.register_target(EditableTextTarget {
            target_id: "t1".to_string(),
            is_persistent: true,
            current_text: "abc".to_string(),
            profile: TextEditorProfile::default(),
        });
        assert!(coord.begin_edit("t1"));
        assert!(coord.commit_active_edit());
        assert!(coord.active_target_id().is_none());
        assert!(coord.persistent_session_ids.contains_key("t1"));
    }

    #[test]
    fn cancel_closes_session() {
        let mut coord = LinuxTextEditorCoordinator::new();
        coord.register_target(EditableTextTarget {
            target_id: "t1".to_string(),
            is_persistent: true,
            current_text: "abc".to_string(),
            profile: TextEditorProfile::default(),
        });
        assert!(coord.begin_edit("t1"));
        assert!(coord.cancel_active_edit());
        assert!(coord.active_target_id().is_none());
        assert!(!coord.persistent_session_ids.contains_key("t1"));
    }

    #[test]
    fn commit_without_active_returns_false() {
        let mut coord = LinuxTextEditorCoordinator::new();
        assert!(!coord.commit_active_edit());
    }

    #[test]
    fn cancel_without_active_returns_false() {
        let mut coord = LinuxTextEditorCoordinator::new();
        assert!(!coord.cancel_active_edit());
    }

    #[test]
    fn unregister_cancels_active_edit() {
        let mut coord = LinuxTextEditorCoordinator::new();
        coord.register_target(EditableTextTarget {
            target_id: "t1".to_string(),
            is_persistent: false,
            current_text: "abc".to_string(),
            profile: TextEditorProfile::default(),
        });
        assert!(coord.begin_edit("t1"));
        coord.unregister_target("t1");
        assert!(coord.active_target_id().is_none());
    }

    #[test]
    fn switching_target_commits_previous() {
        let mut coord = LinuxTextEditorCoordinator::new();
        coord.register_target(EditableTextTarget {
            target_id: "t1".to_string(),
            is_persistent: false,
            current_text: "abc".to_string(),
            profile: TextEditorProfile::default(),
        });
        coord.register_target(EditableTextTarget {
            target_id: "t2".to_string(),
            is_persistent: false,
            current_text: "def".to_string(),
            profile: TextEditorProfile::default(),
        });
        assert!(coord.begin_edit("t1"));
        assert!(coord.begin_edit("t2"));
        assert_eq!(coord.active_target_id(), Some("t2"));
    }

    #[test]
    fn update_target_text() {
        let mut coord = LinuxTextEditorCoordinator::new();
        coord.register_target(EditableTextTarget {
            target_id: "t1".to_string(),
            is_persistent: false,
            current_text: "abc".to_string(),
            profile: TextEditorProfile::default(),
        });
        coord.update_target_text("t1", "xyz".to_string());
        assert_eq!(coord.targets.get("t1").unwrap().current_text, "xyz");
    }

    #[test]
    fn persistent_session_reuse() {
        let mut coord = LinuxTextEditorCoordinator::new();
        coord.register_target(EditableTextTarget {
            target_id: "t1".to_string(),
            is_persistent: true,
            current_text: "abc".to_string(),
            profile: TextEditorProfile::default(),
        });
        assert!(coord.begin_edit("t1"));
        let first_session = coord.active_session_id.unwrap();
        assert!(coord.commit_active_edit());
        assert!(coord.begin_edit("t1"));
        let second_session = coord.active_session_id.unwrap();
        assert_eq!(first_session, second_session);
    }

    #[test]
    fn get_active_session_returns_session() {
        let mut coord = LinuxTextEditorCoordinator::new();
        coord.register_target(EditableTextTarget {
            target_id: "t1".to_string(),
            is_persistent: false,
            current_text: "abc".to_string(),
            profile: TextEditorProfile::default(),
        });
        coord.begin_edit("t1");
        let session = coord.get_active_session();
        assert!(session.is_some());
    }

    #[test]
    fn secret_text_cleared_on_commit() {
        let mut coord = LinuxTextEditorCoordinator::new();
        coord.register_target(EditableTextTarget {
            target_id: "secret-1".to_string(),
            is_persistent: false,
            current_text: "my-secret-token".to_string(),
            profile: TextEditorProfile::secret_token(),
        });
        assert!(coord.begin_edit("secret-1"));
        assert!(coord.commit_active_edit());
        assert_eq!(coord.targets.get("secret-1").unwrap().current_text, "");
    }

    #[test]
    fn secret_text_not_cleared_on_persistent_commit() {
        let mut coord = LinuxTextEditorCoordinator::new();
        coord.register_target(EditableTextTarget {
            target_id: "persistent-secret".to_string(),
            is_persistent: true,
            current_text: "my-secret-token".to_string(),
            profile: TextEditorProfile::secret_token(),
        });
        assert!(coord.begin_edit("persistent-secret"));
        assert!(coord.commit_active_edit());
        assert_eq!(coord.targets.get("persistent-secret").unwrap().current_text, "my-secret-token");
    }

    #[test]
    fn non_secret_text_not_cleared_on_commit() {
        let mut coord = LinuxTextEditorCoordinator::new();
        coord.register_target(EditableTextTarget {
            target_id: "normal-1".to_string(),
            is_persistent: false,
            current_text: "hello".to_string(),
            profile: TextEditorProfile::search_query(),
        });
        assert!(coord.begin_edit("normal-1"));
        assert!(coord.commit_active_edit());
        assert_eq!(coord.targets.get("normal-1").unwrap().current_text, "hello");
    }

    #[test]
    fn profile_presets_correct() {
        let secret = TextEditorProfile::secret_token();
        assert!(secret.is_secret());
        assert_eq!(secret.input_type, TextInputType::Password);
        assert_eq!(secret.copy_policy, CopyPolicy::Block);
        assert_eq!(secret.selection_policy, SelectionPolicy::CursorOnly);

        let search = TextEditorProfile::search_query();
        assert!(!search.is_secret());
        assert_eq!(search.autocorrect_policy, AutocorrectPolicy::Disabled);
        assert!(search.single_line);
    }

    #[test]
    fn take_and_return_session_kernel() {
        let mut coord = LinuxTextEditorCoordinator::new();
        coord.register_target(EditableTextTarget {
            target_id: "t1".to_string(),
            is_persistent: false,
            current_text: "hello world".to_string(),
            profile: TextEditorProfile::short_title(),
        });
        assert!(coord.begin_edit("t1"));
        let kernel = coord.take_active_session_kernel();
        assert!(kernel.is_some());
        let k = kernel.unwrap();
        assert_eq!(k.text(), "hello world");
        assert!(coord.return_kernel_to_active_session(k));
    }

    #[test]
    fn take_session_kernel_without_active_returns_none() {
        let mut coord = LinuxTextEditorCoordinator::new();
        assert!(coord.take_active_session_kernel().is_none());
    }

    #[test]
    fn active_session_id_returns_value() {
        let mut coord = LinuxTextEditorCoordinator::new();
        coord.register_target(EditableTextTarget {
            target_id: "t1".to_string(),
            is_persistent: false,
            current_text: "abc".to_string(),
            profile: TextEditorProfile::default(),
        });
        assert!(coord.active_session_id().is_none());
        assert!(coord.begin_edit("t1"));
        assert!(coord.active_session_id().is_some());
    }

    #[test]
    fn active_profile_returns_correct_profile() {
        let mut coord = LinuxTextEditorCoordinator::new();
        coord.register_target(EditableTextTarget {
            target_id: "secret-1".to_string(),
            is_persistent: false,
            current_text: "token".to_string(),
            profile: TextEditorProfile::secret_token(),
        });
        assert!(coord.active_profile().is_none());
        assert!(coord.begin_edit("secret-1"));
        let profile = coord.active_profile().unwrap();
        assert!(profile.is_secret());
        assert_eq!(profile.input_type, TextInputType::Password);
    }

    #[test]
    fn is_active_single_line_returns_correctly() {
        let mut coord = LinuxTextEditorCoordinator::new();
        coord.register_target(EditableTextTarget {
            target_id: "title-1".to_string(),
            is_persistent: false,
            current_text: "title".to_string(),
            profile: TextEditorProfile::short_title(),
        });
        assert!(!coord.is_active_single_line());
        assert!(coord.begin_edit("title-1"));
        assert!(coord.is_active_single_line());
    }

    #[test]
    fn return_kernel_to_nonexistent_session_fails() {
        let mut coord = LinuxTextEditorCoordinator::new();
        let kernel = writer_core::editor::EditorKernel::new();
        assert!(!coord.return_kernel_to_active_session(kernel));
    }
}
