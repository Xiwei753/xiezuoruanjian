// HdsWorkspaceDestinationApi23 — API23+ 项目工作区页 NavDestination 壳层。
// Issue #768 评论5835958606：把 HdsNavDestination + HdsNavigationTitleBarOptions
// 收进 impl/api23 组件，共享页面 WorkspaceScreen 不再直接 import @kit.UIDesignKit，
// 避免高版本模块引用进入所有版本都会加载的共享页面，破坏版本隔离。
// Issue #773 评论5844518092：统一为 V2 @ComponentV2 + @Param，消除 V2 页面把 @Builder
// 交给 V1 壳的 @BuilderParam 后 this 变成壳组件导致 adaptiveState undefined 的 TypeError。

import { HdsNavDestination, HdsNavigationTitleBarOptions, hdsMaterial } from '@kit.UIDesignKit'
import { NavigationHeaderActionDispatcher } from '../../NavigationHeaderActionDispatcher'
import { ActionRole } from '../../../../corebridge/dto/PlatformDtos'

@ComponentV2
export struct HdsWorkspaceDestinationApi23 {
  @BuilderParam contentBuilder: () => void
  @Param title: string | Resource = ''
  @Param onBack: (() => void) | null = null
  @Param onShown: (() => void) | null = null

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
          value: [
            {
              label: '同步',
              icon: $r('app.media.ic_refresh'),
              action: () => {
                NavigationHeaderActionDispatcher.getInstance().dispatchAction(ActionRole.Sync)
              }
            },
            {
              label: '搜索',
              icon: $r('app.media.ic_search'),
              action: () => {
                NavigationHeaderActionDispatcher.getInstance().dispatchAction(ActionRole.Search)
              }
            },
            {
              label: '设置',
              icon: $r('sys.media.ohos_ic_public_more'),
              action: () => {
                NavigationHeaderActionDispatcher.getInstance().dispatchAction(ActionRole.Settings)
              }
            }
          ]
        }
      },
      style: {
        systemMaterialEffect: {
          materialType: hdsMaterial.MaterialType.IMMERSIVE,
          materialLevel: hdsMaterial.MaterialLevel.ADAPTIVE
        }
      }
    } as HdsNavigationTitleBarOptions)
    .onBackPressed(() => {
      if (this.onBack) {
        this.onBack()
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
