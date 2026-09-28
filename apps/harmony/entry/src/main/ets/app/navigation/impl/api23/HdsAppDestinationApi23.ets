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
    HdsNavDestination() {
      this.contentBuilder()
    }
    .hideTitleBar(false)
    .titleBar({
      enableComponentSafeArea: true,
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
