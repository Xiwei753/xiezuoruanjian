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

import { HdsNavDestination, HdsNavigationTitleBarOptions, hdsMaterial } from '@kit.UIDesignKit'
import { NavigationHeaderActionDispatcher } from '../../NavigationHeaderActionDispatcher'
import { ActionRole } from '../../../../corebridge/dto/PlatformDtos'

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

  build() {
    HdsNavDestination() {
      this.contentBuilder()
    }
    .hideTitleBar(false)
    .titleBar({
      enableComponentSafeArea: true,
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
