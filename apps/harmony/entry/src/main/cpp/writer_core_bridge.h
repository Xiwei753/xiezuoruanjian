#ifndef WRITER_CORE_BRIDGE_H
#define WRITER_CORE_BRIDGE_H

#include <cstdint>

#ifdef __cplusplus
extern "C" {
#endif

// ── Core lifecycle ──
int32_t writer_core_init(const char* path);
char*  writer_core_get_load_status(void);
char*  writer_core_get_last_error(void);
int32_t writer_core_calculate_word_count(const char* text);
void    writer_core_free_string(char* ptr);

// ── Layout Policy ──
char*  writer_core_resolve_layout(const char* metrics_json);

// ── Workspace ──
char*  writer_core_validate_workspace(void);
char*  writer_core_list_workspaces(void);
char*  writer_core_open_workspace(const char* path);
char*  writer_core_get_workspace_state(void);
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
char*  writer_core_create_starmap(const char* title, const char* description);
char*  writer_core_delete_starmap(const char* starmap_id);
char*  writer_core_rename_starmap(const char* starmap_id, const char* new_title);

// ── Sync ──
char*  writer_core_load_sync_config(void);
char*  writer_core_save_sync_config(const char* config_json);
char*  writer_core_sync_dry_run(void);
char*  writer_core_sync_diagnostics(void);
char*  writer_core_perform_sync(void);

// ── Writing Stats ──
char*  writer_core_get_writing_stats(void);
char*  writer_core_process_writing_event(const char* event_json);

// ── Misc ──
int32_t writer_core_is_ai_available(void);

#ifdef __cplusplus
}
#endif

#endif
