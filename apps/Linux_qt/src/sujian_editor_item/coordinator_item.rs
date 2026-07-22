use super::linux_coordinator::{
    EditableTextTarget, LinuxTextEditorCoordinator, TextEditorProfile,
    TextInputType, SecretPolicy, AutocorrectPolicy, CapitalizationPolicy,
    CopyPolicy, PastePolicy, SelectionPolicy,
};
use qmetaobject::prelude::*;
use qmetaobject::QString;

#[derive(QObject)]
pub struct TextEditorCoordinatorItem {
    #[allow(dead_code)]
    base: qt_base_class!(trait QObject),

    #[allow(dead_code)]
    register_target: qt_method!(fn(&mut self, target_id: QString, is_persistent: bool, initial_text: QString)),
    #[allow(dead_code)]
    register_secret_target: qt_method!(fn(&mut self, target_id: QString, is_persistent: bool, initial_text: QString)),
    #[allow(dead_code)]
    register_search_target: qt_method!(fn(&mut self, target_id: QString, initial_text: QString)),
    #[allow(dead_code)]
    register_url_target: qt_method!(fn(&mut self, target_id: QString, initial_text: QString)),
    #[allow(dead_code)]
    unregister_target: qt_method!(fn(&mut self, target_id: QString)),
    #[allow(dead_code)]
    begin_edit: qt_method!(fn(&mut self, target_id: QString) -> bool),
    #[allow(dead_code)]
    commit_edit: qt_method!(fn(&mut self) -> bool),
    #[allow(dead_code)]
    cancel_edit: qt_method!(fn(&mut self) -> bool),
    #[allow(dead_code)]
    update_text: qt_method!(fn(&mut self, target_id: QString, text: QString)),
    #[allow(dead_code)]
    get_active_target_id: qt_method!(fn(&self) -> QString),

    coordinator: LinuxTextEditorCoordinator,
}

impl Default for TextEditorCoordinatorItem {
    fn default() -> Self {
        Self {
            base: Default::default(),
            register_target: Default::default(),
            register_secret_target: Default::default(),
            register_search_target: Default::default(),
            register_url_target: Default::default(),
            unregister_target: Default::default(),
            begin_edit: Default::default(),
            commit_edit: Default::default(),
            cancel_edit: Default::default(),
            update_text: Default::default(),
            get_active_target_id: Default::default(),
            coordinator: LinuxTextEditorCoordinator::new(),
        }
    }
}

impl TextEditorCoordinatorItem {
    pub fn register_target(&mut self, target_id: QString, is_persistent: bool, initial_text: QString) {
        let target = EditableTextTarget {
            target_id: target_id.to_string(),
            is_persistent,
            current_text: initial_text.to_string(),
            profile: TextEditorProfile::short_title(),
        };
        self.coordinator.register_target(target);
    }

    pub fn register_secret_target(&mut self, target_id: QString, is_persistent: bool, initial_text: QString) {
        let target = EditableTextTarget {
            target_id: target_id.to_string(),
            is_persistent,
            current_text: initial_text.to_string(),
            profile: TextEditorProfile::secret_token(),
        };
        self.coordinator.register_target(target);
    }

    pub fn register_search_target(&mut self, target_id: QString, initial_text: QString) {
        let target = EditableTextTarget {
            target_id: target_id.to_string(),
            is_persistent: false,
            current_text: initial_text.to_string(),
            profile: TextEditorProfile::search_query(),
        };
        self.coordinator.register_target(target);
    }

    pub fn register_url_target(&mut self, target_id: QString, initial_text: QString) {
        let target = EditableTextTarget {
            target_id: target_id.to_string(),
            is_persistent: false,
            current_text: initial_text.to_string(),
            profile: TextEditorProfile::repository_url(),
        };
        self.coordinator.register_target(target);
    }

    pub fn unregister_target(&mut self, target_id: QString) {
        self.coordinator.unregister_target(&target_id.to_string());
    }

    pub fn begin_edit(&mut self, target_id: QString) -> bool {
        self.coordinator.begin_edit(&target_id.to_string())
    }

    pub fn commit_edit(&mut self) -> bool {
        self.coordinator.commit_active_edit()
    }

    pub fn cancel_edit(&mut self) -> bool {
        self.coordinator.cancel_active_edit()
    }

    pub fn update_text(&mut self, target_id: QString, text: QString) {
        self.coordinator.update_target_text(&target_id.to_string(), text.to_string());
    }

    pub fn get_active_target_id(&self) -> QString {
        QString::from(self.coordinator.active_target_id().unwrap_or_default())
    }
}
