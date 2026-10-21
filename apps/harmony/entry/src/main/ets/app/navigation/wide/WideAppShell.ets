// WideAppShell — 宽屏应用外壳的唯一结构入口。
//
// Issue #972：不再用全屏 Stack 叠旧宽屏导航，而是真实布局流：
//   左侧固定一级导航（可整体隐藏） | 右侧列：顶部全局动作条 + 页面画布。
// 写作 / 星图详情 / 设置等由平台动作表要求隐藏一级导航时，整条左栏和右上动作条一起消失，
// 页面画布占满窗口；页面内部再使用自己的专用顶栏。
//
// 本组件只负责“摆壳”，不决定当前 tab、不创建 NavPathStack、不执行业务动作。

import { WidePrimaryRail } from './WidePrimaryRail'
import { WideTopActions } from './WideTopActions'
import { WideLayoutMode } from './WideLayoutMode'
import { ActionSlot } from '../ScreenSemantics'
import { SyncRunSnapshot } from '../../../feature/settings/presentation/SyncRunPresentationState'
import { SyncStatusStrip } from '../../../ui/components/SyncStatusStrip'

const SYNC_STRIP_TOP_GAP_VP: number = 8
const SYNC_STRIP_MAX_WIDTH_VP: number = 560

@Component
export struct WideAppShell {
  /** 当前一级 Tab：0 作品 / 1 星图 / 2 统计 */
  @Prop currentTab: number = 0
  /** 统一布局模式；宽屏外壳只应收到 Wide */
  @Prop layoutMode: WideLayoutMode = WideLayoutMode.Wide
  /** 由平台动作表决定是否显示左侧一级导航与右上全局动作条 */
  @Prop showPrimaryNavigation: boolean = true
  /** 当前页面角色的页头动作槽 */
  @Prop headerActions: ActionSlot[] = []
  /** 窗口顶部安全区高度（vp） */
  @Prop safeTopVp: number = 0
  /** 手动同步状态条是否可见 */
  @Prop showSyncStatus: boolean = false
  @Prop syncSnapshot: SyncRunSnapshot = {
    phase: 'idle',
    source: 'manual',
    startedAtMs: null,
    finishedAtMs: null,
    outcome: null
  }
  /** 用户点击一级导航时交给壳层记录意图并切 tab */
  onTabRequest: (index: number) => void = (_index: number): void => {}
  /** 同步状态条：打开同步设置 */
  onOpenSyncDetails: () => void = (): void => {}
  /** 同步状态条：关闭 */
  onDismissSyncStatus: () => void = (): void => {}
  /** 实际挂载后回报诊断 */
  onMounted: () => void = (): void => {}

  @Builder
  emptyPage() {}

  @BuilderParam pageBuilder: () => void = this.emptyPage

  private get actionTopInsetVp(): number {
    return this.showSyncStatus ? SYNC_STRIP_TOP_GAP_VP : Math.max(this.safeTopVp, SYNC_STRIP_TOP_GAP_VP)
  }

  @Builder
  SyncStatusBanner() {
    Row() {
      SyncStatusStrip({
        snapshot: this.syncSnapshot,
        onOpenDetails: (): void => { this.onOpenSyncDetails() },
        onDismiss: (): void => { this.onDismissSyncStatus() }
      })
        .width('100%')
        .constraintSize({ maxWidth: SYNC_STRIP_MAX_WIDTH_VP })
    }
    .width('100%')
    .padding({
      top: Math.max(this.safeTopVp, SYNC_STRIP_TOP_GAP_VP),
      left: 12,
      right: 12,
      bottom: SYNC_STRIP_TOP_GAP_VP
    })
    .alignItems(VerticalAlign.Center)
  }

  build() {
    Row() {
      if (this.showPrimaryNavigation) {
        WidePrimaryRail({
          currentTab: this.currentTab,
          safeTopVp: this.safeTopVp,
          onTabRequest: (index: number): void => {
            this.onTabRequest(index)
          }
        })

        Divider()
          .vertical(true)
          .height('100%')
          .strokeWidth(0.5)
          .color($r('sys.color.comp_divider'))
      }

      Column() {
        if (this.showPrimaryNavigation) {
          if (this.showSyncStatus) {
            this.SyncStatusBanner()
          }

          WideTopActions({
            actionSlots: this.headerActions,
            topInsetVp: this.actionTopInsetVp,
            visible: true
          })

          Divider()
            .strokeWidth(0.5)
            .color($r('sys.color.comp_divider'))
        }

        Stack() {
          this.pageBuilder()
        }
        .layoutWeight(1)
        .width('100%')
      }
      .layoutWeight(1)
      .height('100%')
    }
    .width('100%')
    .height('100%')
    .backgroundColor($r('sys.color.background_secondary'))
    .onAppear(() => {
      this.onMounted()
    })
  }
}
