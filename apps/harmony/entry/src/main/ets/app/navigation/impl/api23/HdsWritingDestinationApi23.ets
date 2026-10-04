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
// 改为通过 ScreenPolicyBridge 异步加载 ActionSlot[]，交给 HeaderTitleBarStack
// 自绘标题 + 页头动作胶囊。

import { HdsNavDestination, HdsNavigationTitleBarOptions, hdsMaterial } from '@kit.UIDesignKit'
import { HeaderTitleBarStack } from './HeaderTitleBarStack'
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
  // Issue #825：宽屏时动作搬进页面自己的右侧工具区，页头动作胶囊关掉，
  // 保证同步/搜索/设置只有一个入口。
  @Param showActionCapsule: boolean = true
  @Local headerActions: ActionSlot[] = []

  async aboutToAppear() {
    const policy = await ScreenPolicyBridge.getInstance().resolveScreenPolicy(this.screenRole)
    if (policy !== null) {
      this.headerActions = policy.actionSlots
    }
  }

  // stackBuilder 是 CustomBuilder，组件调用必须写在 @Builder 里。
  // 直接在箭头函数体里写 HeaderTitleBarStack({...}) 会被当普通函数调用，运行时崩：
  // TypeError: class constructor cannot called without 'new'。
  @Builder
  headerTitleBar() {
    HeaderTitleBarStack({
      title: this.title,
      headerActions: this.headerActions,
      showBackButton: true,
      showActionCapsule: this.showActionCapsule
    })
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
        // 标题与页头动作胶囊改由 stackBuilder 自绘，与根页面壳和工作区壳同一观感。
        // 原因见 HeaderTitleBarStack 顶部注释：HDS 的 menu.value 每项都是独立圆面，
        // 渲染不出「一个胶囊包住三个动作」。
        title: { mainTitle: '' },
        // Issue #783 评论5856245403：使用 HDS 标题栏原生 backIcon，
        // 点击只调用 onTitleBack（退出页面），不和系统返回手势共用回调。
        // label 置空：HDS 会把文案画进返回按钮的圆形里，标题栏里已有自绘标题。
        backIcon: {
          label: '',
          icon: $r('sys.symbol.chevron_left'),
          isEnabled: true,
          action: () => {
            if (this.onTitleBack) {
              this.onTitleBack()
            }
          }
        },
        stackBuilder: (): void => {
          this.headerTitleBar()
        }
      },
      style: {
        // ADAPTIVE 是官方《沉浸光感》示例里的值，标题栏的按压光要靠它。
        systemMaterialEffect: {
          materialType: hdsMaterial.MaterialType.ADAPTIVE,
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
