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

// ── Layout Policy ──
// writer_core_resolve_layout: Input/output are JSON strings (ResultEnvelope<LayoutPolicyDto>).
char*  writer_core_resolve_layout(const char* metrics_json);

// ── Screen Policy ──
// writer_core_resolve_screen_policy: Input/output are JSON strings (ResultEnvelope<ScreenPolicyDto>).
char*  writer_core_resolve_screen_policy(const char* screen_role_json, const char* shell_mode_json);

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

#ifdef __cplusplus
}
#endif

#endif
