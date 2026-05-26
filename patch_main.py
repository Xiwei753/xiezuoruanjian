import re

with open("apps/linux/src/main.rs", "r") as f:
    content = f.read()

new_methods = """
    fn delete_project_json(&mut self, project_id: QString) -> QString {
        self.error_message = "".into();
        self.delete_project(project_id);
        let success = self.error_message.to_string().is_empty();
        let msg = if success { "删除成功".to_string() } else { self.error_message.to_string() };
        let final_res = serde_json::json!({
            "success": success,
            "message": msg,
            "state": serde_json::from_str::<serde_json::Value>(&self.refresh_app_state_json().to_string()).unwrap_or_default()
        });
        final_res.to_string().into()
    }

    fn delete_volume_json(&mut self, project_id: QString, volume_id: QString) -> QString {
        self.error_message = "".into();
        self.delete_volume(project_id, volume_id);
        let success = self.error_message.to_string().is_empty();
        let msg = if success { "删除成功".to_string() } else { self.error_message.to_string() };
        let final_res = serde_json::json!({
            "success": success,
            "message": msg,
            "state": serde_json::from_str::<serde_json::Value>(&self.refresh_app_state_json().to_string()).unwrap_or_default()
        });
        final_res.to_string().into()
    }

    fn delete_chapter_json(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) -> QString {
        self.error_message = "".into();
        self.delete_chapter(project_id, volume_id, chapter_id);
        let success = self.error_message.to_string().is_empty();
        let msg = if success { "删除成功".to_string() } else { self.error_message.to_string() };
        let final_res = serde_json::json!({
            "success": success,
            "message": msg,
            "state": serde_json::from_str::<serde_json::Value>(&self.refresh_app_state_json().to_string()).unwrap_or_default()
        });
        final_res.to_string().into()
    }
"""

# insert after select_tree_item_json
target = "        final_res.to_string().into()\n    }"
parts = content.split("fn select_tree_item_json", 1)
subparts = parts[1].split(target, 1)

new_content = parts[0] + "fn select_tree_item_json" + subparts[0] + target + new_methods + subparts[1]

with open("apps/linux/src/main.rs", "w") as f:
    f.write(new_content)

