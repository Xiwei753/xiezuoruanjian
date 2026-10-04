// SettingsOverlayController — 宽屏设置悬浮层的可见性控制器。
// Issue #825：宽屏（WorkspaceLayoutMode.Workbench）下设置不再压一条路由，
// 而是悬浮在打开设置前的页面之上，所以这里只管 open/close 和可见状态。
// 边界：
// - 只持有 visible 状态，不放任何设置业务数据（设置内容在 SettingsViewModel）。
// - close() 只隐藏悬浮层，不触碰任何 NavPathStack，因此下面原页面和原路由栈保持原位置。
// - 不 import 任何 @kit.*，让 app/navigation 层可以直接引用。
import { AdaptiveContext } from '../../../ui/adaptive/AdaptiveContext'
import { WorkspaceLayoutMode } from '../../../corebridge/dto/PlatformDtos'

@ObservedV2
export class SettingsOverlayController {
  private static instance: SettingsOverlayController | null = null

  // 宽屏悬浮层是否可见。@ObservedV2 + @Trace 让 SettingsOverlayHost 组件自动响应。
  @Trace visible: boolean = false

  static getInstance(): SettingsOverlayController {
    if (!SettingsOverlayController.instance) {
      SettingsOverlayController.instance = new SettingsOverlayController()
    }
    return SettingsOverlayController.instance
  }

  // 当前窗口是否用悬浮设置。窄屏仍然走 Routes.Settings 路由页。
  isOverlayLayout(): boolean {
    const state = AdaptiveContext.getInstance().getCurrentState()
    return state.layoutPlan.workspaceLayoutMode === WorkspaceLayoutMode.Workbench
  }

  open(): void {
    this.visible = true
  }

  close(): void {
    this.visible = false
  }

  toggle(): void {
    this.visible = !this.visible
  }
}