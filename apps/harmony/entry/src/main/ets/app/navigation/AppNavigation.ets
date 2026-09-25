// AppNavigation — 应用统一导航宿主。
// Issue #767：改为装载 PrimaryTabShell（真正的横向 Tabs + BottomTabBarStyle）。
// 删除手写 BottomTabBar builder 和 switchToTab 的 clear() + pushPath() 实现。
// 每个 tab 各自持有自己的 NavPathStack 和 Navigation，一级切换只切 tab index。
//
// 修复 navPathStack render 期间修改问题：
// 不再在 aboutToAppear() 里 pushPath(Home)，NavPathStack 在 PrimaryTabShell 字段初始化时
// 通过工厂函数 createStackWithRoot() 创建并带根路由。
//
// 导航事务逻辑（意图队列、离开守卫、安全 pop/replace/rebuild）在 NavigationTransactionCoordinator。
// 接续恢复逻辑保留在 AppNavigation（通过 coordinator 获取活动栈操作）。

import { Routes, WorkspaceRouteParams, WritingRouteParams } from './Routes'
import hilog from '@ohos.hilog'
import { ContinuationPayload } from '../../platform/continuity/ContinuationPayload'
import { NavigationTransactionCoordinator } from './NavigationTransactionCoordinator'
import { PrimaryTabShell } from './PrimaryTabShell'

// 一级导航 Tab 索引常量
const TAB_HOME: number = 0    // 作品
const TAB_STARMAP: number = 1 // 星图
const TAB_STATS: number = 2   // 统计

@Component
export struct AppNavigation {
  // 当前接续恢复的编辑器状态（selection/cursor/viewport/contentHash），供页面层主动读取。
  @State continuationPayload: ContinuationPayload | null = null
  // Issue #767：一级导航当前选中 Tab（0=作品, 1=星图, 2=统计），与 PrimaryTabShell 通过 @Link 共享。
  @State currentTab: number = TAB_HOME

  aboutToAppear(): void {
    hilog.info(0x0000, 'AppNavigation', 'aboutToAppear: registering with coordinator')
    // 注册到 coordinator，使 EntryAbility 能触发接续恢复。
    // Issue #767 评论5830638688 第1项：不再创建 placeholderStack 污染 activeTabStack。
    // 只注册 restoreListener 回调；pending payload 的消费延迟到 PrimaryTabShell 注册 homeStack
    // 后由 tryConsumePendingRestore() 触发。
    const coordinator = NavigationTransactionCoordinator.getInstance()
    coordinator.registerRestoreListener((payload: ContinuationPayload) => {
      this.applyContinuation(payload)
    })
  }

  aboutToDisappear(): void {
    NavigationTransactionCoordinator.getInstance().unregister()
  }

  // 根据接续 payload 重建 NavPathStack 并恢复 selection/cursor/viewport。
  // NavPathStack 不能自动迁移（SUPPORT_CONTINUE_PAGE_STACK_KEY=false），这里手动重建。
  // Issue #629 评论14 第2项：改用 safeClearAndRebuild，先调 leaveGuard（保存+关闭事务），
  // guard 成功才 clear + rebuild；失败保持当前页面，不清栈（不卸载活跃 WritingScreen 绕过保存）。
  // Issue #767：接续恢复有 project/chapter 时先切到作品 tab，再重建作品栈。
  private async applyContinuation(payload: ContinuationPayload): Promise<void> {
    const dedupeKey = `continuation:${payload.chapterId ?? ''}:${payload.projectId ?? ''}`
    await NavigationTransactionCoordinator.getInstance().safeClearAndRebuild(dedupeKey, (): void => {
      // 接续恢复始终回到 Home tab。
      // Issue #767 评论5832129578 第1项：currentTab 变化会通过 @Watch 触发
      // PrimaryTabShell.syncActiveTab，不再依赖 Tabs.onChange 回调。
      this.currentTab = TAB_HOME

      // Issue #767：用 getTabStack(TAB_HOME) 获取作品 tab 的 NavPathStack，
      // 而不是 getNavPathStack()（可能返回其他 tab 的 stack）。
      const coordinator = NavigationTransactionCoordinator.getInstance()
      const homeStack = coordinator.getTabStack(TAB_HOME)
      if (homeStack === null) {
        // PrimaryTabShell 还没注册 homeStack，不消费 payload（保留 pending）。
        hilog.error(0x0000, 'AppNavigation', 'applyContinuation: homeStack not registered yet, cannot rebuild')
        return
      }

      // 清栈到根后按 payload 重建。clear() 清空全部路由，再 push Home 作为根。
      homeStack.clear()
      homeStack.pushPath({ name: Routes.Home })

      // 底栏显隐由 ScreenPolicy 决定，不再手工设置。
      // continuation 导航到 Workspace 时 showPrimaryNavigation=true（底栏显示），
      // 导航到 Writing 时 showPrimaryNavigation=false（底栏隐藏），与普通进入一致。
      const hasProject: boolean = payload.projectId !== undefined && payload.projectId !== ''
      const hasChapter: boolean = payload.chapterId !== undefined && payload.chapterId !== ''
      const hasVolume: boolean = payload.volumeId !== undefined && payload.volumeId !== ''

      if (hasProject) {
        const wsParam: WorkspaceRouteParams = {
          projectId: payload.projectId as string,
          projectName: '' // payload 不带 projectName；WorkspaceScreen 会从 core 加载。
        }
        homeStack.pushPath({ name: Routes.Workspace, param: wsParam })
      }

      if (hasChapter && hasProject && hasVolume) {
        // 携带 selection/cursor/viewport/contentHash 到 WritingScreen 入口。
        const wParam: WritingRouteParams = {
          chapterId: payload.chapterId as string,
          chapterTitle: '',
          projectId: payload.projectId as string,
          volumeId: payload.volumeId as string
        }
        if (payload.selection !== undefined) {
          wParam.selection = payload.selection
        }
        if (payload.cursor !== undefined) {
          wParam.cursor = payload.cursor
        }
        if (payload.viewport !== undefined) {
          wParam.viewport = payload.viewport
        }
        if (payload.contentHash !== undefined) {
          wParam.contentHash = payload.contentHash
        }
        homeStack.pushPath({ name: Routes.Writing, param: wParam })
      }

      // 暂存 payload，供页面层主动读取编辑器状态（selection/cursor/viewport）。
      this.continuationPayload = payload
    })
  }

  build() {
    Column() {
      // Issue #767：装载 PrimaryTabShell（真正的横向 Tabs + BottomTabBarStyle）。
      // currentTab 通过 @Link 共享，接续恢复时可从 AppNavigation 切到 Home tab。
      PrimaryTabShell({ currentTab: $currentTab })
    }
    .width('100%')
    .height('100%')
  }
}
