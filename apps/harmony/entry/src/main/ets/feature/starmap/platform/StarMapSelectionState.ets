// ─── StarMapSelectionState.ets ───
// 全树唯一选中态（Issue #816）。
//
// 之前每层 Scene 各存一份 selectedNodeId，于是"选中父 Embed 再点子节点"两处都还亮着，
// 而且没法判断"点的是哪一层的哪个东西"。这里改成全树一份：
// 选中身份 = scenePath + kind + itemId，任意一层的 Scene 都能读到同一份真相。
//
// 用 V1 的 @Observed + @ObjectLink：星图这棵组件树本身是 V1 装饰器
// （@Component/@State/@Prop/@Link），和 V2 混用反而会多一层代理。
// @ObjectLink 只观察一层属性，所以每次改选中都**整体换成新的数组/字符串**，
// 绝不能原地改数组元素，否则观察不到变化。

import { StarMapPathSegment } from '../../../corebridge/dto/StarMapDtos'
import { isSameScenePath, describeScenePath } from './StarMapGeometry'

/** 选中对象的类型。edge 也算一种选中身份：一条边被选中时 kind 就是 edge。 */
export type StarMapSelectionKind = 'none' | 'node' | 'embed' | 'edge'

/** 一次选中的完整身份：哪一层 Scene 的哪个对象。 */
export interface StarMapSelection {
  /** 容纳该对象的 Scene 绝对路径 */
  scenePath: StarMapPathSegment[]
  /** 对象类型 */
  kind: StarMapSelectionKind
  /** 对象 ID（nodeId / Embed instanceId / edgeId），'none' 时为空串 */
  itemId: string
}

@Observed
export class StarMapSelectionState {
  /** 当前选中对象所在 Scene 的绝对路径 */
  selectedScenePath: StarMapPathSegment[] = []
  /** 当前选中对象类型 */
  selectedKind: StarMapSelectionKind = 'none'
  /** 当前选中对象 ID */
  selectedItemId: string = ''

  /**
   * 选中一个对象。
   *
   * 语义是"替换"而不是"追加"：选中子节点时父 Embed 的选中态立刻消失，
   * 这正是 #816 要求的行为，也是全树唯一选中态的题中之义。
   */
  select(scenePath: StarMapPathSegment[], kind: StarMapSelectionKind, itemId: string): void {
    this.selectedScenePath = scenePath.map((seg: StarMapPathSegment): StarMapPathSegment => ({
      type: seg.type, instanceId: seg.instanceId, nodeId: seg.nodeId
    }))
    this.selectedKind = kind
    this.selectedItemId = itemId
  }

  /** 清空选中（点空白处）。 */
  clear(): void {
    this.selectedScenePath = []
    this.selectedKind = 'none'
    this.selectedItemId = ''
  }

  /** 读取选中快照。 */
  getSelection(): StarMapSelection {
    return {
      scenePath: this.selectedScenePath.map((seg: StarMapPathSegment): StarMapPathSegment => ({
        type: seg.type, instanceId: seg.instanceId, nodeId: seg.nodeId
      })),
      kind: this.selectedKind,
      itemId: this.selectedItemId
    }
  }

  /** 当前是否有选中对象。 */
  hasSelection(): boolean {
    return this.selectedKind !== 'none' && this.selectedItemId.length > 0
  }

  /** 指定 Scene 里的指定对象是否被选中。 */
  isSelected(scenePath: StarMapPathSegment[], kind: StarMapSelectionKind, itemId: string): boolean {
    return this.hasSelection() &&
      this.selectedKind === kind &&
      this.selectedItemId === itemId &&
      isSameScenePath(this.selectedScenePath, scenePath)
  }

  /**
   * 本 Scene 是否就是当前选中对象所在的 Scene。
   * 用来决定"要不要把 Embed 描粗"这类局部渲染差异。
   */
  isSceneSelected(scenePath: StarMapPathSegment[]): boolean {
    return this.hasSelection() && isSameScenePath(this.selectedScenePath, scenePath)
  }

  /** 选中项的可读描述，日志和诊断用。 */
  describeSelection(): string {
    if (!this.hasSelection()) {
      return 'none'
    }
    return `${describeScenePath(this.selectedScenePath)}#${this.selectedKind}:${this.selectedItemId}`
  }
}
