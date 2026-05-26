import re

with open("apps/linux/src/main.rs", "r") as f:
    content = f.read()

# Add the new properties/methods to AppBackend declaration
new_methods = """
    refresh_app_state_json: qt_method!(fn(&mut self) -> QString),
    create_project_json: qt_method!(fn(&mut self, title: QString) -> QString),
    create_volume_json: qt_method!(fn(&mut self, project_id: QString, title: QString) -> QString),
    create_chapter_json: qt_method!(fn(&mut self, project_id: QString, volume_id: QString, title: QString) -> QString),
    select_tree_item_json: qt_method!(fn(&mut self, item_type: QString, project_id: QString, volume_id: QString, chapter_id: QString) -> QString),
"""

if "refresh_app_state_json:" not in content:
    content = content.replace("get_tree_model_json: qt_method!(fn(&self) -> QString),", "get_tree_model_json: qt_method!(fn(&self) -> QString)," + new_methods)


# Add implementation for the new methods
impl_code = """
    fn refresh_app_state_json(&mut self) -> QString {
        self.reload_tree();
        let tree_json = self.build_tree_model_json();
        let state = serde_json::json!({
            "hasWorkspace": self.current_has_workspace,
            "workspacePath": self.current_workspace,
            "saveStatus": self.current_save_status,
            "selected": {
                "projectId": self.selected_project_id.clone().unwrap_or_default(),
                "volumeId": self.selected_volume_id.clone().unwrap_or_default(),
                "chapterId": self.selected_chapter_id.clone().unwrap_or_default()
            },
            "tree": tree_json,
            "settings": {
                "fontSize": self.current_setting_font_size,
                "themeMode": self.setting_theme_mode().to_string()
            },
            "sync": {
                "status": self.current_sync_status
            }
        });
        state.to_string().into()
    }

    fn create_project_json(&mut self, title: QString) -> QString {
        let res = self.create_new_project(title.clone());
        let parsed: serde_json::Value = serde_json::from_str(&res.to_string()).unwrap_or_else(|_| serde_json::json!({"success": false}));
        let success = parsed["success"].as_bool().unwrap_or(false);
        let msg = parsed["message"].as_str().unwrap_or("").to_string();
        
        let mut final_res = serde_json::json!({
            "success": success,
            "message": msg,
            "state": serde_json::from_str::<serde_json::Value>(&self.refresh_app_state_json().to_string()).unwrap_or_default()
        });
        final_res.to_string().into()
    }

    fn create_volume_json(&mut self, project_id: QString, title: QString) -> QString {
        self.create_new_volume(project_id.clone(), title.clone());
        let final_res = serde_json::json!({
            "success": true,
            "message": "创建卷成功",
            "state": serde_json::from_str::<serde_json::Value>(&self.refresh_app_state_json().to_string()).unwrap_or_default()
        });
        final_res.to_string().into()
    }

    fn create_chapter_json(&mut self, project_id: QString, volume_id: QString, title: QString) -> QString {
        self.create_new_chapter(project_id.clone(), volume_id.clone(), title.clone());
        let final_res = serde_json::json!({
            "success": true,
            "message": "创建章节成功",
            "state": serde_json::from_str::<serde_json::Value>(&self.refresh_app_state_json().to_string()).unwrap_or_default()
        });
        final_res.to_string().into()
    }

    fn select_tree_item_json(&mut self, item_type: QString, project_id: QString, volume_id: QString, chapter_id: QString) -> QString {
        let t = item_type.to_string();
        if t == "project" {
            self.select_project(project_id);
        } else if t == "volume" {
            self.select_volume(project_id, volume_id);
        } else if t == "chapter" {
            self.select_chapter(project_id, volume_id, chapter_id);
        }
        let final_res = serde_json::json!({
            "success": true,
            "message": "选择成功",
            "state": serde_json::from_str::<serde_json::Value>(&self.refresh_app_state_json().to_string()).unwrap_or_default()
        });
        final_res.to_string().into()
    }
"""

if "fn refresh_app_state_json(" not in content:
    # insert before fn get_tree_model(&self)
    content = content.replace("fn get_tree_model(&self) -> QJsonArray {", impl_code + "\n    fn get_tree_model(&self) -> QJsonArray {")


with open("apps/linux/src/main.rs", "w") as f:
    f.write(content)

print("Updated main.rs successfully")
