// Index — 应用入口。
// 只做入口和 Navigation 宿主：渲染 AppNavigation，路由由 app/navigation 统一接管。

import { AppNavigation } from '../app/navigation/AppNavigation'

@Entry
@Component
struct Index {
  build() {
    Column() {
      AppNavigation()
    }
    .width('100%')
    .height('100%')
  }
}
