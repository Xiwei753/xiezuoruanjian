// ProjectListViewModel — 作品列表页 ViewModel。
// 从 HomeScreen.ets 移出的所有 Core/NAPI 调用、async 加载、CRUD 操作。
// 页面只调 ViewModel 暴露的接口，不直接接触 bridge/native。
// Issue #760 评论 5821290182 问题7：改为 @ObservedV2 + @Trace，页面直接观察状态。
// Issue #760 评论 5821290182 问题5：加载 ScreenPolicy，暴露排序后的 Context actions。

import hilog from '@ohos.hilog'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { ProjectSummary, WritingStats } from '../../../corebridge/dto/ProjectDtos'
import { RecentEdit } from '../../../corebridge/dto/ResultEnvelope'
import { getBridge, getAppDataRoot, getAppContext } from '../../../app/AppContext'
import { NativeWriterCoreBridge } from '../../../corebridge/NativeWriterCoreBridge'
import { HarmonyThemeAdapter } from '../../../ui/theme/HarmonyThemeAdapter'
import { ScreenPolicy, ScreenRole, ActionSlot, ActionRegion, ActionTarget } from '../../../corebridge/dto/PlatformDtos'

const TAG = 'ProjectListViewModel'
const DOMAIN = 0x0000

@ObservedV2
export class ProjectListViewModel {
  // ── 可观察状态字段（@Trace，UI 直接观察） ──
  @Trace projects: ProjectSummary[] = []
  @Trace isLoading: boolean = true
  @Trace errorMessage: string = ''
  @Trace showCreateDialog: boolean = false
  @Trace newProjectName: string = ''
  @Trace renameTarget: ProjectSummary | null = null
  @Trace renameInput: string = ''
  @Trace deleteTarget: ProjectSummary | null = null
  @Trace isSyncing: boolean = false
  @Trace contextActions: ActionSlot[] = []
  @Trace headerTrailingActions: ActionSlot[] = []
  @Trace primaryActions: ActionSlot[] = []
  @Trace selectedProjectId: string = ''
  // 宽屏布局所需字段（Issue #760 评论 5822350374 问题3）
  @Trace writingStats: WritingStats | null = null
  @Trace recentEdit: RecentEdit | null = null
  @Trace nativeStatus: string = ''
  @Trace appDataRoot: string = ''
  @Trace currentMenuProjectId: string = ''

  // ── 非观察私有字段 ──
  private bridge: IWriterCoreBridge = getBridge()
  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private screenPolicy: ScreenPolicy | null = null

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
      await this.loadScreenPolicy()
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

  // ── 加载 ScreenPolicy（问题5） ──

  private async loadScreenPolicy(): Promise<void> {
    try {
      const result = await this.bridge.resolveScreenPolicy(ScreenRole.ProjectList)
      if (result.success && result.data) {
        this.screenPolicy = result.data
        this.contextActions = result.data.actionSlots
          .filter((slot: ActionSlot) => slot.region === ActionRegion.Context)
          .sort((a: ActionSlot, b: ActionSlot) => a.order - b.order)
        this.headerTrailingActions = result.data.actionSlots
          .filter((slot: ActionSlot) => slot.region === ActionRegion.HeaderTrailing)
          .sort((a: ActionSlot, b: ActionSlot) => a.order - b.order)
        this.primaryActions = result.data.actionSlots
          .filter((slot: ActionSlot) => slot.region === ActionRegion.PrimaryAction)
          .sort((a: ActionSlot, b: ActionSlot) => a.order - b.order)
      }
    } catch (_e) {
      hilog.warn(DOMAIN, TAG, 'loadScreenPolicy failed')
    }
  }

  // ── 同步（问题6：执行真正的同步动作） ──

  async performSync(): Promise<void> {
    this.isSyncing = true
    try {
      await this.bridge.performSync()
    } catch (_e) {
      hilog.error(DOMAIN, TAG, 'performSync failed')
    } finally {
      this.isSyncing = false
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

  // ── CRUD 操作 ──

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

  // ── 事件方法（页面派发意图，ViewModel 内部管理 async 和状态） ──

  onCreateProject(): void {
    if (this.newProjectName.trim().length === 0) {
      return
    }
    this.createProject(this.newProjectName.trim())
    this.showCreateDialog = false
    this.newProjectName = ''
  }

  onDeleteProject(): void {
    if (this.deleteTarget) {
      this.deleteProject(this.deleteTarget.id)
    }
    this.deleteTarget = null
  }

  onRenameProject(): void {
    if (this.renameTarget && this.renameInput.trim().length > 0) {
      this.renameProject(this.renameTarget.id, this.renameInput.trim())
    }
    this.renameTarget = null
    this.renameInput = ''
  }

  // ── 重新加载 ──

  async retry(): Promise<void> {
    await this.loadData()
  }

  // ─── ScreenPolicy / Context Actions ───

  /** 获取指定目标的排序后 Context actions（供 UI 按 action slot 渲染菜单） */
  getContextActions(target: ActionTarget): ActionSlot[] {
    if (!this.screenPolicy) {
      return []
    }
    const actions: ActionSlot[] = []
    for (const slot of this.screenPolicy.actionSlots) {
      if (slot.region === ActionRegion.Context && slot.target === target) {
        actions.push(slot)
      }
    }
    actions.sort((a: ActionSlot, b: ActionSlot) => a.order - b.order)
    return actions
  }

  /** 查找指定目标和角色的 ActionSlot（用于获取 requiresConfirmation 等属性） */
  findContextAction(target: ActionTarget, role: string): ActionSlot | null {
    const actions = this.getContextActions(target)
    for (const slot of actions) {
      if (slot.role === role) {
        return slot
      }
    }
    return null
  }

  // ─── 项目 Context Action 处理 ───

  /** 开始重命名项目 */
  startRenameProject(projectId: string): void {
    const project = this.projects.find((p: ProjectSummary) => p.id === projectId)
    if (project) {
      this.renameTarget = project
      this.renameInput = project.title
    }
  }

  /** 开始删除项目（按 requiresConfirmation 决定是否需要确认对话框） */
  startDeleteProject(projectId: string, requiresConfirmation: boolean): void {
    const project = this.projects.find((p: ProjectSummary) => p.id === projectId)
    if (project) {
      if (requiresConfirmation) {
        this.deleteTarget = project
      } else {
        // 不需要确认，直接删除
        this.deleteProject(projectId)
      }
    }
  }

  /** 取消重命名 */
  cancelRename(): void {
    this.renameTarget = null
    this.renameInput = ''
  }

  /** 确认重命名 */
  confirmRename(): void {
    this.onRenameProject()
  }

  /** 取消删除 */
  cancelDelete(): void {
    this.deleteTarget = null
  }

  /** 确认删除 */
  confirmDelete(): void {
    this.onDeleteProject()
  }
}
