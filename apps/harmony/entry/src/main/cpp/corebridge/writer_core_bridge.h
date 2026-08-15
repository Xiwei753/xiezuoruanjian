#ifndef WRITER_CORE_BRIDGE_H
#define WRITER_CORE_BRIDGE_H

#include <cstdint>

#ifdef __cplusplus
extern "C" {
#endif

// ── Core lifecycle ──
// writer_core_init: Initialize core with app data root path. Returns 0 on success,
//   -1 if path is null/empty, -2 if projects dir creation failed, -3 if core state init failed.
// All char* return values are heap-allocated by core and MUST be freed via writer_core_free_string.
int32_t writer_core_init(const char* path);
char*  writer_core_get_load_status(void);
char*  writer_core_get_last_error(void);
int32_t writer_core_calculate_word_count(const char* text);
// writer_core_free_string: Free a char* previously returned by any writer_core_* function.
//   Passing null is safe (no-op). Must not be called twice on the same pointer.
void    writer_core_free_string(char* ptr);

// ── Layout Contract（#610） ──
// writer_core_resolve_layout: Input/output are JSON strings (ResultEnvelope<LayoutContract>).
// Input is WindowCapabilities: { availablePaneCount, hasSeparatingFold, pointerClass, keyboardVisible }.
char*  writer_core_resolve_layout(const char* capabilities_json);

// ── Screen Contract（#610：动作区域/顺序是产品语义，不随壳层变化） ──
// writer_core_resolve_screen_policy: Input/output are JSON strings (ResultEnvelope<ScreenPolicyDto>).
char*  writer_core_resolve_screen_policy(const char* screen_role_json);

// ── App State 查询 ──
// 新 Core API 边界：平台通过 writer_core_init 注入 app_data_root 与 projects_root，
// Core 不再创建/验证/打开 workspace。此处仅保留查询类 C ABI。
// All return ResultEnvelope JSON. Path arguments are UTF-8 strings.
char*  writer_core_list_app_summaries(void);
char*  writer_core_get_app_state(void);
char*  writer_core_resolve_chapter_location(const char* chapter_id);
char*  writer_core_resolve_volume_location(const char* volume_id);

// ── Project ──
char*  writer_core_list_projects(void);
char*  writer_core_get_project_tree(const char* project_id);
char*  writer_core_create_project(const char* name);

// ── Volume ──
char*  writer_core_list_volumes(const char* project_id);
char*  writer_core_create_volume(const char* project_id, const char* name);

// ── Chapter ──
char*  writer_core_list_chapters(const char* project_id, const char* volume_id);
char*  writer_core_create_chapter(const char* project_id, const char* volume_id, const char* name);
char*  writer_core_open_chapter(const char* project_id, const char* volume_id, const char* chapter_id);
char*  writer_core_save_chapter(const char* project_id, const char* volume_id, const char* chapter_id, const char* content);

// ── Recent edits ──
char*  writer_core_get_recent_edits(void);

// ── Settings ──
char*  writer_core_load_local_settings(void);
char*  writer_core_save_local_settings(const char* settings_json);
char*  writer_core_load_syncable_settings(void);
char*  writer_core_save_syncable_settings(const char* settings_json);

// ── Project mutations ──
char*  writer_core_rename_project(const char* project_id, const char* new_name);
char*  writer_core_delete_project(const char* project_id);
char*  writer_core_get_project_stats(const char* project_id);

// ── Volume mutations ──
char*  writer_core_rename_volume(const char* project_id, const char* volume_id, const char* new_name);
char*  writer_core_delete_volume(const char* project_id, const char* volume_id);
char*  writer_core_reorder_volumes(const char* project_id, const char* ordered_ids_json);

// ── Chapter mutations ──
char*  writer_core_rename_chapter(const char* project_id, const char* volume_id, const char* chapter_id, const char* new_name);
char*  writer_core_delete_chapter(const char* project_id, const char* volume_id, const char* chapter_id);
char*  writer_core_reorder_chapters(const char* project_id, const char* volume_id, const char* ordered_ids_json);
char*  writer_core_clear_chapter(const char* project_id, const char* volume_id, const char* chapter_id);

// ── StarMap ──
char*  writer_core_list_starmaps(void);
char*  writer_core_list_starmaps_for_project(const char* project_id);
char*  writer_core_get_starmap(const char* starmap_id);
char*  writer_core_get_starmap_graph(const char* starmap_id);
char*  writer_core_get_starmap_motion_policy(void);
char*  writer_core_get_starmap_layout(const char* starmap_id);
char*  writer_core_save_starmap_layout(const char* starmap_id, const char* layout_json);
char*  writer_core_save_starmap_viewport(const char* starmap_id, const char* viewport_json);
char*  writer_core_compute_starmap_edge_renders(const char* graph_json);
char*  writer_core_create_starmap(const char* title, const char* description);
char*  writer_core_delete_starmap(const char* starmap_id);
char*  writer_core_rename_starmap(const char* starmap_id, const char* new_title);

// ── Sync ──
// All sync functions use JSON-in/JSON-out via ResultEnvelope.
// All sync functions are per-project: project_id selects the sync root (Issue #600 评论 #3).
// writer_core_save_sync_config: project_id + SyncConfigDto JSON input.
char*  writer_core_load_sync_config(const char* project_id);
char*  writer_core_save_sync_config(const char* project_id, const char* config_json);
char*  writer_core_sync_dry_run(const char* project_id);
char*  writer_core_sync_diagnostics(const char* project_id);
char*  writer_core_perform_sync(const char* project_id);

// ── App-level sync (Issue #600 评论 #3/#4) — sync root = app_data_root ──
char*  writer_core_load_app_sync_config(void);
char*  writer_core_save_app_sync_config(const char* config_json);
char*  writer_core_app_sync_dry_run(void);
char*  writer_core_app_sync_diagnostics(void);
char*  writer_core_perform_app_sync(void);
char*  writer_core_load_app_sync_state(void);
char*  writer_core_save_app_sync_state(const char* state_json);

// ── Writing Stats ──
char*  writer_core_get_writing_stats(void);
char*  writer_core_process_writing_event(const char* event_json);

// ── Palette / Theme ──
char*  writer_core_list_palette_records(void);
char*  writer_core_load_palette_record(const char* device_id, const char* fingerprint);
char*  writer_core_delete_palette_record(const char* device_id, const char* fingerprint);
char*  writer_core_list_builtin_themes(void);

// ── Misc ──
int32_t writer_core_is_ai_available(void);

// ── Editor Session（TextEditSession C ABI）── 返回 JSON ResultEnvelope，须 writer_core_free_string 释放
char* writer_core_editor_session_create(const char* target_id, const char* initial_text, uint32_t initial_cursor_byte_offset, uint8_t is_persistent);
char* writer_core_editor_session_close(uint64_t session_id);
char* writer_core_editor_session_snapshot(uint64_t session_id);
char* writer_core_editor_session_insert(uint64_t session_id, uint32_t byte_offset, const char* text, const char* cause, uint64_t expected_revision);
char* writer_core_editor_session_delete(uint64_t session_id, uint32_t byte_start, uint32_t byte_end_exclusive, const char* cause, uint64_t expected_revision);
char* writer_core_editor_session_replace(uint64_t session_id, uint32_t byte_start, uint32_t byte_end_exclusive, const char* replacement_text, const char* original_text, const char* cause, uint64_t expected_revision);
char* writer_core_editor_session_set_selection(uint64_t session_id, uint32_t anchor_byte_offset, uint32_t head_byte_offset, uint64_t expected_revision);
char* writer_core_editor_session_undo(uint64_t session_id, uint64_t expected_revision);
char* writer_core_editor_session_redo(uint64_t session_id, uint64_t expected_revision);
char* writer_core_editor_session_commit_text(uint64_t session_id, uint32_t byte_start, uint32_t byte_end_exclusive, const char* replacement_text, uint32_t resulting_selection_anchor, uint32_t resulting_selection_head, uint64_t composition_session_id, uint64_t composition_base_revision, uint64_t composition_generation, const char* cause, uint64_t expected_revision);
char* writer_core_editor_session_delete_surrounding(uint64_t session_id, uint32_t before_byte_start, uint32_t before_byte_end_exclusive, uint32_t after_byte_start, uint32_t after_byte_end_exclusive, const char* cause, uint64_t expected_revision);
char* writer_core_editor_session_begin_composition(uint64_t session_id, uint32_t replace_start, uint32_t replace_end_exclusive, uint64_t expected_revision);
char* writer_core_editor_session_update_composition(uint64_t session_id, uint64_t composition_session_id, uint64_t composition_generation, const char* new_preedit_text, uint32_t new_preedit_cursor_offset, uint64_t expected_revision);
char* writer_core_editor_session_finish_composition(uint64_t session_id, uint64_t composition_session_id, uint64_t composition_generation, uint64_t expected_revision);
char* writer_core_editor_session_cancel_composition(uint64_t session_id, uint64_t composition_session_id, uint64_t composition_generation, uint64_t expected_revision);
char* writer_core_editor_session_get_text(uint64_t session_id);
char* writer_core_editor_session_get_revision(uint64_t session_id);

#ifdef __cplusplus
}
#endif

#endif
