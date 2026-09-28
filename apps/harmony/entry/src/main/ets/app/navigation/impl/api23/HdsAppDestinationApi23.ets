// HdsAppDestinationApi23 — API23+ Home/StarMap/Stats 共享根页面壳。
// Issue #784 评论 5857397503 第 1 点：Home 删除旧顶栏后没有新标题栏接上，
// 新建此共享根页面壳，让 Home / StarMap / Stats 共用 HdsNavDestination + 标题栏。
// 通过 ScreenPolicyBridge 异步加载 ActionSlot[]，用 buildHdsMenuItems 生成菜单，
// 不再在平台端硬编码"同步/搜索/设置"三个 item。
// onShown 时通过 NavigationChromeStore.reportScreenRole 上报当前 ScreenRole + tabIndex。

import { HdsNavDestination, HdsNavigationTitleBarOptions, hdsMaterial } from '@kit.UIDesignKit'
import { buildHdsMenuItems } from './HdsHeaderActionMenu'
import { ScreenPolicyBridge } from '../../../../ui/adaptive/ScreenPolicyBridge'
import { NavigationChromeStore } from '../../NavigationChromeStore'
import { ScreenRole, ActionSlot } from '../../../../corebridge/dto/PlatformDtos'

@ComponentV2
export struct HdsAppDestinationApi23 {
  @BuilderParam contentBuilder: () => void
  @Param title: string | Resource = ''
  @Param screenRole: ScreenRole = ScreenRole.ProjectList
  @Param onShown: (() => void) | null = null
  @Param screenTabIndex: number = 0
  @Local headerActions: ActionSlot[] = []

  async aboutToAppear() {
    const policy = await ScreenPolicyBridge.getInstance().resolveScreenPolicy(this.screenRole)
    if (policy !== null) {
      this.headerActions = policy.actionSlots
    }
  }

  build() {
    // 页面底色由各页自己的根容器设置（HomeScreen / StarMapScreen / StatsScreen 用
    // sys.color.background_secondary），这里不再包一层 Stack：
    // 实测 HdsNavDestination 会用自己的白色 surface 覆盖设在它上面的 backgroundColor，
    // 而包在 contentBuilder 外面时这个 Stack 又拿不到确定高度，backgroundColor 同样不生效。
    // 只改背景色，不碰 systemMaterialEffect，材质类型与等级仍完全由系统决定。
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
    .onShown(() => {
      NavigationChromeStore.getInstance().reportScreenRole(this.screenRole, this.screenTabIndex)
      if (this.onShown) {
        this.onShown()
      }
    })
  }
}
