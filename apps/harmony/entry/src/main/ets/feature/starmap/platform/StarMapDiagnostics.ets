// ─── StarMapDiagnostics.ets ───
// 星图手势边界日志（Issue #816）。
//
// 只在**手势边界**打点（开始 / 结束 / 命中 / 选中 / 连线成败），不在每帧打。
// #816 的诊断包里"没有 createEdge / add_starmap_edge 记录"、"反复 Touch test result is empty"，
// 说明当时既看不出手势归属哪一层，也看不出连线到底走没走通——
// 补齐这六个边界事件，真机问题就能从日志直接定位，不用再靠猜。
//
// 字段名固定成 issue 里要求的那些（pinch_begin / pinch_end / recursive_hit /
// selection_changed / connect_begin / connect_end），后面看日志不用再对代码。

import hilog from '@ohos.hilog'
import { StarMapTargetPath } from '../../../corebridge/dto/StarMapDtos'
import { RecursiveHit, describeScenePath, describeTargetPath } from './StarMapGeometry'

const DOMAIN: number = 0x0000
const TAG: string = 'StarMapGesture'

/** 统一前缀，方便在 hilog 里按 Issue #816 过滤。 */
const PREFIX: string = '#816'

/**
 * 数字格式化，全函数（total）。
 *
 * 这一层是**纯诊断**，它的存在是为了"出问题时不崩"，不是"自己也能把 App 崩了"。
 * 真机上 double-touch 的第一帧里，某些触摸点的相对坐标字段并不总是有值——
 * #816 首轮真机复测就是这么崩的：\`logPinchBegin\` 直接对可能为 undefined 的
 * 坐标调 \`.toFixed()\`，一个 TypeError 打断 pinch_begin，导致双指一缩放就闪退。
 *
 * 所以这里对 undefined / null / NaN / Infinity 一律返回可读占位符，绝不抛。
 * 真出问题时日志里会看到 \`none\` / \`nan\`，这本身就是有用的信号，
 * 比整个 App 挂掉要好得多。
 */
function fmt(value: number | undefined | null, digits: number): string {
  if (value === undefined || value === null) {
    return 'none'
  }
  if (typeof value !== 'number' || Number.isNaN(value)) {
    return 'nan'
  }
  if (!Number.isFinite(value)) {
    return 'inf'
  }
  return value.toFixed(digits)
}

/** 双指缩放开始：归属哪一层、双指中心在哪、起始比例多少。 */
export function logPinchBegin(
  ownerScenePath: string,
  centerX: number,
  centerY: number,
  baseScale: number
): void {
  hilog.info(
    DOMAIN, TAG,
    `${PREFIX} pinch_begin ownerScenePath=${ownerScenePath} centerX=${fmt(centerX, 1)} ` +
    `centerY=${fmt(centerY, 1)} baseScale=${fmt(baseScale, 3)}`
  )
}

/** 双指缩放结束：归属哪一层、最终比例多少。 */
export function logPinchEnd(ownerScenePath: string, finalScale: number): void {
  hilog.info(
    DOMAIN, TAG,
    `${PREFIX} pinch_end ownerScenePath=${ownerScenePath} finalScale=${fmt(finalScale, 3)}`
  )
}

/**
 * 递归命中结果：最深的真实目标落在哪一层的哪个对象上。
 * target 为 null 表示落在空白处（这时只打归属层）。
 */
export function logRecursiveHit(hit: RecursiveHit): void {
  if (hit.target === null) {
    hilog.info(
      DOMAIN, TAG,
      `${PREFIX} recursive_hit resolvedScenePath=${describeScenePath(hit.ownerScenePath)} ` +
      `kind=blank itemId=`
    )
    return
  }
  hilog.info(
    DOMAIN, TAG,
    `${PREFIX} recursive_hit resolvedScenePath=${describeScenePath(hit.target.scenePath)} ` +
    `kind=${hit.target.objectKind} itemId=${hit.target.objectId} region=${hit.target.hitRegion} ` +
    `ownerScenePath=${describeScenePath(hit.ownerScenePath)}`
  )
}

/** 选中变化：哪一层的什么类型对象被选中了。 */
export function logSelectionChanged(scenePath: string, kind: string, itemId: string): void {
  hilog.info(
    DOMAIN, TAG,
    `${PREFIX} selection_changed scenePath=${scenePath} kind=${kind} itemId=${itemId}`
  )
}

/** 连线开始：拖拽源对象的完整路径。 */
export function logConnectBegin(sourcePath: StarMapTargetPath): void {
  hilog.info(
    DOMAIN, TAG,
    `${PREFIX} connect_begin sourcePath=${describeTargetPath(sourcePath)}`
  )
}

/**
 * 连线结束：两端路径、落在哪张图上、成没成功。
 * canceled 表示没落点（空白 / 自身 / 宿主解析不出来），这三种都归到 cancel，
 * 免得诊断时把"正常取消"和"落库失败"混在一起看。
 */
export function logConnectEnd(
  sourcePath: StarMapTargetPath | null,
  targetPath: StarMapTargetPath | null,
  hostStarmapId: string,
  success: boolean,
  reason: string
): void {
  hilog.info(
    DOMAIN, TAG,
    `${PREFIX} connect_end sourcePath=${sourcePath !== null ? describeTargetPath(sourcePath) : 'none'} ` +
    `targetPath=${targetPath !== null ? describeTargetPath(targetPath) : 'none'} ` +
    `hostStarmapId=${hostStarmapId} ${success ? 'success' : 'cancel'}` +
    (reason.length > 0 ? ` reason=${reason}` : '')
  )
}
