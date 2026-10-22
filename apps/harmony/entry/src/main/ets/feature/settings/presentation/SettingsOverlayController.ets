// SettingsOverlayController — 宽屏设置模态浮层的生命周期控制器。
//
// Issue #825 复核第2项：悬浮设置改成 ArkUI 窗口层级 custom dialog
// （getUIContext().getPromptAction().openCustomDialog），
// 系统返回 / 侧滑返回 / 遮罩 / 「背景仍是打开设置前的页面」都由对话框容器保证，
// 所以这里不再自己画常驻 Stack，也不需要在各个页面分别补 back 判断。
//
// Issue #972：浮层内容改为 WideSettingsOverlay（顶部返回/标题/搜索 + 两列分类卡片），
// 本控制器仍唯一拥有 customDialog 生命周期，不参与浮层内部的版式与业务状态。
//
// Issue #825 复核第3项：ComponentContent 的生命周期按官方要求收口——
// - 关闭时先 dispose 再清字段：官方文档要求弹窗关闭后调用 dispose()，
//   否则每开一次设置就丢掉一个没释放的 BuilderNode。
// - close() 不在 closeCustomDialog 成功前清字段，字段由 onDidDisappear 统一回收，
//   这样快速连点不会在旧弹窗还没消失时又开一层。
// - open() 失败（Promise reject）时也 dispose 刚创建的 content。
// - 不在这里判断 / 快照窗口宽度档位：是否浮层由 isOverlayLayout() 的唯一模式判定决定，
//   浮层内部布局也跟随窗口尺寸自行重排。
//
// 边界：
// - 只持有 UIContext 和对话框句柄，不放任何设置业务数据
//   （内容与状态在 SettingsContent / SettingsViewModel）。
// - close() 只关闭对话框，不触碰任何 NavPathStack，下面原页面和原路由栈保持原位置。
import hilog from '@ohos.hilog'
import { UIContext, ComponentContent } from '@kit.ArkUI'
import { WideLayoutMode, getWideLayoutState } from '../../../app/navigation/wide/WideLayoutMode'
import { createSettingsOverlayContent, SettingsOverlayArgs } from '../ui/wide/WideSettingsOverlay'
import { SearchOverlayController } from '../../search/presentation/SearchOverlayController'
import { SearchContext } from '../../../corebridge/dto/SearchDtos'

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

  /**
   * 已发起 closeCustomDialog 但对话框还没消失。
   * 这段时间内 open() 直接返回，避免在旧弹窗还没消失时叠第二层。
   */
  private closing: boolean = false

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
   * Issue #932 A：判定统一为外层导航壳写入的 WideLayoutMode。
   * 600–839vp 但仍使用底栏的窗口不再算浮层档，设置进入窄屏路由页；
   * isWorkbenchLayout 只服务页面内部内容，不再决定设置呈现方式。
   */
  isOverlayLayout(): boolean {
    return getWideLayoutState().current() === WideLayoutMode.Wide
  }

  /** 当前是否已经有设置浮层在（含正在关闭的那一段）。 */
  isOpen(): boolean {
    return this.dialogContent !== null || this.closing
  }

  /**
   * 打开宽屏设置浮层。重复调用幂等：已开着、或正在关闭，都直接返回。
   * 浮层内容固定为 WideSettingsOverlay；宽屏模式判定不在这里重复推导，
   * 调用方只在 isOverlayLayout() 为真时进入本方法。
   * Issue #982：initialSection / initialSettingKey 供全局搜索直接定位到具体设置项。
   */
  open(initialSection: string = '', initialSettingKey: string = ''): void {
    if (this.dialogContent !== null || this.closing) {
      return
    }
    if (!this.uiContext) {
      hilog.error(DOMAIN, TAG, 'UIContext is not attached; cannot open settings overlay')
      return
    }

    const args: SettingsOverlayArgs = {
      onRequestClose: () => { this.close() },
      onOpenGlobalSearch: (section: string) => {
        const context: SearchContext = { originScreen: 'settings' }
        if (section.length > 0) {
          context.currentSettingsSection = section
        }
        // 搜索浮层叠在设置浮层之上；关闭搜索后仍回到原来的设置浮层与分类位置。
        SearchOverlayController.getInstance().open(context, '', (): void => {
          // 搜索命中并导航到其他页面时，设置浮层也一起退出，避免遮住目标页面。
          this.close()
        })
      },
      initialSection: initialSection,
      initialSettingKey: initialSettingKey
    }
    const content = createSettingsOverlayContent(this.uiContext, args)
    this.dialogContent = content

    this.uiContext.getPromptAction().openCustomDialog(content, {
      alignment: DialogAlignment.Center,
      maskColor: '#80000000',
      // 点遮罩关闭；系统返回 / 侧滑返回同样由对话框容器消费，不会落到下面的 NavDestination。
      autoCancel: true,
      onDidDisappear: () => { this.releaseDialog(content) }
    }).catch((e: Error) => {
      // open 失败时对话框不会出现，onDidDisappear 不会回调，这里负责回收 content。
      this.releaseDialog(content)
      hilog.error(DOMAIN, TAG, 'openCustomDialog failed: %{public}s', String(e))
    })
  }

  /** 关闭设置浮层。只 dismiss 对话框，不动下面的页面和路由栈。 */
  close(): void {
    if (this.dialogContent === null || this.closing || !this.uiContext) {
      return
    }
    const content = this.dialogContent
    this.closing = true
    this.uiContext.getPromptAction().closeCustomDialog(content).catch((e: Error) => {
      // 关闭请求失败时对话框可能仍在，这里不 dispose 也不清字段，
      // 交给用户下一次交互（点遮罩 / 系统返回）走 autoCancel 路径。
      this.closing = false
      hilog.error(DOMAIN, TAG, 'closeCustomDialog failed: %{public}s', String(e))
    })
  }

  /**
   * 窗口级清理：壳层销毁时如果还持有当前窗口的浮层，先关掉再摘掉 UIContext。
   * 否则单例会攥着一个已经失效的 UIContext，下个窗口 open() 直接失败。
   */
  detachUIContext(uiContext: UIContext): void {
    if (this.uiContext !== uiContext) {
      return
    }
    this.close()
    this.uiContext = null
  }

  /**
   * 统一回收一处 ComponentContent：dispose 之后清字段。
   * 按 content 身份判断，重复回调（onDidDisappear + open 失败兜底）不会二次 dispose。
   */
  private releaseDialog(content: ComponentContent<SettingsOverlayArgs>): void {
    if (this.dialogContent !== content) {
      return
    }
    this.dialogContent = null
    this.closing = false
    // 官方要求弹窗关闭后 dispose，否则每次打开都会漏一个 BuilderNode。
    content.dispose()
  }
}