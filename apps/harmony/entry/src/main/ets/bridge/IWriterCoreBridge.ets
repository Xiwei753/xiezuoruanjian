import {
  WorkspaceSummary,
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
  // Workspace 查询
  listWorkspaces(): Promise<ResultEnvelope<WorkspaceSummary[]>>
  getWorkspaceState(): Promise<ResultEnvelope<WorkspaceSummary>>

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

  // Sync
  // loadSyncConfig/saveSyncConfig/syncDiagnostics are global (not per-project).
  // syncDryRun/performSync are per-project: projectId selects the sync root.
  loadSyncConfig(): Promise<ResultEnvelope<SyncConfig>>
  saveSyncConfig(config: SyncConfig): Promise<ResultEnvelope<boolean>>
  syncDryRun(projectId: string): Promise<ResultEnvelope<SyncResult>>
  performSync(projectId: string): Promise<ResultEnvelope<SyncResult>>
  syncDiagnostics(): Promise<ResultEnvelope<SyncDiagnosticsResult>>

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
