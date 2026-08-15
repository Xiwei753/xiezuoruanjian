// AppNavigation — 应用统一导航宿主。
// 用 HarmonyOS Navigation 组件 + NavDestination，按 Routes 装载对应 feature/*/ui Screen。
// 不再用 router 跳转。

import { Routes } from './Routes'
import { HomeScreen } from '../../feature/project/ui/HomeScreen'
import { WorkspaceScreen } from '../../feature/project/ui/WorkspaceScreen'
import { WritingScreen } from '../../feature/editor/ui/WritingScreen'
import { StarMapScreen } from '../../feature/starmap/ui/StarMapScreen'
import { SettingsScreen } from '../../feature/settings/ui/SettingsScreen'
import { StatsScreen } from '../../feature/stats/ui/StatsScreen'

@Component
export struct AppNavigation {
  @Provide('navPathStack') navPathStack: NavPathStack = new NavPathStack()

  aboutToAppear(): void {
    // 默认显示 Home。
    this.navPathStack.pushPath({ name: Routes.Home })
  }

  @Builder
  PageMap(name: string) {
    if (name === Routes.Home) {
      HomeScreen()
    } else if (name === Routes.Workspace) {
      WorkspaceScreen()
    } else if (name === Routes.Writing) {
      WritingScreen()
    } else if (name === Routes.StarMap) {
      StarMapScreen()
    } else if (name === Routes.Settings) {
      SettingsScreen()
    } else if (name === Routes.Stats) {
      StatsScreen()
    }
  }

  build() {
    Navigation(this.navPathStack) {
      // 内容由 navPathStack 驱动。
    }
    .navDestination(this.PageMap)
    .mode(NavigationMode.Stack)
    .hideTitleBar(true)
  }
}
