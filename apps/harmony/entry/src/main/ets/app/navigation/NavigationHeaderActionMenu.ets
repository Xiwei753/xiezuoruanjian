// NavigationHeaderActionMenu — 普通 ArkUI NavDestination 菜单项构建器（API < 23）。
// 返回平台真实类型 NavigationMenuItem[]，不依赖 @kit.UIDesignKit。
// 与 HdsHeaderActionMenu（API23+ HDS 类型）分流，共享 NavigationHeaderActionModel 排序和 Dispatcher 行为。
//
// Issue #784 评论 5858136516：拆分 HdsHeaderActionMenu，普通 ArkUI 菜单项独立文件。

import { ActionSlot, ActionRole } from '../../corebridge/dto/PlatformDtos'
import { sortedHeaderTrailingActions } from './NavigationHeaderActionModel'
import { NavigationHeaderActionDispatcher } from './NavigationHeaderActionDispatcher'

/**
 * 构建 NavDestination .menus() 所需的菜单项数组（API < 23）。
 * 返回平台真实类型 NavigationMenuItem[]，每个项包含 value（必填）、icon、action。
 * 仅处理 Sync / Search / Settings 三个 ActionRole，其余跳过。
 */
export function buildNavMenuItems(actionSlots: ActionSlot[]): NavigationMenuItem[] {
  const sorted = sortedHeaderTrailingActions(actionSlots)
  const items: NavigationMenuItem[] = []

  for (const slot of sorted) {
    const mapped = mapNavMenuItem(slot)
    if (mapped !== null) {
      items.push(mapped)
    }
  }

  return items
}

/** ActionRole → NavigationMenuItem 映射；非 Sync/Search/Settings 返回 null */
function mapNavMenuItem(slot: ActionSlot): NavigationMenuItem | null {
  if (slot.role === ActionRole.Sync) {
    return {
      value: '同步',
      icon: $r('app.media.ic_refresh'),
      action: () => {
        NavigationHeaderActionDispatcher.getInstance().dispatchAction(slot.role)
      }
    }
  }

  if (slot.role === ActionRole.Search) {
    return {
      value: '搜索',
      icon: $r('app.media.ic_search'),
      action: () => {
        NavigationHeaderActionDispatcher.getInstance().dispatchAction(slot.role)
      }
    }
  }

  if (slot.role === ActionRole.Settings) {
    return {
      value: '设置',
      icon: $r('app.media.ic_settings'),
      action: () => {
        NavigationHeaderActionDispatcher.getInstance().dispatchAction(slot.role)
      }
    }
  }

  return null
}
