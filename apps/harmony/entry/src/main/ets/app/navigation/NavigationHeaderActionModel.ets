// NavigationHeaderActionModel — 把 Core 的 HeaderTrailing slot 按 order 排序，
// 提供统一的页头动作模型。各页面不再各自复制 Sync/Search/Settings 渲染逻辑。
//
// Issue #784 评论 5856805657：全局页头动作收口到唯一入口。

import { ActionSlot, ActionRegion } from '../../corebridge/dto/PlatformDtos'

/** 排序后的页头尾部动作列表 */
export function sortedHeaderTrailingActions(actionSlots: ActionSlot[]): ActionSlot[] {
  return actionSlots
    .filter((slot: ActionSlot) => slot.region === ActionRegion.HeaderTrailing)
    .sort((a: ActionSlot, b: ActionSlot) => a.order - b.order)
}
