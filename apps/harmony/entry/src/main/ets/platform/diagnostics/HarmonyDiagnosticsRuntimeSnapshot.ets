// HarmonyDiagnosticsRuntimeSnapshot — 运行态快照序列化与持久化。
//
// Issue #773 评论 5845033983：把 theme / navigation / editor 当前状态序列化成
// 三个 JSON 字符串，Checkpoint 和 Exporter 都复用这个类，避免两份序列化逻辑。
//
// 三个方法返回 JSON 字符串：
// - getThemeRuntimeJson(): 从 HarmonyThemeAdapter 读取主题状态
// - getUiRuntimeJson(): 从 NavigationChromeStore 和 AdaptiveContext 读取 UI 状态
// - getEditorRuntimeJson(): 从 EditorRuntimeSnapshotStore 读取编辑器状态
//
// persistAll(context): 把三个 JSON 和归档元数据交给诊断归档 Worker，由 Worker
// 在 filesDir/diagnostics/live/runtime/ 下做双槽原子写入；ArkUI 线程不再执行
// 任何 fs.*Sync。高频编辑器/窗口变化在 Worker 侧合并为最后一次完整 payload。

import { common, ConfigurationConstant } from '@kit.AbilityKit'
import hilog from '@ohos.hilog'

import { HarmonyThemeAdapter } from '../../ui/theme/HarmonyThemeAdapter'
import { NavigationChromeStore } from '../../app/navigation/NavigationChromeStore'
import { AdaptiveContext } from '../../ui/adaptive/AdaptiveContext'
import { NavigationTransactionCoordinator } from '../../app/navigation/NavigationTransactionCoordinator'
import { EditorRuntimeSnapshotStore, CaretRectSnapshot, CurrentLineMetricsSnapshot } from '../../feature/editor/diagnostics/EditorRuntimeSnapshotStore'
import { ScreenRole } from '../../app/navigation/ScreenSemantics'
import { WidthClass } from '../../corebridge/dto/PlatformDtos'
import { PlatformApiResolver } from '../version/PlatformApiResolver'
import { HarmonyMaterialRuntimeApi26 } from '../material/impl/api26/HarmonyMaterialRuntimeApi26'
import { HarmonyDiagnosticsArchiveWorkerClient } from './HarmonyDiagnosticsArchiveWorkerClient'

const LOG_TAG = 'HarmonyDiagnosticsRuntimeSnapshot'
const DIAGNOSTICS_DIR = 'diagnostics'
const RUNTIME_SUBDIR = 'live/runtime'

// 归档元数据文件 — 只保存最近一次归档时间/路径/计数，不包含日志内容。
const ARCHIVE_STATE_FILE = 'archive-state.json'

// ── Theme runtime DTO ──────────────────────────────────────────
// 与 Exporter 中 ThemeRuntimeAttachment interface 字段完全对齐。
interface ThemeRuntimeDto {
  systemColorMode: ConfigurationConstant.ColorMode
  appearanceMode: string
  effectiveColorMode: string
  useSystemTokens: boolean
  hasExplicitThemeOverride: boolean
  selectedBuiltinThemeId: string
  // Issue #773 评论 5846511131：材质配置状态（不是支持状态）。
  // state 是 module.json5 的材质配置值，不代表 HDS 组件已实际生效。
  // 删除 immersiveMaterialSupported（state !== DISABLE 推导不成立），
  // 改为真实语义字段。
  appMaterialState: string
  appMaterialType: string
  hdsRequestedMaterialType: string
  hdsRequestedMaterialLevel: string
}

// ── UI runtime DTO ─────────────────────────────────────────────
// 与 Exporter 中 UiRuntimeAttachment interface 字段完全对齐。
interface UiRuntimeDto {
  screenRole: string
  widthClass: string
  windowWidthVp: number
  windowHeightVp: number
  currentTab: number
  homeStackDepth: number
  starMapStackDepth: number
  statsStackDepth: number
}

// ── Editor runtime DTO ─────────────────────────────────────────
// 与 Exporter 中 EditorRuntimeAttachment interface 字段完全对齐。
interface EditorRuntimeDto {
  hasActiveSession: boolean
  revision: number
  generation: number
  textUtf8Bytes: number
  textUtf16Length: number
  cursorByteOffset: number
  cursorUtf16Offset: number
  selectionAnchorByte: number
  selectionHeadByte: number
  hasComposition: boolean
  compositionStartByte: number
  compositionEndByte: number
  fontSizeVp: number
  lineSpacingMultiplier: number
  effectiveLineHeightVp: number
  firstLineIndentEnabled: boolean
  firstLineIndentChars: number
  effectiveFirstLineIndentVp: number
  viewportWidthPx: number
  viewportHeightPx: number
  contentWidthPx: number
  contentHeightPx: number
  systemLayoutLineCount: number
  caretRect: CaretRectSnapshot | null
  selectionRectCount: number
  currentLineMetrics: CurrentLineMetricsSnapshot | null
  imeAttached: boolean
  editorFocused: boolean
  currentScrollX: number
  currentScrollY: number
  leadingWhitespaceCountOfCurrentParagraph: number
  layoutSource: string
  usingLayoutManager: boolean
  apiLevel: number
  lastLayoutError: string | null
  fallbackCount: number
  isEditorMounted: boolean
  capturedAtMs: number
  layoutRevision: number
  layoutGeneration: number
  layoutCompositionGeneration: number
  visualCaretAffinity: string
  displayedSessionId: number
  displayedRevision: number
  displayedLayoutLineCount: number
  candidateSessionId: number
  candidateRevision: number
  candidateLayoutLineCount: number
  isMotionAnimating: boolean
  displayedFrameAnimating: boolean
  displayedMotionWindowCount: number
  staticClipMode: string
  staticClipLayoutRevision: number
  staticClipLayoutGeneration: number
  staticVisibleRanges: string
  motionGlyphRanges: string
  staticMaskRectCount: number
  staticMaskEmpty: boolean
}

export class HarmonyDiagnosticsRuntimeSnapshot {
  private static instance: HarmonyDiagnosticsRuntimeSnapshot

  private constructor() {}

  static getInstance(): HarmonyDiagnosticsRuntimeSnapshot {
    if (!HarmonyDiagnosticsRuntimeSnapshot.instance) {
      HarmonyDiagnosticsRuntimeSnapshot.instance = new HarmonyDiagnosticsRuntimeSnapshot()
    }
    return HarmonyDiagnosticsRuntimeSnapshot.instance
  }

  // 主题运行态 JSON — 从 HarmonyThemeAdapter 读取主题状态。
  getThemeRuntimeJson(): string {
    const adapter = HarmonyThemeAdapter.getInstance()
    const useSystemTokens = adapter.isUsingSystemTokens()

    // Issue #773 评论 5846511131：材质配置状态（不是支持状态）。
    // API >= 26 从 uiMaterial.getMaterialInfo() 读取；API < 26 字段写 "unavailable"。
    // state 是 module.json5 的材质配置值（DEFAULT/ENABLE/DISABLE），不代表 HDS 组件已实际生效。
    let appMaterialState = 'unavailable'
    let appMaterialType = 'unavailable'
    const hdsRequestedMaterialType = 'IMMERSIVE'
    const hdsRequestedMaterialLevel = 'ADAPTIVE'
    if (PlatformApiResolver.getInstance().isApiAtLeast(26)) {
      try {
        const materialInfo = HarmonyMaterialRuntimeApi26.getInstance().getMaterialRuntimeInfo()
        appMaterialState = materialInfo.appMaterialState
        appMaterialType = materialInfo.appMaterialType
      } catch (e) {
        hilog.error(0x0000, LOG_TAG, 'read material runtime failed: %{public}s', String(e))
        appMaterialState = 'error'
      }
    }

    const dto: ThemeRuntimeDto = {
      systemColorMode: adapter.getSystemColorMode(),
      appearanceMode: adapter.getMode(),
      effectiveColorMode: adapter.getEffectiveColorMode(),
      useSystemTokens: useSystemTokens,
      hasExplicitThemeOverride: !useSystemTokens,
      selectedBuiltinThemeId: adapter.getSelectedBuiltinThemeId(),
      appMaterialState: appMaterialState,
      appMaterialType: appMaterialType,
      hdsRequestedMaterialType: hdsRequestedMaterialType,
      hdsRequestedMaterialLevel: hdsRequestedMaterialLevel
    }
    return JSON.stringify(dto, null, 2)
  }

  // UI 运行态 JSON — 从 NavigationChromeStore 和 AdaptiveContext 读取 UI 状态。
  // 逻辑直接从 Exporter 的 addUiRuntimeAttachment 方法搬运。
  getUiRuntimeJson(): string {
    // 从 AdaptiveContext 获取 WidthClass 和窗口尺寸
    const adaptiveState = AdaptiveContext.getInstance().getCurrentState()
    const widthClass: WidthClass = adaptiveState.sizeClass
    const windowWidthVp: number = adaptiveState.widthVp
    const windowHeightVp: number = adaptiveState.heightVp

    // 从 NavigationChromeStore 获取当前 ScreenRole
    const screenRole: ScreenRole = NavigationChromeStore.getInstance().getCurrentRole()

    // 从 NavigationTransactionCoordinator 获取当前 tab 和各 stack 深度
    const coordinator = NavigationTransactionCoordinator.getInstance()
    const activeStack = coordinator.getNavPathStack()

    // 通过引用比较确定当前活动 tab（coordinator 没有 getActiveTab getter）
    let currentTab: number = -1
    if (activeStack !== null) {
      for (let i = 0; i <= 2; i++) {
        const tabStack = coordinator.getTabStack(i)
        if (tabStack === activeStack) {
          currentTab = i
          break
        }
      }
    }

    // 各 tab 导航栈深度（NavPathStack.size() 返回栈中路径数）
    const homeStack = coordinator.getTabStack(0)
    const starMapStack = coordinator.getTabStack(1)
    const statsStack = coordinator.getTabStack(2)
    const homeStackDepth: number = homeStack !== null ? homeStack.size() : -1
    const starMapStackDepth: number = starMapStack !== null ? starMapStack.size() : -1
    const statsStackDepth: number = statsStack !== null ? statsStack.size() : -1

    const dto: UiRuntimeDto = {
      screenRole: screenRole,
      widthClass: widthClass,
      windowWidthVp: windowWidthVp,
      windowHeightVp: windowHeightVp,
      currentTab: currentTab,
      homeStackDepth: homeStackDepth,
      starMapStackDepth: starMapStackDepth,
      statsStackDepth: statsStackDepth
    }
    return JSON.stringify(dto, null, 2)
  }

  // 编辑器运行态 JSON — 从 EditorRuntimeSnapshotStore 读取编辑器状态。
  getEditorRuntimeJson(): string {
    const snapshot = EditorRuntimeSnapshotStore.getInstance().getSnapshot()
    if (snapshot === null) {
      return JSON.stringify({ 'error': 'no editor runtime snapshot available' }, null, 2)
    }
    const dto: EditorRuntimeDto = {
      hasActiveSession: snapshot.hasActiveSession,
      revision: snapshot.revision,
      generation: snapshot.generation,
      textUtf8Bytes: snapshot.textUtf8Bytes,
      textUtf16Length: snapshot.textUtf16Length,
      cursorByteOffset: snapshot.cursorByteOffset,
      cursorUtf16Offset: snapshot.cursorUtf16Offset,
      selectionAnchorByte: snapshot.selectionAnchorByte,
      selectionHeadByte: snapshot.selectionHeadByte,
      hasComposition: snapshot.hasComposition,
      compositionStartByte: snapshot.compositionStartByte,
      compositionEndByte: snapshot.compositionEndByte,
      fontSizeVp: snapshot.fontSizeVp,
      lineSpacingMultiplier: snapshot.lineSpacingMultiplier,
      effectiveLineHeightVp: snapshot.effectiveLineHeightVp,
      firstLineIndentEnabled: snapshot.firstLineIndentEnabled,
      firstLineIndentChars: snapshot.firstLineIndentChars,
      effectiveFirstLineIndentVp: snapshot.effectiveFirstLineIndentVp,
      viewportWidthPx: snapshot.viewportWidthPx,
      viewportHeightPx: snapshot.viewportHeightPx,
      contentWidthPx: snapshot.contentWidthPx,
      contentHeightPx: snapshot.contentHeightPx,
      systemLayoutLineCount: snapshot.systemLayoutLineCount,
      caretRect: snapshot.caretRect,
      selectionRectCount: snapshot.selectionRectCount,
      currentLineMetrics: snapshot.currentLineMetrics,
      imeAttached: snapshot.imeAttached,
      editorFocused: snapshot.editorFocused,
      currentScrollX: snapshot.currentScrollX,
      currentScrollY: snapshot.currentScrollY,
      leadingWhitespaceCountOfCurrentParagraph: snapshot.leadingWhitespaceCountOfCurrentParagraph,
      layoutSource: snapshot.layoutSource,
      usingLayoutManager: snapshot.usingLayoutManager,
      apiLevel: snapshot.apiLevel,
      lastLayoutError: snapshot.lastLayoutError,
      fallbackCount: snapshot.fallbackCount,
      isEditorMounted: snapshot.isEditorMounted,
      capturedAtMs: snapshot.capturedAtMs,
      layoutRevision: snapshot.layoutRevision,
      layoutGeneration: snapshot.layoutGeneration,
      layoutCompositionGeneration: snapshot.layoutCompositionGeneration,
      visualCaretAffinity: snapshot.visualCaretAffinity,
      displayedSessionId: snapshot.displayedSessionId,
      displayedRevision: snapshot.displayedRevision,
      displayedLayoutLineCount: snapshot.displayedLayoutLineCount,
      candidateSessionId: snapshot.candidateSessionId,
      candidateRevision: snapshot.candidateRevision,
      candidateLayoutLineCount: snapshot.candidateLayoutLineCount,
      isMotionAnimating: snapshot.isMotionAnimating,
      displayedFrameAnimating: snapshot.displayedFrameAnimating,
      displayedMotionWindowCount: snapshot.displayedMotionWindowCount,
      staticClipMode: snapshot.staticClipMode,
      staticClipLayoutRevision: snapshot.staticClipLayoutRevision,
      staticClipLayoutGeneration: snapshot.staticClipLayoutGeneration,
      staticVisibleRanges: snapshot.staticVisibleRanges,
      motionGlyphRanges: snapshot.motionGlyphRanges,
      staticMaskRectCount: snapshot.staticMaskRectCount,
      staticMaskEmpty: snapshot.staticMaskEmpty
    }
    return JSON.stringify(dto, null, 2)
  }

  // 构建三个运行态 JSON 和归档元数据，一次性投递给归档 Worker。
  // 返回的 Promise 在 Worker 完成原子写入后 resolve；调用方可以忽略，后台收尾
  // 路径会 await 它，保证压缩前元数据已经落盘。
  persistAll(context: common.UIAbilityContext, archiveStateJson: string = ''): Promise<void> {
    const runtimeDir = `${context.filesDir}/${DIAGNOSTICS_DIR}/${RUNTIME_SUBDIR}`
    const archiveStatePath = `${context.filesDir}/${DIAGNOSTICS_DIR}/snapshots/${ARCHIVE_STATE_FILE}`
    return HarmonyDiagnosticsArchiveWorkerClient.getInstance().requestSnapshot(
      runtimeDir,
      archiveStatePath,
      archiveStateJson,
      this.getThemeRuntimeJson(),
      this.getUiRuntimeJson(),
      this.getEditorRuntimeJson()
    ).then((result) => {
      if (!result.success) {
        hilog.error(0x0000, LOG_TAG,
          'runtime snapshot failed: stage=%{public}s reason=%{public}s',
          result.stage, result.message)
      }
    })
  }
}
