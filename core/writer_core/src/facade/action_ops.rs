use crate::action_registry::{ActionDescriptor, ActionRegistry, ActionResult};

use serde_json::Value;

impl super::WriterCore {
    pub fn list_registered_actions(&self) -> crate::error::Result<Vec<ActionDescriptor>> {
        let registry = ActionRegistry::new();
        Ok(registry.list_registered_actions())
    }

    pub fn get_action(&self, action_id: &str) -> crate::error::Result<Option<ActionDescriptor>> {
        let registry = ActionRegistry::new();
        Ok(registry.get_action(action_id))
    }

    #[allow(clippy::cast_possible_truncation)]
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn execute_action(
        &self,
        action_id: &str,
        args_json: &str,
        _context_json: &str,
    ) -> crate::error::Result<ActionResult> {
        let registry = ActionRegistry::new();
        let _action = registry.get_action(action_id).ok_or_else(|| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Action not found: {}", action_id),
            ))
        })?;

        let args: Value = if args_json.trim().is_empty() {
            Value::Null
        } else {
            match serde_json::from_str(args_json) {
                Ok(v) => v,
                Err(_) => {
                    return Ok(ActionResult {
                        success: false,
                        message: Some("invalid args json".to_string()),
                        data: None,
                        proposed_ui: None,
                        requires_confirmation: None,
                    })
                }
            }
        };

        match action_id {
            "settings.editor.font_size.get" => {
                let font_size = crate::settings::get_effective_font_size(&self.app_data_root);
                Ok(ActionResult {
                    success: true,
                    message: None,
                    data: Some(serde_json::json!({ "fontSize": font_size, "source": "syncable" })),
                    proposed_ui: None,
                    requires_confirmation: None,
                })
            }
            "settings.editor.font_size.set" => {
                if let Some(font_size) = args.get("fontSize").and_then(|v| v.as_f64()) {
                    if !(10.0..=72.0).contains(&font_size) {
                        return Ok(ActionResult {
                            success: false,
                            message: Some("Font size must be between 10 and 72".to_string()),
                            data: None,
                            proposed_ui: None,
                            requires_confirmation: None,
                        });
                    }
                    crate::settings::set_editor_font_size(&self.app_data_root, font_size)?;
                    Ok(ActionResult {
                        success: true,
                        message: Some("Font size updated".to_string()),
                        data: Some(
                            serde_json::json!({ "fontSize": font_size, "source": "syncable" }),
                        ),
                        proposed_ui: None,
                        requires_confirmation: None,
                    })
                } else {
                    Ok(ActionResult {
                        success: false,
                        message: Some("Missing or invalid fontSize parameter".to_string()),
                        data: None,
                        proposed_ui: None,
                        requires_confirmation: None,
                    })
                }
            }
            "settings.editor.auto_save.get" => {
                let settings = self.load_local_settings()?;
                Ok(ActionResult {
                    success: true,
                    message: None,
                    data: Some(serde_json::json!({ "enabled": settings.auto_save_enabled })),
                    proposed_ui: None,
                    requires_confirmation: None,
                })
            }
            "settings.editor.auto_save.set" => {
                if let Some(enabled) = args.get("enabled").and_then(|v| v.as_bool()) {
                    let mut settings = self.load_local_settings()?;
                    settings.auto_save_enabled = enabled;
                    self.save_local_settings(&settings)?;
                    Ok(ActionResult {
                        success: true,
                        message: Some("Auto save setting updated".to_string()),
                        data: None,
                        proposed_ui: None,
                        requires_confirmation: None,
                    })
                } else {
                    Ok(ActionResult {
                        success: false,
                        message: Some("Missing or invalid enabled parameter".to_string()),
                        data: None,
                        proposed_ui: None,
                        requires_confirmation: None,
                    })
                }
            }
            "settings.editor.auto_save_delay.set" => {
                if let Some(delay) = args.get("delayMs").and_then(|v| v.as_u64()) {
                    if !(500..=10000).contains(&delay) {
                        return Ok(ActionResult {
                            success: false,
                            message: Some("Delay must be between 500 and 10000".to_string()),
                            data: None,
                            proposed_ui: None,
                            requires_confirmation: None,
                        });
                    }
                    let mut settings = self.load_local_settings()?;
                    settings.auto_save_delay_ms = delay;
                    self.save_local_settings(&settings)?;
                    Ok(ActionResult {
                        success: true,
                        message: Some("Auto save delay updated".to_string()),
                        data: None,
                        proposed_ui: None,
                        requires_confirmation: None,
                    })
                } else {
                    Ok(ActionResult {
                        success: false,
                        message: Some("Missing or invalid delayMs parameter".to_string()),
                        data: None,
                        proposed_ui: None,
                        requires_confirmation: None,
                    })
                }
            }
            "settings.sync.config.get" => {
                let config = self.load_sync_config()?;
                Ok(ActionResult {
                    success: true,
                    message: None,
                    data: Some(serde_json::to_value(config).unwrap_or(Value::Null)),
                    proposed_ui: None,
                    requires_confirmation: None,
                })
            }
            "sync.diagnostics.run" => {
                let config = self.load_sync_config()?;
                let secrets = self.load_sync_secrets()?;
                let mut diagnostics_config = config.clone();
                diagnostics_config.enabled = true;
                let token = secrets.token.clone().unwrap_or_default();
                let mut secrets_for_diag = secrets.clone();
                secrets_for_diag.token = Some(token);
                let result = crate::sync::SyncService::perform_sync_diagnostics(
                    &diagnostics_config,
                    &secrets_for_diag,
                );
                match result {
                    Ok(diag) => Ok(ActionResult {
                        success: true,
                        message: Some("Diagnostics completed".to_string()),
                        data: Some(serde_json::to_value(diag).unwrap_or(Value::Null)),
                        proposed_ui: None,
                        requires_confirmation: None,
                    }),
                    Err(e) => Ok(ActionResult {
                        success: false,
                        message: Some(format!("Diagnostics failed: {}", e)),
                        data: None,
                        proposed_ui: None,
                        requires_confirmation: None,
                    }),
                }
            }
            "sync.plan.preview" => {
                let project_id = args
                    .get("projectId")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        crate::Error::Io(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "Missing or invalid projectId parameter",
                        ))
                    })?;
                let config = self.load_sync_config()?;
                let plan_result = self.perform_sync_dry_run(project_id, &config);
                match plan_result {
                    Ok(plan) => Ok(ActionResult {
                        success: true,
                        message: Some("Plan calculated".to_string()),
                        data: Some(serde_json::to_value(plan).unwrap_or(Value::Null)),
                        proposed_ui: None,
                        requires_confirmation: None,
                    }),
                    Err(e) => Ok(ActionResult {
                        success: false,
                        message: Some(format!("Plan calculation failed: {}", e)),
                        data: None,
                        proposed_ui: None,
                        requires_confirmation: None,
                    }),
                }
            }
            "settings.editor.line_spacing.get" => {
                let settings = self.load_local_settings()?;
                Ok(ActionResult {
                    success: true,
                    message: None,
                    data: Some(
                        serde_json::json!({ "multiplier": settings.editor_line_spacing_multiplier }),
                    ),
                    proposed_ui: None,
                    requires_confirmation: None,
                })
            }
            "settings.editor.line_spacing.set" => {
                if let Some(multiplier) = args.get("multiplier").and_then(|v| v.as_f64()) {
                    if !(1.0..=3.0).contains(&multiplier) {
                        return Ok(ActionResult {
                            success: false,
                            message: Some(
                                "Line spacing multiplier must be between 1.0 and 3.0".to_string(),
                            ),
                            data: None,
                            proposed_ui: None,
                            requires_confirmation: None,
                        });
                    }
                    let mut settings = self.load_local_settings()?;
                    settings.editor_line_spacing_multiplier = multiplier as f32;
                    self.save_local_settings(&settings)?;
                    Ok(ActionResult {
                        success: true,
                        message: Some("Line spacing updated".to_string()),
                        data: None,
                        proposed_ui: None,
                        requires_confirmation: None,
                    })
                } else {
                    Ok(ActionResult {
                        success: false,
                        message: Some("Missing or invalid multiplier parameter".to_string()),
                        data: None,
                        proposed_ui: None,
                        requires_confirmation: None,
                    })
                }
            }
            "settings.editor.auto_indent.get" => {
                let settings = self.load_local_settings()?;
                Ok(ActionResult {
                    success: true,
                    message: None,
                    data: Some(
                        serde_json::json!({ "enabled": settings.auto_indent_enabled, "widthChars": settings.auto_indent_width }),
                    ),
                    proposed_ui: None,
                    requires_confirmation: None,
                })
            }
            "settings.editor.auto_indent.set" => {
                if let Some(enabled) = args.get("enabled").and_then(|v| v.as_bool()) {
                    let mut settings = self.load_local_settings()?;
                    settings.auto_indent_enabled = enabled;
                    if let Some(width) = args.get("widthChars").and_then(|v| v.as_f64()) {
                        settings.auto_indent_width = width as f32;
                    }
                    self.save_local_settings(&settings)?;
                    Ok(ActionResult {
                        success: true,
                        message: Some("Auto indent updated".to_string()),
                        data: None,
                        proposed_ui: None,
                        requires_confirmation: None,
                    })
                } else {
                    Ok(ActionResult {
                        success: false,
                        message: Some("Missing or invalid enabled parameter".to_string()),
                        data: None,
                        proposed_ui: None,
                        requires_confirmation: None,
                    })
                }
            }
            "settings.editor.typing_animation.get" => {
                let settings = self.load_local_settings()?;
                Ok(ActionResult {
                    success: true,
                    message: None,
                    data: Some(serde_json::json!({
                        "enabled": settings.editor_typing_animation_enabled,
                        "durationMs": settings.editor_typing_animation_duration_ms
                    })),
                    proposed_ui: None,
                    requires_confirmation: None,
                })
            }
            "settings.editor.typing_animation.set" => {
                let mut settings = self.load_local_settings()?;
                let mut modified = false;
                if let Some(enabled) = args.get("enabled").and_then(|v| v.as_bool()) {
                    settings.editor_typing_animation_enabled = enabled;
                    modified = true;
                }
                if let Some(duration) = args.get("durationMs").and_then(|v| v.as_u64()) {
                    settings.editor_typing_animation_duration_ms = duration;
                    modified = true;
                }
                if modified {
                    self.save_local_settings(&settings)?;
                }
                Ok(ActionResult {
                    success: true,
                    message: Some("Typing animation updated".to_string()),
                    data: None,
                    proposed_ui: None,
                    requires_confirmation: None,
                })
            }
            "settings.editor.smooth_cursor.get" => {
                let settings = self.load_local_settings()?;
                Ok(ActionResult {
                    success: true,
                    message: None,
                    data: Some(serde_json::json!({
                        "enabled": settings.editor_smooth_cursor_enabled,
                        "durationMs": settings.editor_smooth_cursor_duration_ms
                    })),
                    proposed_ui: None,
                    requires_confirmation: None,
                })
            }
            "settings.editor.smooth_cursor.set" => {
                let mut settings = self.load_local_settings()?;
                let mut modified = false;
                if let Some(enabled) = args.get("enabled").and_then(|v| v.as_bool()) {
                    settings.editor_smooth_cursor_enabled = enabled;
                    modified = true;
                }
                if let Some(duration) = args.get("durationMs").and_then(|v| v.as_u64()) {
                    settings.editor_smooth_cursor_duration_ms = duration;
                    modified = true;
                }
                if modified {
                    self.save_local_settings(&settings)?;
                }
                Ok(ActionResult {
                    success: true,
                    message: Some("Smooth cursor updated".to_string()),
                    data: None,
                    proposed_ui: None,
                    requires_confirmation: None,
                })
            }
            _ => Ok(ActionResult {
                success: false,
                message: Some(format!("Action execution not implemented: {}", action_id)),
                data: None,
                proposed_ui: None,
                requires_confirmation: None,
            }),
        }
    }
}
