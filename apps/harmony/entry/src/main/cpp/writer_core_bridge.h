#ifndef WRITER_CORE_BRIDGE_H
#define WRITER_CORE_BRIDGE_H

#include <cstdint>

#ifdef __cplusplus
extern "C" {
#endif

// ── Core lifecycle ──
int32_t writer_core_init(const char* path);
char*  writer_core_get_load_status(void);
int32_t writer_core_calculate_word_count(const char* text);
void    writer_core_free_string(char* ptr);

// ── Workspace ──
char*  writer_core_validate_workspace(void);

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

#ifdef __cplusplus
}
#endif

#endif
