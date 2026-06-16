import {
  WorkspaceSummary,
  Project,
  Volume,
  ChapterMeta,
  ChapterContent,
  ChapterSaveReceipt,
  StarMapMeta,
  StarMapGraph,
  LocalSettings,
  SyncableSettings,
  ResultEnvelope,
  ProjectTree,
  WritingStats,
  SyncConfig,
  SyncResult,
  RecentEdit,
  WindowMetrics,
  LayoutPlan
} from '../model/CoreDtos'

export interface IWriterCoreBridge {
  // Workspace
  listWorkspaces(): Promise<ResultEnvelope<WorkspaceSummary[]>>
  openWorkspace(path: string): Promise<ResultEnvelope<WorkspaceSummary>>
  validateWorkspace(path: string): Promise<ResultEnvelope<boolean>>
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

  // Settings
  getLocalSettings(): Promise<ResultEnvelope<LocalSettings>>
  saveLocalSettings(settings: LocalSettings): Promise<ResultEnvelope<boolean>>
  getSyncableSettings(): Promise<ResultEnvelope<SyncableSettings>>
  saveSyncableSettings(settings: SyncableSettings): Promise<ResultEnvelope<boolean>>

  // Sync
  loadSyncConfig(): Promise<ResultEnvelope<SyncConfig>>
  saveSyncConfig(config: SyncConfig): Promise<ResultEnvelope<boolean>>
  syncDryRun(): Promise<ResultEnvelope<SyncResult>>
  performSync(): Promise<ResultEnvelope<SyncResult>>

  // Stats
  getWritingStats(): Promise<ResultEnvelope<WritingStats>>
  calculateWordCount(text: string): Promise<number>
  processWritingEvent(charsAdded: number, durationSeconds: number): Promise<ResultEnvelope<boolean>>
  getRecentEdits(): Promise<ResultEnvelope<RecentEdit[]>>

  // Layout Policy
  resolveLayout(metrics: WindowMetrics): Promise<ResultEnvelope<LayoutPlan>>

  // Native Status
  getNativeLoadStatus(): Promise<ResultEnvelope<string>>
  isWorkspaceValid(): Promise<ResultEnvelope<boolean>>
  isAiAvailable(): Promise<ResultEnvelope<boolean>>
}
