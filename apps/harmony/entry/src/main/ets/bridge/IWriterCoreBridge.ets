import {
  AppSummary,
  Project,
  Volume,
  ChapterMeta,
  ChapterContent,
  ChapterSaveReceipt,
  StarMapMeta,
  StarMapGraph,
  StarMapLayout,
  StarMapEdgeRender,
  StarMapMotionPolicy,
  LocalSettings,
  SyncableSettings,
  ThemePaletteRecordDto,
  BuiltinThemeDto,
  ResultEnvelope,
  ProjectTree,
  WritingStats,
  SyncConfig,
  SyncResult,
  SyncDiagnosticsResult,
  RecentEdit,
  WindowMetrics,
  LayoutPlan,
  ScreenPolicy,
  ScreenRole,
  ShellMode,
  EditorVisualTransaction,
  EditorTransactionCause
} from '../model/CoreDtos'

export interface IWriterCoreBridge {
  // App State 查询
  listAppSummaries(): Promise<ResultEnvelope<AppSummary[]>>
  getAppState(): Promise<ResultEnvelope<AppSummary>>

  // Project
  listProjects(): Promise<ResultEnvelope<Project[]>>
  getProjectTree(projectId: string): Promise<ResultEnvelope<ProjectTree>>
  createProject(name: string): Promise<ResultEnvelope<Project>>
  renameProject(projectId: string, newName: string): Promise<ResultEnvelope<boolean>>
  deleteProject(projectId: string): Promise<ResultEnvelope<boolean>>
  getProjectStats(projectId: string): Promise<ResultEnvelope<WritingStats>>

  // Volume
  listVolumes(projectId: string): Promise<ResultEnvelope<Volume[]>>
  createVolume(projectId: string, name: string): Promise<ResultEnvelope<Volume>>
  renameVolume(projectId: string, volumeId: string, newName: string): Promise<ResultEnvelope<boolean>>
  deleteVolume(projectId: string, volumeId: string): Promise<ResultEnvelope<boolean>>
  reorderVolumes(projectId: string, orderedVolumeIds: string[]): Promise<ResultEnvelope<boolean>>

  // Chapter
  listChapters(volumeId: string): Promise<ResultEnvelope<ChapterMeta[]>>
  loadChapter(chapterId: string): Promise<ResultEnvelope<ChapterContent>>
  saveChapter(chapterId: string, text: string): Promise<ResultEnvelope<ChapterSaveReceipt>>
  clearChapter(chapterId: string): Promise<ResultEnvelope<ChapterSaveReceipt>>
  createChapter(volumeId: string, name: string): Promise<ResultEnvelope<ChapterMeta>>
  renameChapter(chapterId: string, newName: string): Promise<ResultEnvelope<boolean>>
  deleteChapter(chapterId: string): Promise<ResultEnvelope<boolean>>
  reorderChapters(volumeId: string, orderedChapterIds: string[]): Promise<ResultEnvelope<boolean>>

  // StarMap
  listStarMaps(): Promise<ResultEnvelope<StarMapMeta[]>>
  listStarMapsForProject(projectId: string): Promise<ResultEnvelope<StarMapMeta[]>>
  getStarMap(starmapId: string): Promise<ResultEnvelope<StarMapMeta>>
  getStarMapGraph(starmapId: string): Promise<ResultEnvelope<StarMapGraph>>
  createStarMap(title: string, description: string): Promise<ResultEnvelope<StarMapMeta>>
  deleteStarMap(starmapId: string): Promise<ResultEnvelope<boolean>>
  renameStarMap(starmapId: string, newTitle: string): Promise<ResultEnvelope<boolean>>

  // StarMap Layout & Motion
  saveStarMapLayout(starmapId: string, layout: StarMapLayout): Promise<ResultEnvelope<boolean>>
  getStarMapLayout(starmapId: string): Promise<ResultEnvelope<StarMapLayout>>
  getStarMapMotionPolicy(): Promise<ResultEnvelope<StarMapMotionPolicy>>
  computeStarMapEdgeRenders(graph: StarMapGraph): Promise<ResultEnvelope<StarMapEdgeRender[]>>

  // Settings
  getLocalSettings(): Promise<ResultEnvelope<LocalSettings>>
  saveLocalSettings(settings: LocalSettings): Promise<ResultEnvelope<boolean>>
  getSyncableSettings(): Promise<ResultEnvelope<SyncableSettings>>
  saveSyncableSettings(settings: SyncableSettings): Promise<ResultEnvelope<boolean>>

  // Theme Palette
  listPaletteRecords(): Promise<ResultEnvelope<ThemePaletteRecordDto[]>>
  loadPaletteRecord(deviceId: string, fingerprint: string): Promise<ResultEnvelope<ThemePaletteRecordDto>>
  deletePaletteRecord(deviceId: string, fingerprint: string): Promise<ResultEnvelope<boolean>>
  listBuiltinThemes(): Promise<ResultEnvelope<BuiltinThemeDto[]>>

  // Sync（作品级，per-project: projectId selects the sync root）
  loadSyncConfig(projectId: string): Promise<ResultEnvelope<SyncConfig>>
  saveSyncConfig(projectId: string, config: SyncConfig): Promise<ResultEnvelope<boolean>>
  syncDryRun(projectId: string): Promise<ResultEnvelope<SyncResult>>
  performSync(projectId: string): Promise<ResultEnvelope<SyncResult>>
  syncDiagnostics(projectId: string): Promise<ResultEnvelope<SyncDiagnosticsResult>>

  // 应用级同步通道（Issue #600 评论 #3 问题四）
  // 应用级 Git 仓库根 = app_data_root，同步设置/全局星图/主题调色板。
  loadAppSyncConfig(): Promise<ResultEnvelope<SyncConfig>>
  saveAppSyncConfig(config: SyncConfig): Promise<ResultEnvelope<boolean>>
  performAppSync(): Promise<ResultEnvelope<SyncResult>>
  appSyncDiagnostics(): Promise<ResultEnvelope<SyncDiagnosticsResult>>

  // Stats
  getWritingStats(): Promise<ResultEnvelope<WritingStats>>
  calculateWordCount(text: string): Promise<number>
  processWritingEvent(deviceId: string, platform: string, projectId: string, volumeId: string,
    chapterId: string, oldText: string, newText: string, durationSeconds: number, sessionId: string): Promise<ResultEnvelope<boolean>>
  getRecentEdits(): Promise<ResultEnvelope<RecentEdit[]>>

  // Layout Policy
  resolveLayout(metrics: WindowMetrics): Promise<ResultEnvelope<LayoutPlan>>

  // Screen Policy
  resolveScreenPolicy(screenRole: ScreenRole, shellMode: ShellMode): Promise<ResultEnvelope<ScreenPolicy>>

  // Native Status
  getNativeLoadStatus(): Promise<ResultEnvelope<string>>
  isAiAvailable(): Promise<ResultEnvelope<boolean>>

  // Editor Animation
  editorVisualTransaction(
    oldText: string,
    newText: string,
    oldCursorIndex: number,
    newCursorIndex: number,
    cause: EditorTransactionCause,
    maxAnimatedChars: number,
    animationDurationMs: number
  ): Promise<ResultEnvelope<EditorVisualTransaction | null>>
}
