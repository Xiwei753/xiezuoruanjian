import { IWriterCoreBridge } from './IWriterCoreBridge'
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

import nativeModule from 'libwriter_core.so'

export class NativeWriterCoreBridge implements IWriterCoreBridge {
  private static instance: NativeWriterCoreBridge
  private isInitialized: boolean = false

  static getInstance(): NativeWriterCoreBridge {
    if (!NativeWriterCoreBridge.instance) {
      NativeWriterCoreBridge.instance = new NativeWriterCoreBridge()
    }
    return NativeWriterCoreBridge.instance
  }

  private notImplemented<T>(): ResultEnvelope<T> {
    return {
      success: false,
      errorCode: 'NOT_IMPLEMENTED',
      userMessage: 'Native bridge not implemented yet'
    }
  }

  async initialize(workspacePath: string): Promise<boolean> {
    if (this.isInitialized) {
      return true
    }
    try {
      const result = nativeModule.nativeInit(workspacePath)
      if (result === 0) {
        this.isInitialized = true
        return true
      }
      console.error(`nativeInit failed with code: ${result}`)
      return false
    } catch (e) {
      console.error(`nativeInit exception: ${e}`)
      return false
    }
  }

  // Workspace
  async listWorkspaces(): Promise<ResultEnvelope<Workspace[]>> {
    return this.notImplemented()
  }

  async openWorkspace(path: string): Promise<ResultEnvelope<WorkspaceState>> {
    return this.notImplemented()
  }

  async validateWorkspace(path: string): Promise<ResultEnvelope<boolean>> {
    return this.notImplemented()
  }

  async getWorkspaceState(): Promise<ResultEnvelope<WorkspaceState>> {
    return this.notImplemented()
  }

  // Project
  async listProjects(): Promise<ResultEnvelope<Project[]>> {
    return this.notImplemented()
  }

  async getProjectTree(projectId: string): Promise<ResultEnvelope<ProjectTree>> {
    return this.notImplemented()
  }

  async createProject(name: string): Promise<ResultEnvelope<Project>> {
    return this.notImplemented()
  }

  async renameProject(projectId: string, newName: string): Promise<ResultEnvelope<boolean>> {
    return this.notImplemented()
  }

  async deleteProject(projectId: string): Promise<ResultEnvelope<boolean>> {
    return this.notImplemented()
  }

  async getProjectStats(projectId: string): Promise<ResultEnvelope<WritingStats>> {
    return this.notImplemented()
  }

  // Volume
  async listVolumes(projectId: string): Promise<ResultEnvelope<Volume[]>> {
    return this.notImplemented()
  }

  async createVolume(projectId: string, name: string): Promise<ResultEnvelope<Volume>> {
    return this.notImplemented()
  }

  async renameVolume(projectId: string, volumeId: string, newName: string): Promise<ResultEnvelope<boolean>> {
    return this.notImplemented()
  }

  async deleteVolume(projectId: string, volumeId: string): Promise<ResultEnvelope<boolean>> {
    return this.notImplemented()
  }

  async reorderVolumes(projectId: string, orderedVolumeIds: string[]): Promise<ResultEnvelope<boolean>> {
    return this.notImplemented()
  }

  // Chapter
  async listChapters(volumeId: string): Promise<ResultEnvelope<Chapter[]>> {
    return this.notImplemented()
  }

  async loadChapter(chapterId: string): Promise<ResultEnvelope<ChapterData>> {
    return this.notImplemented()
  }

  async saveChapter(chapterId: string, text: string): Promise<ResultEnvelope<SaveReceipt>> {
    return this.notImplemented()
  }

  async clearChapter(chapterId: string): Promise<ResultEnvelope<SaveReceipt>> {
    return this.notImplemented()
  }

  async createChapter(volumeId: string, name: string): Promise<ResultEnvelope<Chapter>> {
    return this.notImplemented()
  }

  async renameChapter(chapterId: string, newName: string): Promise<ResultEnvelope<boolean>> {
    return this.notImplemented()
  }

  async deleteChapter(chapterId: string): Promise<ResultEnvelope<boolean>> {
    return this.notImplemented()
  }

  async reorderChapters(volumeId: string, orderedChapterIds: string[]): Promise<ResultEnvelope<boolean>> {
    return this.notImplemented()
  }

  // StarMap
  async listStarMaps(): Promise<ResultEnvelope<StarMapMeta[]>> {
    return this.notImplemented()
  }

  async listStarMapsForProject(projectId: string): Promise<ResultEnvelope<StarMapMeta[]>> {
    return this.notImplemented()
  }

  async getStarMap(starmapId: string): Promise<ResultEnvelope<StarMapMeta>> {
    return this.notImplemented()
  }

  async getStarMapGraph(starmapId: string): Promise<ResultEnvelope<StarMapGraph>> {
    return this.notImplemented()
  }

  async createStarMap(title: string, description: string): Promise<ResultEnvelope<StarMapMeta>> {
    return this.notImplemented()
  }

  async deleteStarMap(starmapId: string): Promise<ResultEnvelope<boolean>> {
    return this.notImplemented()
  }

  async renameStarMap(starmapId: string, newTitle: string): Promise<ResultEnvelope<boolean>> {
    return this.notImplemented()
  }

  // Settings
  async getLocalSettings(): Promise<ResultEnvelope<LocalSettings>> {
    return this.notImplemented()
  }

  async saveLocalSettings(settings: LocalSettings): Promise<ResultEnvelope<boolean>> {
    return this.notImplemented()
  }

  async getSyncableSettings(): Promise<ResultEnvelope<SyncableSettings>> {
    return this.notImplemented()
  }

  async saveSyncableSettings(settings: SyncableSettings): Promise<ResultEnvelope<boolean>> {
    return this.notImplemented()
  }

  async getEffectiveSettings(): Promise<ResultEnvelope<EffectiveSettings>> {
    return this.notImplemented()
  }

  // Sync
  async loadSyncConfig(): Promise<ResultEnvelope<SyncConfig>> {
    return this.notImplemented()
  }

  async saveSyncConfig(config: SyncConfig): Promise<ResultEnvelope<boolean>> {
    return this.notImplemented()
  }

  async syncDryRun(): Promise<ResultEnvelope<SyncReport>> {
    return this.notImplemented()
  }

  async syncDiagnostics(): Promise<ResultEnvelope<SyncDiagnostics>> {
    return this.notImplemented()
  }

  async performSync(): Promise<ResultEnvelope<SyncResult>> {
    return this.notImplemented()
  }

  // Stats
  async getWritingStats(): Promise<ResultEnvelope<WritingStats>> {
    return this.notImplemented()
  }

  async calculateWordCount(text: string): Promise<number> {
    if (!this.isInitialized) {
      return -1
    }
    try {
      return nativeModule.nativeCalculateWordCount(text)
    } catch (e) {
      return -1
    }
  }

  async processWritingEvent(charsAdded: number, durationSeconds: number): Promise<ResultEnvelope<boolean>> {
    return this.notImplemented()
  }

  async getRecentEdits(): Promise<ResultEnvelope<RecentEdit[]>> {
    return this.notImplemented()
  }

  // Native Status
  async getNativeLoadStatus(): Promise<ResultEnvelope<string>> {
    try {
      const status = nativeModule.nativeGetLoadStatus()
      return { success: true, data: status }
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async isWorkspaceValid(): Promise<ResultEnvelope<boolean>> {
    return this.notImplemented()
  }

  async isAiAvailable(): Promise<ResultEnvelope<boolean>> {
    return {
      success: true,
      data: false
    }
  }
}
