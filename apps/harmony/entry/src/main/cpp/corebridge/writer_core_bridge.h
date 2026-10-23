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
// writer_core_init_harmony: Harmony-specific init with PlatformServices injection.
//   Injects SyncTransportFactory for GitHub API sync support.
//   Returns 0 on success, -1 if app_data_root is null, -2 if invalid UTF-8,
//   -4 if bootstrap failed, -5 if app_data_root is empty.
//   is_connected: 1 = online, 0 = offline, -1 = unknown (query failed).
//   is_metered: 1 = metered, 0 = unmetered, -1 = unknown.
//   未知连通性不会注入伪造的在线状态（network_state 记为 None）。
int32_t writer_core_init_harmony(const char* app_data_root, const char* files_dir, const char* cache_dir, const char* device_id, const char* app_version, const char* locale, const char* timezone, int32_t is_connected, int32_t is_metered);
// writer_core_update_network_state: Update the Core's cached network state from the
//   platform's current query result. Three-state: 1 = yes, 0 = no, -1 = unknown
//   (clears the cached state, does not fabricate online). Safe to call before init
//   (returns -3). Returns 0 on success, -3 if the service is not initialized.
int32_t writer_core_update_network_state(int32_t is_connected, int32_t is_metered);
char*  writer_core_get_load_status(void);
char*  writer_core_get_last_error(void);
int32_t writer_core_calculate_word_count(const char* text);
// writer_core_free_string: Free a char* previously returned by any writer_core_* function.
//   Passing null is safe (no-op). Must not be called twice on the same pointer.
void    writer_core_free_string(char* ptr);

// ── Diagnostics Init ──
// writer_core_init_diagnostics: Initialize diagnostics backend. Returns 0 on success,
//   -1 if any argument is null, -2 if any argument contains invalid UTF-8,
//   -3 if local settings could not be read/parsed.
//   session_id is generated inside Rust (uuid v4); enabled/verbose are read from the
//   Core LocalSettings under app_data_root (settings.local.json), not hardcoded.
//   Idempotent: repeated calls have no side effect.
int32_t writer_core_init_diagnostics(const char* log_dir, const char* app_data_root, const char* device_id, const char* app_version, const char* build_key, const char* locale, const char* timezone);

// ── Diagnostics operations ──
int32_t writer_core_set_diagnostics_config(int32_t enabled, int32_t verbose);
int32_t writer_core_flush_diagnostics(void);
int32_t writer_core_clear_diagnostics(void);
char*  writer_core_export_diagnostics(const char* output_dir, const char* attachments_json);
// writer_core_record_diagnostic_event_json: Record one structured diagnostic event.
//   payload_json is a NUL-terminated UTF-8 JSON string with fields
//   { level, origin, event, target, message?, fields:[{key,value}] }.
//   Returns 0 on success, -1 null pointer, -2 invalid UTF-8, -3 JSON parse failure,
//   -4 if the core failed to record the event (defensive; not expected today).
int32_t writer_core_record_diagnostic_event_json(const char* payload_json);

// ── App State 查询 ──
// 新 Core API 边界：平台通过 writer_core_init 注入 app_data_root 与 projects_root，
// Core 不再创建/验证/打开 workspace。此处仅保留查询类 C ABI。
// All return ResultEnvelope JSON. Path arguments are UTF-8 strings.
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

// ── Global search ──
// writer_core_global_search_page: 统一全局搜索（作品/卷/章/星图/设置）。
//   query/scope 为 UTF-8 字符串；scope 取 chapterBody、chapterTitle、chapterNote、
//   projectTitle、volumeTitle、starmapTitle、starmapNode、starmapEdgeLabel、
//   starmapHyperlink、starmapLink、starmapEmbed、setting 或 all。
//   cursor 为空时从第一页开始；context_json 为 SearchContext camelCase JSON（可为 null）。
//   返回 ResultEnvelope JSON，data 为 { items, nextCursor }；正文 byteStart/byteEnd
//   是原文 UTF-8 字节偏移。返回值由 Core 分配，必须用 writer_core_free_string 释放。
char*  writer_core_global_search_page(const char* query, const char* scope, uint32_t limit,
                                      const char* cursor, const char* context_json);

// ── Settings ──
char*  writer_core_load_local_settings(void);
char*  writer_core_save_local_settings(const char* settings_json);
char*  writer_core_load_syncable_settings(void);
char*  writer_core_save_syncable_settings(const char* settings_json);

// ── Project mutations ──
char*  writer_core_rename_project(const char* project_id, const char* new_name);
char*  writer_core_delete_project(const char* project_id);
char*  writer_core_get_project_stats(const char* project_id);
// 单作品写作档案（#934）：开写日期 / 累计人工码字 / 单日最高纪录 / 写作天数。
// 返回 ResultEnvelope JSON；firstWrittenAt / bestWritingDay 可为 null。
char*  writer_core_get_project_writing_lifetime_stats(const char* project_id);

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
char*  writer_core_list_root_starmaps(void);
char*  writer_core_list_starmaps_for_project(const char* project_id);
char*  writer_core_get_starmap(const char* starmap_id);
char*  writer_core_get_starmap_graph(const char* starmap_id);
char*  writer_core_create_starmap(const char* title, const char* description);
char*  writer_core_delete_starmap(const char* starmap_id);
char*  writer_core_rename_starmap(const char* starmap_id, const char* new_title);
char*  writer_core_update_starmap_node(const char* starmap_id, const char* node_id, const char* patch_json);
char*  writer_core_add_starmap_edge(const char* starmap_id, const char* edge_json);
char*  writer_core_add_starmap_node(const char* starmap_id, const char* node_json, float x, float y);
char*  writer_core_add_starmap_embed(const char* starmap_id, const char* embed_json);
char*  writer_core_create_starmap_child_embed(const char* host_starmap_id, const char* title, const char* position_json);
char*  writer_core_update_starmap_embed(const char* starmap_id, const char* instance_id, const char* patch_json);
char*  writer_core_resolve_starmap_path(const char* root_starmap_id, const char* segments_json);
// 跨层连线宿主规划：入参是 {"from": StarMapTargetPathDto, "to": StarMapTargetPathDto}
// 的 JSON，返回含 hostStarmapId / hostSegments / from / to 的 ResultEnvelope。
char*  writer_core_plan_cross_layer_connection(const char* json);

// ── Sync ──
// All sync functions use JSON-in/JSON-out via ResultEnvelope.
// 全量同步统一入口（Issue #630）：一个全局 SyncConfig + 一份全局凭据，
// App target + 所有 Project target 一次同步。旧的 per-project sync /
// app-level sync 双套 C ABI 已删除，对应声明不再保留。
char*  writer_core_load_sync_config(void);
char*  writer_core_save_sync_config(const char* config_json);
char*  writer_core_load_sync_secrets(void);
char*  writer_core_save_sync_secrets(const char* secrets_json);
char*  writer_core_full_sync_dry_run(void);
char*  writer_core_full_sync_diagnostics(void);
char*  writer_core_perform_full_sync(void);
char*  writer_core_load_app_sync_state(void);
char*  writer_core_save_app_sync_state(const char* state_json);
// writer_core_load_full_sync_state: 读取 full_state.local.json（最近一次全量同步的总体结果）。
//   Core 尚未写入任何结果时 data 为 null，表示"从未成功同步"，不是查询失败。
char*  writer_core_load_full_sync_state(void);
// writer_core_load_latest_full_sync_report: 读取最近一次整轮同步报告（含脱敏失败详情）。
//   Core 尚未持久化报告时 data 为 null，不代表查询失败。
char*  writer_core_load_latest_full_sync_report(void);
// writer_core_list_all_sync_conflicts: 聚合 App target + 所有 Project target 的
//   未解决冲突摘要（AllSyncConflictEntryDto[]，不含正文/快照）。
char*  writer_core_list_all_sync_conflicts(void);
// writer_core_load_sync_conflict_preview: 单条冲突的本地/远端内容预览。
//   target_kind 为 "app" / "project"；project_id 允许 null 或空串（App target）。
//   remote_deleted 为 true 时 remoteContent 为 null，不伪造空字符串。
char*  writer_core_load_sync_conflict_preview(const char* target_kind, const char* project_id, const char* path);
// writer_core_resolve_sync_conflict: 按用户决定解决冲突。
//   decision 只接受 "keep_local" / "take_remote" / "mark_merged"，映射到现有
//   Core API，不在 FFI 层实现合并策略；target_kind == "project" 时 project_id 必填。
char*  writer_core_resolve_sync_conflict(const char* target_kind, const char* project_id, const char* path, const char* decision);

// ── Writing Stats ──
char*  writer_core_get_writing_stats(void);
// 「今天」的写作汇总：无参数。「今天是哪一天」由 Core 的本地日历口径决定，
// 平台端不传日期——每日统计按事件发生地的本地午夜分桶，端侧自己拼日期
// 会在凌晨和 Core 错开一天。
char*  writer_core_get_today_writing_stats_summary(void);
char*  writer_core_get_writing_stats_summary(const char* start_date, const char* end_date);
// 统计页 Explorer 快照（#944）：生涯摘要 + 四张时段卡 + 当前选中时段详情。
//   preset 固定为 "last7Days" / "last30Days" / "last6Months" / "lifetime" / "custom"；
//   custom_start_date / custom_end_date 仅在 preset == "custom" 时是用户所选日期
//   （按业务日 YYYY-MM-DD 原样透传，不把未来结束日改写成今天），其余可为 NULL 或空串。
char*  writer_core_get_writing_stats_explorer(const char* preset, const char* custom_start_date, const char* custom_end_date);
// 每日码字日历（#956）：指定公历年（1970–2100）的逐日人工输入真值快照。
//   一年返回 365/366 项，按日期升序；没有记录的已过日期返回 0，未来日期
//   由日期字段交给 UI 标记「未到」；intensity 0–4 由 Core 按当年正数日
//   分布统一分级，端侧不做二次计算。非法年份返回 INVALID_ARGUMENT。
char*  writer_core_get_writing_stats_heatmap(int32_t year);
char*  writer_core_get_writing_speed_curve(const char* start_date, const char* end_date, uint32_t bucket_minutes);
// 实时写作速度：与上面的历史曲线分工，速度由 Core 以「最近 window_seconds 秒」重算。
char*  writer_core_get_current_writing_speed(uint32_t window_seconds);
// 按编辑事务上报写作统计（Harmony 走这条）。cause→source 的映射在 Core 侧做，
// C 层只透传编辑事实。event_json 里带平台端固定并重发的 eventId/occurredAtMs，
// Core 按 eventId 幂等去重。
bool  writer_core_record_editor_change_stats(const char* event_json);
// 上报一条已测量的前台编辑器写作计时切片。平台只给 session_id / 起止毫秒 /
// 项目卷章 / 暂停原因与固定 event_id；device_id、业务日由 Core 补齐。Core 按
// event_id 幂等去重，同一设备重叠切片按并集计。
bool  writer_core_record_writing_activity_segment(const char* segment_json);
// 正文保存 + 未确认统计事件一次提交：先走正文保存事务，成功后再幂等记录
// events_json（[EditorChangeStatsInputDto] 数组，可为 NULL/空串）里的事件。
// 正文保存失败则不记任何事件；重复提交同一批事件不会重复计字。
char*  writer_core_save_chapter_with_stats(const char* project_id, const char* volume_id, const char* chapter_id, const char* content, const char* events_json);

// ── Palette / Theme ──
char*  writer_core_list_palette_records(void);
char*  writer_core_load_palette_record(const char* device_id, const char* fingerprint);
char*  writer_core_delete_palette_record(const char* device_id, const char* fingerprint);
char*  writer_core_list_builtin_themes(void);

// ── Misc ──
int32_t writer_core_is_ai_available(void);

// ── Editor Session（TextEditSession C ABI）── 返回 JSON ResultEnvelope，须 writer_core_free_string 释放
char* writer_core_editor_session_create(const char* target_id, const char* initial_text, uint32_t initial_cursor_byte_offset);
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
char* writer_core_editor_session_previous_grapheme_boundary(uint64_t session_id, uint32_t byte_offset);
char* writer_core_editor_session_next_grapheme_boundary(uint64_t session_id, uint32_t byte_offset);

// #629 R8: composition 专用 grapheme 语义操作。
// 只改 composition session 的 preeditText / preeditCursorUtf16 / generation；
// 不修改 committed 正文，不把 raw platform event 带入 Core。
char* writer_core_editor_session_composition_move_grapheme_left(uint64_t session_id, uint64_t composition_session_id, uint64_t composition_generation, uint64_t expected_revision);
char* writer_core_editor_session_composition_move_grapheme_right(uint64_t session_id, uint64_t composition_session_id, uint64_t composition_generation, uint64_t expected_revision);
char* writer_core_editor_session_composition_delete_grapheme_backward(uint64_t session_id, uint64_t composition_session_id, uint64_t composition_generation, uint64_t expected_revision);
char* writer_core_editor_session_composition_delete_grapheme_forward(uint64_t session_id, uint64_t composition_session_id, uint64_t composition_generation, uint64_t expected_revision);

// ── Chinese correction ──
// All methods return ResultEnvelope JSON; the returned string must be released with
// writer_core_free_string. Ranges and revisions are Core UTF-8 byte offsets/revisions.
char* writer_core_set_correction_model_path(const char* path);
char* writer_core_get_correction_resource_status(void);
char* writer_core_get_correction_model_path_status(const char* path);
char* writer_core_check_correction_segment(uint64_t session_id, const char* project_id, const char* chapter_id, uint64_t text_revision, uint32_t start_byte, uint32_t end_byte);
char* writer_core_check_correction_edits(uint64_t session_id, const char* project_id, const char* chapter_id, uint64_t text_revision, uint64_t base_revision, const char* text, const char* edits_json);
char* writer_core_apply_correction_edits(uint64_t session_id, const char* project_id, const char* chapter_id, uint64_t text_revision, uint64_t base_revision, const char* edits_json);
char* writer_core_scan_correction_pending(uint64_t session_id, const char* project_id, const char* chapter_id, uint64_t text_revision, const char* text);
char* writer_core_check_correction_chapter(uint64_t session_id, const char* project_id, const char* chapter_id, uint64_t text_revision);
char* writer_core_get_correction_chapter_snapshot(uint64_t session_id, const char* project_id, const char* chapter_id, uint64_t text_revision);
char* writer_core_ignore_correction(const char* project_id, const char* chapter_id, const char* rule_id, const char* original, const char* scope, uint64_t text_revision, uint32_t start_byte, uint32_t end_byte);
char* writer_core_add_correction_dictionary_entry(const char* project_id, const char* entry);
char* writer_core_apply_correction(uint64_t session_id, const char* project_id, const char* chapter_id, uint64_t text_revision, uint32_t start_byte, uint32_t end_byte, const char* original, const char* replacement);

#ifdef __cplusplus
}
#endif

#endif
