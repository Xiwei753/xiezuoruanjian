// SettingsOverlayController — 宽屏设置模态浮层的生命周期控制器。
//
// Issue #825 复核第2项：悬浮设置改成 ArkUI 窗口层级 custom dialog
// （getUIContext().getPromptAction().openCustomDialog），
// 系统返回 / 侧滑返回 / 遮罩 / 「背景仍是打开设置前的页面」都由对话框容器保证，
// 所以这里不再自己画常驻 Stack，也不需要在各个页面分别补 back 判断。
//
// 边界：
// - 只持有 UIContext 和对话框句柄，不放任何设置业务数据
//   （内容与状态在 SettingsContent / SettingsViewModel）。
// - close() 只关闭对话框，不触碰任何 NavPathStack，下面原页面和原路由栈保持原位置。
// - open() 幂等：已经有对话框在就直接返回，不会叠多层。
import hilog from '@ohos.hilog'
import { UIContext, ComponentContent } from '@kit.ArkUI'
import { AdaptiveContext } from '../../../ui/adaptive/AdaptiveContext'
import { PrimaryNavigationPlacement, WorkspaceLayoutMode } from '../../../corebridge/dto/PlatformDtos'
import { createSettingsOverlayContent, SettingsOverlayArgs } from '../ui/SettingsOverlayPanel'

const TAG = 'SettingsOverlayController'
const DOMAIN = 0x0000

export class SettingsOverlayController {
  private static instance: SettingsOverlayController | null = null

  /**
   * 窗口级 UIContext。
   * openCustomDialog 必须在 UIContext 上调用，而页头动作可能来自任何 NavDestination，
   * 所以由壳层 PrimaryTabShell 在 aboutToAppear 注册一次，不在每个页面各传一遍。
   */
  private uiContext: UIContext | null = null

  /** 当前打开的对话框句柄；null 表示当前没有设置浮层。 */
  private dialogContent: ComponentContent<SettingsOverlayArgs> | null = null

  static getInstance(): SettingsOverlayController {
    if (!SettingsOverlayController.instance) {
      SettingsOverlayController.instance = new SettingsOverlayController()
    }
    return SettingsOverlayController.instance
  }

  /** 壳层注册窗口级 UIContext。 */
  attachUIContext(uiContext: UIContext): void {
    this.uiContext = uiContext
  }

  /**
   * 当前窗口是否用悬浮设置。窄屏仍然走 Routes.Settings 路由页。
   * Issue #825 复核第4项：按 Core 的 workspace_layout_mode 判 Workbench，
   * 600–839vp 这一档 Core 已经给 Workbench（但一级导航仍是 Bottom），
   * 那一档窗口里设置同样走悬浮层。
   */
  isOverlayLayout(): boolean {
    const state = AdaptiveContext.getInstance().getCurrentState()
    return state.layoutPlan.workspaceLayoutMode === WorkspaceLayoutMode.Workbench
  }

  /**
   * 打开宽屏设置浮层。重复调用幂等。
   * 是否两列由 Core 的 primaryNavigationPlacement 决定：
   * 只有一级导航在 Side（>=840vp）才排两列，Bottom 那档用单列。
   */
  open(): void {
    if (this.dialogContent !== null) {
      return
    }
    if (!this.uiContext) {
      hilog.error(DOMAIN, TAG, 'UIContext is not attached; cannot open settings overlay')
      return
    }

    const layoutPlan = AdaptiveContext.getInstance().getCurrentState().layoutPlan
    const args: SettingsOverlayArgs = {
      onRequestClose: () => { this.close() },
      twoColumn: layoutPlan.primaryNavigationPlacement === PrimaryNavigationPlacement.Side
    }
    const content = createSettingsOverlayContent(this.uiContext, args)
    this.dialogContent = content

    this.uiContext.getPromptAction().openCustomDialog(content, {
      alignment: DialogAlignment.Center,
      maskColor: '#80000000',
      // 点遮罩关闭；系统返回 / 侧滑返回同样由对话框容器消费，不会落到下面的 NavDestination。
      autoCancel: true,
      onDidDisappear: () => {
        this.dialogContent = null
      }
    }).catch((e: Error) => {
      this.dialogContent = null
      hilog.error(DOMAIN, TAG, 'openCustomDialog failed: %{public}s', String(e))
    })
  }

  /** 关闭设置浮层。只 dismiss 对话框，不动下面的页面和路由栈。 */
  close(): void {
    if (this.dialogContent === null || !this.uiContext) {
      return
    }
    const content = this.dialogContent
    this.dialogContent = null
    this.uiContext.getPromptAction().closeCustomDialog(content).catch((e: Error) => {
      hilog.error(DOMAIN, TAG, 'closeCustomDialog failed: %{public}s', String(e))
    })
  }
}
