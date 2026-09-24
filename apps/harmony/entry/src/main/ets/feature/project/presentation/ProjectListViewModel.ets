// ProjectListViewModel — 作品列表页 ViewModel。
// 从 HomeScreen.ets 移出的所有 Core/NAPI 调用、async 加载、CRUD 操作。
// 页面只调 ViewModel 暴露的接口，不直接接触 bridge/native。

import hilog from '@ohos.hilog'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { ProjectSummary, WritingStats } from '../../../corebridge/dto/ProjectDtos'
import { RecentEdit } from '../../../corebridge/dto/ResultEnvelope'
import { getBridge, getAppDataRoot, getAppContext } from '../../../app/AppContext'
import { NativeWriterCoreBridge } from '../../../corebridge/NativeWriterCoreBridge'
import { HarmonyThemeAdapter } from '../../../ui/theme/HarmonyThemeAdapter'

const TAG = 'ProjectListViewModel'
const DOMAIN = 0x0000

export class ProjectListViewModel {
  private bridge: IWriterCoreBridge = getBridge()
  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()

  // 状态字段
  private projects: ProjectSummary[] = []
  private recentEdit: RecentEdit | null = null
  private writingStats: WritingStats | null = null
  private isLoading: boolean = true
  private errorMessage: string = ''
  private nativeStatus: string = ''
  private appDataRoot: string = ''

  // ── 公共查询接口 ──

  getProjects(): ProjectSummary[] {
    return this.projects
  }

  getRecentEdit(): RecentEdit | null {
    return this.recentEdit
  }

  getWritingStats(): WritingStats | null {
    return this.writingStats
  }

  isLoadingData(): boolean {
    return this.isLoading
  }

  getErrorMessage(): string {
    return this.errorMessage
  }

  getNativeStatus(): string {
    return this.nativeStatus
  }

  getAppDataRoot(): string {
    return this.appDataRoot
  }

  // ── 初始化+加载 ──

  async loadData(): Promise<void> {
    this.isLoading = true
    this.errorMessage = ''

    try {
      const ctx = getAppContext()
      if (ctx.isNativeEnvironment()) {
        const nativeBridge = NativeWriterCoreBridge.getInstance()
        const appDataRoot = getAppDataRoot()
        const initOk = await nativeBridge.initialize(appDataRoot)
        if (!initOk) {
          ctx.markNativeInitFailed()
          const errCode = nativeBridge.getLastInitErrorCode()
          const rustError = await nativeBridge.getLastError()
          let errDesc = `Unknown error(${errCode})`
          if (errCode === -1) { errDesc = 'Path is empty' }
          else if (errCode === -2) { errDesc = 'Invalid path encoding' }
          else if (errCode === -3) { errDesc = 'Internal state error' }
          else if (errCode === -4) { errDesc = 'Workspace dir creation failed' }
          else if (errCode === -999) { errDesc = 'NAPI exception (SO load failed?)' }
          this.errorMessage = `Native core init failed: ${errDesc}\nPath: ${appDataRoot}\nDetail: ${rustError}`
          this.isLoading = false
          return
        }
        const statusResult = await nativeBridge.getNativeLoadStatus()
        this.nativeStatus = statusResult.success ? (statusResult.data || 'unknown') : 'error'
      }

      await this.loadWorkspace()
      this.initializeTheme()
    } catch (err) {
      this.errorMessage = `Init error: ${err}`
      this.isLoading = false
    }
  }

  // ── 加载 workspace 数据 ──

  private async loadWorkspace(): Promise<void> {
    this.isLoading = true
    this.errorMessage = ''

    try {
      const appDataRoot = getAppDataRoot()
      this.appDataRoot = appDataRoot
      const appStateResult = await this.bridge.getAppState()
      const statsResult = await this.bridge.getWritingStats()

      if (appStateResult.success && appStateResult.data) {
        this.projects = appStateResult.data.projects
        this.recentEdit = appStateResult.data.recentEdit
      } else {
        this.errorMessage = 'Failed to load workspace data'
      }

      if (statsResult.success && statsResult.data) {
        this.writingStats = statsResult.data
      }
    } catch (err) {
      this.errorMessage = `Failed to load workspace: ${err}`
    } finally {
      this.isLoading = false
    }
  }

  // ── 初始化主题 ──

  private initializeTheme(): void {
    this.loadPaletteRecords()
    this.loadBuiltinThemes()
    this.loadLocalSettings()
  }

  private async loadPaletteRecords(): Promise<void> {
    try {
      const paletteResult = await this.bridge.listPaletteRecords()
      if (paletteResult.success && paletteResult.data) {
        this.themeAdapter.setPaletteRecords(paletteResult.data)
      }
    } catch (_e) {
      hilog.warn(DOMAIN, TAG, 'loadPaletteRecords failed')
    }
  }

  private async loadBuiltinThemes(): Promise<void> {
    try {
      const builtinResult = await this.bridge.listBuiltinThemes()
      if (builtinResult.success && builtinResult.data) {
        this.themeAdapter.setBuiltinThemes(builtinResult.data)
      }
    } catch (_e) {
      hilog.warn(DOMAIN, TAG, 'loadBuiltinThemes failed')
    }
  }

  private async loadLocalSettings(): Promise<void> {
    try {
      const settingsResult = await this.bridge.getLocalSettings()
      if (settingsResult.success && settingsResult.data) {
        const appearanceMode = settingsResult.data.appearanceMode || settingsResult.data.themeMode || 'system'
        this.themeAdapter.updateFromLocalSettings(
          appearanceMode,
          settingsResult.data.colorSource || 'built_in',
          settingsResult.data.selectedPaletteId || '',
          settingsResult.data.selectedBuiltinThemeId || ''
        )
      }
    } catch (_e) {
      hilog.warn(DOMAIN, TAG, 'loadLocalSettings failed')
    }
  }

  // ── 创建作品 ──

  async createProject(name: string): Promise<boolean> {
    try {
      const result = await this.bridge.createProject(name)
      if (result.success) {
        await this.loadWorkspace()
        return true
      }
      return false
    } catch (err) {
      hilog.error(DOMAIN, TAG, `createProject failed: ${err}`)
      return false
    }
  }

  // ── 删除作品 ──

  async deleteProject(projectId: string): Promise<boolean> {
    try {
      const result = await this.bridge.deleteProject(projectId)
      if (result.success) {
        await this.loadWorkspace()
        return true
      }
      return false
    } catch (err) {
      hilog.error(DOMAIN, TAG, `deleteProject failed: ${err}`)
      return false
    }
  }

  // ── 重命名作品 ──

  async renameProject(projectId: string, newName: string): Promise<boolean> {
    try {
      const result = await this.bridge.renameProject(projectId, newName)
      if (result.success) {
        await this.loadWorkspace()
        return true
      }
      return false
    } catch (err) {
      hilog.error(DOMAIN, TAG, `renameProject failed: ${err}`)
      return false
    }
  }

  // ── 重新加载 ──

  async retry(): Promise<void> {
    await this.loadData()
  }
}
