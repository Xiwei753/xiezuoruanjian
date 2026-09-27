// HdsHeaderActionMenu — API23+ HDS 页头菜单项适配器。
// 输入 Core ScreenPolicy 的 ActionSlot[]，按 sortedHeaderTrailingActions 排序后，
// 做 ActionRole → HdsNavigationBadgeIconOptions 的平台映射，不再在平台端硬编码"同步/搜索/设置"。
//
// Issue #784 评论 5857397503：统一右上角全局动作链路。
// Core ScreenPolicy → NavigationHeaderActionModel → HdsHeaderActionMenu 适配器 → NavigationHeaderActionDispatcher。
// Issue #784 评论 5857856673：HDS 菜单项改用真实 SDK 类型 HdsNavigationBadgeIconOptions，
// 不再用 Object[] 绕过编译器类型检查。
// Issue #784 评论 5858136516：拆分普通 ArkUI buildNavMenuItems 到 NavigationHeaderActionMenu.ets，
// 本文件只保留 HDS（API23+）菜单项构建。

import { HdsNavigationBadgeIconOptions } from '@kit.UIDesignKit'
import { ActionSlot, ActionRole } from '../../../../corebridge/dto/PlatformDtos'
import { sortedHeaderTrailingActions } from '../../NavigationHeaderActionModel'
import { NavigationHeaderActionDispatcher } from '../../NavigationHeaderActionDispatcher'

/**
 * 构建 HDS titleBar.content.menu.value 所需的菜单项数组（API23+）。
 * 返回 HdsNavigationBadgeIconOptions[]，与 HDS SDK 类型系统对齐。
 * 仅处理 Sync / Search / Settings 三个 ActionRole，其余跳过。
 */
export function buildHdsMenuItems(actionSlots: ActionSlot[]): HdsNavigationBadgeIconOptions[] {
  const sorted = sortedHeaderTrailingActions(actionSlots)
  const items: HdsNavigationBadgeIconOptions[] = []

  for (const slot of sorted) {
    const mapped = mapHdsMenuItem(slot)
    if (mapped !== null) {
      items.push(mapped)
    }
  }

  return items
}

/** ActionRole → HDS 菜单项映射；非 Sync/Search/Settings 返回 null */
function mapHdsMenuItem(slot: ActionSlot): HdsNavigationBadgeIconOptions | null {
  if (slot.role === ActionRole.Sync) {
    return {
      content: {
        icon: $r('app.media.ic_refresh'),
        label: '同步',
        action: () => {
          NavigationHeaderActionDispatcher.getInstance().dispatchAction(slot.role)
        }
      }
    }
  }

  if (slot.role === ActionRole.Search) {
    return {
      content: {
        icon: $r('app.media.ic_search'),
        label: '搜索',
        action: () => {
          NavigationHeaderActionDispatcher.getInstance().dispatchAction(slot.role)
        }
      }
    }
  }

  if (slot.role === ActionRole.Settings) {
    return {
      content: {
        icon: $r('app.media.ic_settings'),
        label: '设置',
        action: () => {
          NavigationHeaderActionDispatcher.getInstance().dispatchAction(slot.role)
        }
      }
    }
  }

  return null
}
