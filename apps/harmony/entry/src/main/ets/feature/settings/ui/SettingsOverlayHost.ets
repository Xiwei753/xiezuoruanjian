// SettingsOverlayHost — 宽屏设置悬浮层宿主。
// Issue #825：宽屏（WorkspaceLayoutMode.Workbench）下设置悬浮在打开设置前的页面之上，
// 而不是再压一条路由，所以本组件挂在三个 tab 的 Navigation 之上，只负责遮罩 + 大号面板。
// 边界：
// - 关闭悬浮层只调 SettingsOverlayController.close()，下面的原页面和原 NavPathStack 不动。
// - 面板内容是 SettingsContent 的 twoColumn 形态（顶部搜索 + 两列折叠分组），
//   设置内容、保存逻辑、主题逻辑仍走现有 SettingsViewModel / backend，这里不复制任何设置状态。
import { SettingsContent } from './SettingsContent'
import { SettingsOverlayController } from '../presentation/SettingsOverlayController'
import { DesignTokens } from '../../../ui/theme/DesignTokens'

@ComponentV2
export struct SettingsOverlayHost {
  // Issue #825 复核第3项：必须用 @Local 持有 controller，@Local 才能把
  // SettingsOverlayController 的 @Trace visible 接进本组件的 V2 状态树；
  // 用普通成员变量的话 visible 变化不会触发重渲染，悬浮层永远不显示。
  @Local controller: SettingsOverlayController = SettingsOverlayController.getInstance()

  build() {
    // 不可见时不占任何布局：整个组件直接不渲染，避免影响下面页面的尺寸分配。
    if (this.controller.visible) {
      Stack({ alignContent: Alignment.Center }) {
        // 遮罩层：点按关闭。背景仍是打开设置前的页面，只是加一层暗色。
        Column()
          .width('100%')
          .height('100%')
          .backgroundColor('#66000000')
          .onClick(() => { this.controller.close() })

        // 大号设置面板：内容宽度受 maxWidth 限制，两列折叠分组由 SettingsContent 负责。
        Column() {
          SettingsContent({
            twoColumn: true,
            onRequestClose: () => { this.controller.close() }
          })
        }
        .width('100%')
        .height('100%')
        .constraintSize({ maxWidth: 1120, maxHeight: 880 })
        .backgroundColor($r('sys.color.background_secondary'))
        .borderRadius(DesignTokens.dialogRadius)
        .margin({ left: 32, right: 32, top: 32, bottom: 32 })
      }
      .width('100%')
      .height('100%')
    }
  }
}