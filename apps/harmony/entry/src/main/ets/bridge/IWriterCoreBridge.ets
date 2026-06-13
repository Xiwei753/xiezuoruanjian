import {
  Workspace,
  WorkspaceState,
  Project,
  Volume,
  Chapter,
  ChapterData,
  SaveReceipt,
  StarMapMeta,
  StarMapGraph,
  LocalSettings,
  SyncableSettings,
  EffectiveSettings,
  ResultEnvelope,
  ProjectTree,
  WritingStats,
  SyncConfig,
  SyncReport,
  SyncDiagnostics,
  SyncResult,
  RecentEdit
} from '../model/CoreDtos'

export interface IWriterCoreBridge {
  // Workspace
  listWorkspaces(): Promise<ResultEnvelope<Workspace[]>>
  openWorkspace(path: string): Promise<ResultEnvelope<WorkspaceState>>
  validateWorkspace(path: string): Promise<ResultEnvelope<boolean>>
  getWorkspaceState(): Promise<ResultEnvelope<WorkspaceState>>

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
  listChapters(volumeId: string): Promise<ResultEnvelope<Chapter[]>>
  loadChapter(chapterId: string): Promise<ResultEnvelope<ChapterData>>
  saveChapter(chapterId: string, text: string): Promise<ResultEnvelope<SaveReceipt>>
  clearChapter(chapterId: string): Promise<ResultEnvelope<SaveReceipt>>
  createChapter(volumeId: string, name: string): Promise<ResultEnvelope<Chapter>>
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
  getEffectiveSettings(): Promise<ResultEnvelope<EffectiveSettings>>

  // Sync
  loadSyncConfig(): Promise<ResultEnvelope<SyncConfig>>
  saveSyncConfig(config: SyncConfig): Promise<ResultEnvelope<boolean>>
  syncDryRun(): Promise<ResultEnvelope<SyncReport>>
  syncDiagnostics(): Promise<ResultEnvelope<SyncDiagnostics>>
  performSync(): Promise<ResultEnvelope<SyncResult>>

  // Stats
  getWritingStats(): Promise<ResultEnvelope<WritingStats>>
  calculateWordCount(text: string): Promise<number>
  processWritingEvent(charsAdded: number, durationSeconds: number): Promise<ResultEnvelope<boolean>>
  getRecentEdits(): Promise<ResultEnvelope<RecentEdit[]>>

  // Native Status
  getNativeLoadStatus(): Promise<ResultEnvelope<string>>
  isWorkspaceValid(): Promise<ResultEnvelope<boolean>>
  isAiAvailable(): Promise<ResultEnvelope<boolean>>
}
