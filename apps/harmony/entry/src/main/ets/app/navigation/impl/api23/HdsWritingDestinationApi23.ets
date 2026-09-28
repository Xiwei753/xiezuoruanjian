// HdsWritingDestinationApi23 — API23+ 写作页 NavDestination 壳层。
// Issue #768 评论5835958606：把 HdsNavDestination + HdsNavigationTitleBarOptions
// 收进 impl/api23 组件，共享页面 WritingScreen 不再直接 import @kit.UIDesignKit，
// 避免高版本模块引用进入所有版本都会加载的共享页面，破坏版本隔离。
//
// Issue #783 评论5856245403：分离"标题栏返回"和"系统返回手势"。
// - onTitleBack：标题栏 backIcon 点击，始终走 requestLeave（退出页面）。
// - onSystemBack：系统返回手势（全面屏侧滑返回），IME 打开时先收键盘，再次手势才退出页面。
// - HDS 标题栏使用原生 backIcon.action 绑定返回按钮，不再使用不存在的 customizedArea。
// - .onBackPressed 只调用 onSystemBack 并消费本次系统返回（return true）。
//
// Issue #784 评论5857397503：移除硬编码的三个 menu item（同步/搜索/设置），
// 改为通过 ScreenPolicyBridge 异步加载 ActionSlot[]，用 buildHdsMenuItems 生成菜单。

import { HdsNavDestination, HdsNavigationTitleBarOptions, hdsMaterial } from '@kit.UIDesignKit'
import { buildHdsMenuItems } from './HdsHeaderActionMenu'
import { ScreenPolicyBridge } from '../../../../ui/adaptive/ScreenPolicyBridge'
import { ScreenRole, ActionSlot } from '../../../../corebridge/dto/PlatformDtos'

@ComponentV2
export struct HdsWritingDestinationApi23 {
  @BuilderParam contentBuilder: () => void
  @Param title: string | Resource = ''
  // Issue #783 评论5855754605：分离两个返回意图。
  @Param onTitleBack: (() => void) | null = null
  @Param onSystemBack: (() => void) | null = null
  @Param onShown: (() => void) | null = null
  // Issue #778 任务8：接收父级传入的 scroller，绑定到 HdsNavDestination 做沉浸联动。
  @Param scroller: Scroller = new Scroller()
  // Issue #784 评论5857397503：接收 ScreenRole，异步加载 ScreenPolicy 获取 ActionSlot[]。
  @Param screenRole: ScreenRole = ScreenRole.Writing
  @Local headerActions: ActionSlot[] = []

  async aboutToAppear() {
    const policy = await ScreenPolicyBridge.getInstance().resolveScreenPolicy(this.screenRole)
    if (policy !== null) {
      this.headerActions = policy.actionSlots
    }
  }

  build() {
    HdsNavDestination() {
      this.contentBuilder()
    }
    .hideTitleBar(false)
    .titleBar({
      enableComponentSafeArea: true,
      // 窗口开了 setWindowLayoutFullScreen 之后，标题栏必须显式继续避让顶部状态栏，
      // 否则「素笺」这类标题会被压到 18px 高（实测 [200,157][325,230] → [200,106][325,124]）。
      // 本文件只在 API >= 23 渲染（见各 Screen 的 useHdsNav 判定），该字段 API 20+ 恒可用。
      avoidLayoutSafeArea: true,
      content: {
        title: { mainTitle: this.title },
        // Issue #783 评论5856245403：使用 HDS 标题栏原生 backIcon，
        // 点击只调用 onTitleBack（退出页面），不和系统返回手势共用回调。
        backIcon: {
          label: '返回',
          icon: $r('sys.symbol.chevron_left'),
          isEnabled: true,
          action: () => {
            if (this.onTitleBack) {
              this.onTitleBack()
            }
          }
        },
        // Issue #784 评论5857397503：菜单项由 buildHdsMenuItems 从 ActionSlot[] 动态生成，
        // 不再硬编码"同步/搜索/设置"三个 item。
        menu: {
          value: buildHdsMenuItems(this.headerActions)
        }
      },
      style: {
        systemMaterialEffect: {
          materialType: hdsMaterial.MaterialType.IMMERSIVE,
          materialLevel: hdsMaterial.MaterialLevel.ADAPTIVE
        }
      }
    } as HdsNavigationTitleBarOptions)
    .bindToScrollable([this.scroller])
    .onBackPressed(() => {
      // Issue #783 评论5855754605：.onBackPressed 只处理系统返回手势（全面屏侧滑返回），
      // 不再让默认 HDS 返回按钮和系统手势共用一个回调。
      if (this.onSystemBack) {
        this.onSystemBack()
      }
      return true
    })
    .onShown(() => {
      if (this.onShown) {
        this.onShown()
      }
    })
  }
}
