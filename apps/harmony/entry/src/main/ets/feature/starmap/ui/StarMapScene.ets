// ─── StarMapScene.ets ───
// 可递归星图场景组件：负责某一张星图的完整显示和交互。
// 可嵌套在 StarMapScreen（根场景）或 StarMapEmbedScene（子星图 Embed 内部）中。
//
// 关键约束：
// 1. 所有 gestureTracker.begin* 调用传入 this.scenePath 作为 ownerScenePath
// 2. 所有 bridge mutation 调用使用 this.starmapId（当前 scene 的 starmapId）
// 3. Embed 的 title/border 区域负责选中 Embed；innerContent 区域让事件穿透到 child StarMapScene
// 4. 递归结构：StarMapScene → StarMapEmbedScene → child StarMapScene
// 5. 菜单和对话框都不属于 Scene 的 Stack：菜单用 ArkUI 程序化 bindMenu（由系统定位、自动避让屏幕边缘），
//    对话框用 openCustomDialog 挂到窗口层级。否则菜单会被圆形 Embed 的 clip(true) 裁掉（Issue #813）。
// 6. bindMenu 按"被绑定组件"定位，所以菜单不能直接挂在全屏背景层或节点本体上：
//    Scene 栈里只放 1vp×1vp 的透明锚点，锚点位置在长按时记成手指的屏幕坐标，
//    这样菜单既贴着手指出现，又不会被父级 clip 裁掉（Issue #813 复审）。
// 7. 菜单必须接 onDisappear 把显示标记写回状态：程序化 bindMenu 的 isShow 不会自动双向同步，
//    不写回的话第二次长按同一对象会因为状态没变化而弹不出来（Issue #813 复审）。
// 8. Embed 的 title / 圆环由外层 Stack 的 onTouchIntercept 动态分流，不再用三根矩形窄条：
//    矩形条被 clip(true) 裁出来的只是圆内几段直线，斜着点边框会掉进 child Scene（Issue #813 复审）。
// 9. 圆形子视图必须同时声明圆形热区（responseRegion）并用 HitTestMode.Block 独占 title / 圆环：
//    组件默认热区是整个矩形，borderRadius + clip 只裁视觉不裁热区；HitTestMode.Default 是
//    "自己和子节点都参加"，父 Embed 和 child Scene 会一起进手势竞争（Issue #813 复审）。
// 10. 预览连线的起点和正式边共用 geometry 的 edgeEndpointBoundaryPoint：
//     预览从对象边界出发，不要从中心出发，否则松手前后会跳变。
// 11. 双指缩放用真正的**高优先级 PinchGesture**（`priorityGesture`），只挂在根 Scene 的
//     最外层 Stack 上；child Scene 一律不挂（#818 复审，真机症状）。
//     之前是根 Stack 上的裸 `onTouch` 自己数手指模拟双指，这在真机上抢不到手势：
//     第一根手指落下时节点/Embed/空白各自的 Tap/Pan/LongPress 识别器就已经在竞争，
//     第二根手指落下时单指那边早就赢了（甚至已经把对象选中了）。
//     `gestureTracker.beginPinch()` 只改我们自己的状态，改不了 ArkUI 识别器的优先级，
//     所以必须换成真正声明优先级的 PinchGesture，用 `event.scale` / `event.pinchCenterX/Y`。
// 12. 双指只有一台**全局相机**（#818）：整棵递归星图一起缩放，缩放目标不再由
//     "两指中心落在哪一层"决定。手指落在节点上、落在子星图上、落在二层子星图上，
//     结果完全一样。旧方向"两指中心递归命中 → 找最深 ownerScenePath → 缩放那个 Scene"已作废。
// 13. 变换只有两种（#818）：根 Scene 的全局相机（用户可改），和每个子 Scene 的
//     局部适配 fitScale + 居中偏移（布局用，用户改不了）。禁止再用"每深一层乘 0.6"
//     这种写死系数，也禁止在子层再存一份用户缩放倍数。
//     子 Scene 在画面上的有效比例 = `fitScale * inheritedScale`（inheritedScale 是父层
//     的 viewportScaleValue()），偏移同理要乘 —— 否则相机放大时只有外层圆壳变大，
//     圆里的节点不跟着放大（#818 复审）。fitScale 本身存的是局部数据，
//     fitView / syncFitToSceneSize 都必须先除掉 inheritedScale 还原局部尺寸再算。
// 14. 选中态全树只有一份（StarMapSelectionState），身份是 scenePath + kind + itemId。
//     每层各存一份 selectedNodeId 的话，父子两层会同时亮，且分不清选中的是哪一层的哪个东西。
// 15. 选中、connect 松手、菜单归属全部走 geometry 的递归命中一个入口，
//     不再各写一份"点在不在这一层"的判断；双指缩放不走它（#818：相机只有一台）。
// 16. 双指 > 所有单指（#818 复审）：Pinch 一旦认领，`beginPinch()` 会原子接管整个手势状态，
//     并收掉单指留下的业务现场（panActive / 菜单 / 创建菜单 / 连线预览线）；
//     所有单指回调入口统一先 `if (this.gestureTracker.isPinching()) return`，
//     否则晚到的单指回调会重新选中对象、弹菜单或起连线。
// 17. 子星图**不是独立窗口**（#818 复审），没有自己的平移相机。任意一层空白处的
//     普通单指拖动都只改根全局相机：child Scene 把目标 offset 通过 onCameraPanTo 回传给根。
//     fitOffsetX/Y 只允许由 fitView / syncFitToSceneSize 写，不进任何用户交互写路径。
//     之前 child 能自己写 fitOffset，拖出去的内容又被 clip(true) 裁掉，真机上表现为
//     "子星图里的东西突然看不见"。
//     回传的是**目标值**不是增量：PanGesture 的 event.offsetX/Y 是相对手势起点的累计位移，
//     起手时记一份 panBaseCameraOffsetX/Y，更新时算 基准 + 累计值，根整份覆盖相机 offset；
//     写成 `camera += event.offsetX` 会把同一手势的每一帧叠加一遍，越拖越远。
// 18. 子星图里的对象不能被拖到圆外（#818 复审）：sceneDepth > 0 时 moveNode / moveEmbed
//     的候选坐标先过 `clampItemToEmbedSafeArea()`（复用 CIRCLE_INNER_SAFE_RATIO），
//     再进 applyFreeformLayout、再存进 Core —— 存进数据的也必须是夹好的那份坐标，
//     不能只在显示层夹住然后靠 clip 藏错误。根层对象仍然自由移动。
// 19. 新建也要守边界（#818 复审）：performCreateNode / performCreateEmbed 写 Core 之前
//     先过 `clampItemToLocalSafeArea()`（尺寸取 StarMapLayout 导出的
//     DEFAULT_NODE_WIDTH/HEIGHT、DEFAULT_EMBED_DIAMETER，UI 侧不手写）。
//     clamp 之外还必须 `refitAfterContentAdded()`：空子星图首次 fitView() 时没有内容，
//     fitScale 会停在 1 且 hasFittedView 已为 true，之后第一次新建时 maybeFitView 不再跑，
//     新对象会比当前安全区还大。重算的是 local fit，不是 global camera，
//     所以不会把用户双指缩放好的视角打回初始大小。
//     移动已有对象不重算，继续每帧 clamp 即可。
// 20. 语义缩放按**实际视口**算，不按"第几层"算（#820）：档位判定全在纯函数模块
//     StarMapVisualLod 里，输入只有"根视口宽高 + 本 Scene 累计有效比例 + 焦点"，
//     判据是圆内还剩多少屏幕空间（放得下一个节点就展开），所以小窗自动少展开几层、
//     大窗多展开几层，同一个设备改窗口大小就地重算。禁止写死"显示两层"这种常量。
//     根视口宽高由 StarMapScreen 从 onAreaChange 一路透传下来，子 Scene 不许拿自己
//     那个圆当根视口（那会让"够不够展开"变成自我实现）。
// 21. 三档渲染（#820）：expanded 才实例化 StarMapEmbedScene；collapsed 只画节点尺寸的
//     摘要卡（标题 + 子星图身份 + 选中态），整块热区归这个 Embed、不往内递归；
//     hidden 连摘要卡都不画。折叠态不许 opacity(0) 假装折叠，也不许"先建好再缩"。
//     LOD 层级按 scenePath 判，不按 sceneDepth——递归链里一层 Embed 要加两次 sceneDepth。
// 22. LOD 必须同时改**布局矩形**（buildEmbedLayoutNodes 吃档位映射）和渲染，
//     否则会出现"画的是 160×80 摘要卡、连线端点和命中还按 200×200 的圆算"。
//     形状由 geometry 的 isCircularDisplayRect 从同一份显示边界读出，不另开参数。
//     档位切换只改派生矩形，绝不因为用户捏合就回写 Core 的 authored position。
// 23. 焦点链比对的路径必须是**这颗 Embed 进入之后**的 childScenePath
//     （embedScenePathLabel(instanceId)），不能传本 Scene 的 scenePath：
//     传父 Scene 的话焦点 = root/embed:a 时 a/b/c 全被判成"在焦点链上"强制展开，
//     而初始 focusScenePath = root 更会把根层所有 Embed 全部强制展开，根层 LOD 失效（#820 复核）。
// 24. 展开尺寸必须是同一份显示真相（#820 复核）：`expandedEmbedSizeVp()` 读当前视口的
//     显示预算，LOD 判定、显示边界、布局矩形、渲染盒子全部用它。写死 DEFAULT_EMBED_DIAMETER
//     会让视口只影响 coverage 不影响 LOD，"大屏多展开几层"永远实现不了。
// 25. Embed 的显示矩形以 authored 中心为锚点派生，展开 / 折叠 / 视口 resize 只改尺寸不挪位置；
//     移动保存时用 embedAuthoredPositionFromDisplay 把显示左上角反解回 authored 口径再写 Core。

import hilog from '@ohos.hilog'
import { ComponentContent } from '@kit.ArkUI'
import { CreateTitleDialogArgs, createTitleDialogContent } from './CreateTitleDialog'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import {
  StarMapGraph, StarMapNode, StarMapEdge, StarMapEmbed,
  StarMapTargetPath, StarMapPoint, StarMapPathSegment
} from '../../../corebridge/dto/StarMapDtos'
import { ThemeConfig } from '../../../ui/theme/HarmonyThemeAdapter'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import {
  PlatformNodeRect, PlatformEdgeRender,
  RecursiveSceneContext, RecursiveSceneSource, RecursiveHit,
  deriveNodeRectFromLayoutNode,
  computeAllEdgeRenders,
  screenToCanvas, canvasToScreen, resolveEdgeEndpointAnchor,
  edgeEndpointBoundaryPoint, buildCircleResponseBands,
  buildRecursiveSceneContext, resolveRecursiveHit, convertPointToRoot,
  buildTargetPathForHost, planCrossLayerEdge, describeScenePath,
  embedHitMetricsForScene, describeEmbedInputNodeId, describeSceneInputNodeId,
  extractEmbedInstanceIdFromInputNodeId,
  EMBED_TITLE_HIT_HEIGHT, EMBED_BORDER_HIT_WIDTH, EmbedHitMetrics
} from '../platform/StarMapGeometry'
import {
  PlatformLayoutNode, PlatformPoint, EmbedDisplayBounds,
  buildFreeformLayout, buildEmbedLayoutNodes, applyFreeformLayout,
  embedAuthoredPositionFromDisplay,
  DEFAULT_EMBED_DIAMETER, DEFAULT_NODE_WIDTH, DEFAULT_NODE_HEIGHT, DEFAULT_NODE_RADIUS
} from '../platform/StarMapLayout'
import {
  PlatformMotionConfig,
  wobbleXFromProgress, wobbleYFromProgress,
  MotionPauseTracker
} from '../platform/StarMapMotion'
import { GestureState, StarMapGestureMode, StarMapGestureStateTracker } from '../platform/StarMapGestureState'
import { StarMapSelectionState } from '../platform/StarMapSelectionState'
import {
  StarMapSceneNode, StarMapSceneRegistry, Viewport,
  ContentBounds, computeContentBounds, computeFittedViewport, computeCenteredOffset,
  computeCameraPinch, GlobalCameraTransform,
  clampItemToEmbedSafeArea, computeEmbedInnerContentSafeSide, CIRCLE_INNER_SAFE_RATIO,
  projectLocalLengthToScreen
} from '../platform/StarMapViewport'
import {
  EmbedLodMode, VisualLodContext,
  projectEmbedMetrics, resolveEmbedLodForScene, embedDisplayBoundsForLod, expandedEmbedSizeVp
} from '../platform/StarMapVisualLod'
import {
  logPinchBegin, logPinchEnd, logRecursiveHit, logViewportState, logCreateResult,
  logSelectionChanged, logConnectBegin, logConnectEnd
} from '../platform/StarMapDiagnostics'
import { StarMapEmbedScene } from './StarMapEmbedScene'

/** wobble 视觉偏移（dx, dy）：只影响显示，不写回 Core。 */
interface WobbleOffset {
  dx: number
  dy: number
}



// ─── Embed 命中区尺寸 ───
// EMBED_TITLE_HIT_HEIGHT / EMBED_BORDER_HIT_WIDTH 直接 import 自 StarMapGeometry，
// 不在本文件再抄一份。命中测试和真实触摸必须用同一份数值：曾经几何用 3vp、
// UI 用 4vp，于是手指按这一圈会判成外层边框，而把线拖到同一点松手却判成子图内部。

/**
 * 星图选中描边色。普通节点和 Embed 共用这一个值。
 * 不能用 themeConfig.colors.primary：主题主色跟着配色走，深色主题下可能和
 * 子星图外壳底色接近，选中描边会糊成"边贴边"，选中态直接消失（Issue #813 复审、#818）。
 */
const STARMAP_SELECTED_STROKE_COLOR: string = '#FF6B6B'

// ─── 视口常量 ───

/** 子图内容适配到 Embed 圆形可用区时，四边留白（vp）。 */
const EMBED_FIT_PADDING_VP: number = 8

// 缩放锚点不再自己算：改用高优先级 PinchGesture 事件里的 pinchCenterX / pinchCenterY，
// 它是双指中点在根组件坐标系里的位置（#818 复审）。

@Component
export struct StarMapScene {
  // ─── @Prop：从父组件只读传入 ───
  /** 根星图 ID（整个 StarMapScreen 生命周期内不变） */
  @Prop rootStarmapId: string = ''
  /** 当前 scene 的路径段（根 scene 为空数组） */
  @Prop scenePath: StarMapPathSegment[] = []
  /** 当前 scene 对应的星图 ID */
  @Prop starmapId: string = ''
  /** 当前 scene 的深度（根为 0） */
  @Prop sceneDepth: number = 0
  /**
   * 根 StarMap 实际可绘制内容区宽度（vp），整棵递归树共用同一份（#820）。
   *
   * LOD 判定必须用**根视口**：子 Scene 拿自己那个圆当视口，
   * "这层还有多少展开空间"就变成了自我实现的死循环（自己算自己，永远不够）。
   */
  @Prop @Watch('onRootViewportChange') rootViewportWidth: number = 0
  /** 根 StarMap 实际可绘制内容区高度（vp），语义同 rootViewportWidth（#820）。 */
  @Prop @Watch('onRootViewportChange') rootViewportHeight: number = 0
  /**
   * 当前视觉焦点的 scenePath 描述（根为 'root'）（#820）。
   *
   * 焦点只决定"哪些 Embed 保持展开"，不新建页面、不换相机、不落盘。
   * 焦点链上的 Embed 强制展开，否则用户捏进去的子星图会因为"里面不够大"自己折起来。
   */
  @Prop @Watch('onFocusScenePathChange') focusScenePath: string = 'root'
  /** 主题配置 */
  @Prop themeConfig: ThemeConfig = {
    mode: 'light',
    colorSource: 'built_in',
    selectedPaletteId: '',
    selectedBuiltinThemeId: '',
    colors: {
      primary: '#000000', onPrimary: '#FFFFFF',
      primaryContainer: '#E0E0E0', onPrimaryContainer: '#1A1C1E',
      secondary: '#51606F', onSecondary: '#FFFFFF',
      secondaryContainer: '#D5E2F0', onSecondaryContainer: '#0D1E2C',
      tertiary: '#695779', onTertiary: '#FFFFFF',
      tertiaryContainer: '#F0DBFF', onTertiaryContainer: '#241533',
      error: '#BA1A1A', onError: '#FFFFFF',
      errorContainer: '#FFDAD6', onErrorContainer: '#410002',
      background: '#FDFCFF', onBackground: '#1A1C1E',
      surface: '#FDFCFF', onSurface: '#1A1C1E',
      surfaceVariant: '#DFE2EB', onSurfaceVariant: '#43474E',
      surfaceContainerLowest: '#FFFFFF', surfaceContainerLow: '#F7F8FA',
      surfaceContainer: '#F1F2F5', surfaceContainerHigh: '#EBEDF0',
      surfaceContainerHighest: '#E6E7EA', surfaceDim: '#DAD9DD',
      surfaceBright: '#FDFCFF',
      outline: '#73777F', outlineVariant: '#C3C7CF',
      inverseSurface: '#2F3033', inverseOnSurface: '#F1F0F4',
      inversePrimary: '#A8C8FF',
      scrim: '#000000',
      textPrimary: '#1A1C1E', textSecondary: '#43474E',
      textHint: '#73777F', success: '#2ECC71',
      divider: '#E0E0E0', overlay: '#80000000'
    },
    fontSize: 14,
    fontFamily: 'HarmonyOS Sans',
    lineHeight: 1.5
  }
  /** 运动配置 */
  @Prop motionConfig: PlatformMotionConfig = {
    enabled: true, reduceMotion: false,
    idleWobbleEnabled: true, idleAmplitudeVp: 2.0,
    idlePeriodMs: 4200, dragLiftScale: 1.04,
    settleDurationMs: 220
  }
  /** 动画进度 */
  @Prop @Watch('onMotionProgressChange') motionProgress: number = 0

  // ─── @Link：与父组件双向绑定 ───
  /** 当前星图的图数据 */
  @Link @Watch('onGraphChange') graph: StarMapGraph | null
  // ─── @Prop：全局视角相机（#818）───
  // 整棵递归星图只有一台相机，状态由 StarMapScreen 持有。
  // 子 Scene 也拿到同一份值：它们只是相机里的内容，不能有自己独立的用户缩放。
  /** 全局相机比例 */
  @Prop @Watch('onTransformChange') cameraScale: number = 1
  /** 全局相机 X 偏移（根 Scene 局部屏幕坐标） */
  @Prop @Watch('onTransformChange') cameraOffsetX: number = 0
  /** 全局相机 Y 偏移（根 Scene 局部屏幕坐标） */
  @Prop @Watch('onTransformChange') cameraOffsetY: number = 0
  /**
   * 祖先累计显示比例：父 Scene 的 viewportScaleValue()。
   *
   * 子 Scene 在屏幕上真正显示出来的比例 = `fitScale * inheritedScale`，
   * 也就是把"父层已经吃下的相机 + 父层局部适配"一起乘进来。
   * 没有这一份累计值，相机放大时只有外层圆壳变大，圆里的节点还是原尺寸，
   * 只是在一个更大的圆里重新居中——那不是缩放视角，是放大容器（#818 复审）。
   *
   * 根 Scene 不用它：根就是相机本体，比例直接是 cameraScale。
   */
  @Prop @Watch('onTransformChange') inheritedScale: number = 1

  /**
   * 改全局相机的回调。只有根 Scene 会调，一路传到 StarMapScreen 写回相机。
   * 子 Scene 没有可改的缩放，所以它们只是把 cameraScale/Offset 原样往下传。
   */
  onCameraChanged: (scale: number, offsetX: number, offsetY: number) => void =
    (_scale: number, _offsetX: number, _offsetY: number) => {}

  /**
   * 拖动画布的回调：只改根全局相机（#818 复审）。
   *
   * 子星图不是独立窗口，手指在子星图空白里拖，动的也是整张星图的视角。
   * 所以 child Scene 不写自己的 fitOffset，而是把目标 offset 交给根 Scene。
   *
   * 传的是**目标值**不是增量：PanGesture 的 event.offsetX/Y 是相对手势起点的
   * 累计位移，根 Scene 必须整份覆盖相机 offset，不能拿它再相加。
   */
  onCameraPanTo: (offsetX: number, offsetY: number) => void =
    (_offsetX: number, _offsetY: number) => {}

  /**
   * 请求根侧重算视觉焦点（#820）。
   *
   * 只有根 Scene 会在捏合手势收尾时调它；子 Scene 不调，因为焦点是全树唯一的显示态。
   * 单独留一个回调而不是让 Scene 自己改 focusScenePath：焦点住在 StarMapScreen，
   * Scene 只报告"树变了、相机稳了，该重判一次了"。
   */
  onFocusRecomputeRequested: () => void = () => {}

  /**
   * 全树唯一选中态。根 Scene 持有唯一一份，沿组件树传到每个子 Scene。
   * @ObjectLink 而不是 @State：它是共享对象，@State 会在每层各存一份拷贝。
   */
  @ObjectLink selectionState: StarMapSelectionState

  // ─── 普通属性：共享对象和回调 ───
  /** Core bridge（用于 mutation） */
  bridge: IWriterCoreBridge | null = null
  /** 手势状态追踪器（共享） */
  gestureTracker: StarMapGestureStateTracker = new StarMapGestureStateTracker()
  /**
   * 递归 Scene 注册表（共享）。统一手势入口靠它从根往下找 Scene 句柄。
   * 没有注册表（比如单测或脱离 StarMapScreen 复用）时退化成"只有本层"。
   */
  sceneRegistry: StarMapSceneRegistry | null = null
  /** 新建节点回调 */
  onCreateNode: (scenePath: StarMapPathSegment[], starmapId: string, title: string, position: StarMapPoint) => void = (_path, _id, _title, _pos) => {}
  /** 新建子星图回调 */
  onCreateEmbed: (scenePath: StarMapPathSegment[], starmapId: string, title: string, position: StarMapPoint) => void = (_path, _id, _title, _pos) => {}
  /** 新建边回调 */
  onCreateEdge: (scenePath: StarMapPathSegment[], starmapId: string, fromItemId: string, toItemId: string) => void = (_path, _id, _from, _to) => {}
  /** 移动节点回调 */
  onMoveNode: (scenePath: StarMapPathSegment[], starmapId: string, nodeId: string, position: StarMapPoint) => void = (_path, _id, _nodeId, _pos) => {}
  /** 移动 Embed 回调 */
  onMoveEmbed: (scenePath: StarMapPathSegment[], starmapId: string, embedInstanceId: string, position: StarMapPoint) => void = (_path, _id, _embedId, _pos) => {}

  // ─── @State：场景局部状态 ───
  /**
   * 局部适配比例：由本 Scene 的内容包围盒 + 父 Embed 可用区域算出来。
   * 这是**布局适配**，不是相机：用户改不了它，只有内容或容器尺寸变化会重算。
   * 根 Scene 不用它（根用的是全局相机）。
   */
  @State fitScale: number = 1
  /** 局部适配的居中偏移 X（子 Scene 用） */
  @State fitOffsetX: number = 0
  /** 局部适配的居中偏移 Y（子 Scene 用） */
  @State fitOffsetY: number = 0
  /** 平台层布局节点（普通节点） */
  @State platformLayoutNodes: PlatformLayoutNode[] = []
  /** 平台层布局节点（Embed） */
  @State embedLayoutNodes: PlatformLayoutNode[] = []
  /** 节点显示矩形列表（含 Embed） */
  @State nodeRects: PlatformNodeRect[] = []
  /** 边渲染几何列表 */
  @State localEdgeRenders: PlatformEdgeRender[] = []
  /** 当前手势模式 */
  @State gestureMode: StarMapGestureMode = 'idle'
  /** 是否显示节点菜单 */
  @State showNodeMenu: boolean = false
  /** 节点菜单对应的 nodeId */
  @State nodeMenuNodeId: string = ''
  /** 节点菜单锚点（Scene 局部屏幕坐标）：长按时手指所在处，菜单从这儿弹出 */
  @State nodeMenuAnchor: PlatformPoint = { x: 0, y: 0 }
  /** 预览连线起点 */
  @State previewLineFrom: PlatformPoint = { x: 0, y: 0 }
  /** 预览连线终点 */
  @State previewLineTo: PlatformPoint = { x: 0, y: 0 }
  /** 是否显示新建菜单 */
  @State showCreateMenu: boolean = false
  /** 新建菜单锚点（Scene 局部屏幕坐标）：空白长按时手指所在处，菜单从这儿弹出 */
  @State createMenuAnchor: PlatformPoint = { x: 0, y: 0 }
  /** 新建菜单位置（画布坐标）：新建项落在哪张星图的哪个位置 */
  @State createMenuCanvasPosition: PlatformPoint = { x: 0, y: 0 }
  /** Canvas 是否就绪 */
  @State canvasReady: boolean = false
  /** 预览 Canvas 是否就绪 */
  @State previewCanvasReady: boolean = false
  /** Canvas 是否激活 */
  @State canvasActive: boolean = false

  // ─── 私有状态（非响应式） ───
  private motionPauseTracker: MotionPauseTracker = new MotionPauseTracker()
  private moveNodeStartX: number = 0
  private moveNodeStartY: number = 0
  private moveEmbedStartX: number = 0
  private moveEmbedStartY: number = 0
  private panActive: boolean = false
  /**
   * 本次"拖画布"起手时的全局相机 offset。
   *
   * ArkUI 的 PanGesture event.offsetX/Y 是相对手势起点的累计位移，不是每帧增量，
   * 所以必须留一份基准：更新时算 基准 + 累计位移 得到目标 offset，整份交给根。
   * 直接 `camera += event.offsetX` 会把同一次手势里的每一帧都叠加一遍，越拖越远（#818 复审）。
   */
  private panBaseCameraOffsetX: number = 0
  private panBaseCameraOffsetY: number = 0
  /** 本 Scene 自己的区域尺寸（vp），用于算子图适配比例 */
  private sceneWidth: number = 0
  private sceneHeight: number = 0
  /**
   * 是否已经算过一次 fit view。
   * 首次加载后不自动重算；只有容器局部尺寸变化、或子层新建内容时显式 fitView() 才会重算（#816 / #818 复审）。
   * 移动已有对象不走这里——移动每帧 clamp 就够了，不需要整体重新适配。
   */
  private hasFittedView: boolean = false
  // 上次适配时 Scene 的宽度，用来判断容器尺寸变化是不是父层缩放带来的
  private lastFittedSceneSize: number = 0
  /** 注册到 sceneRegistry 的本层句柄 */
  private sceneHandle: StarMapSceneNode | null = null

  // ─── 全局相机双指缩放状态（只在根 Scene 上被改） ───
  /** 两指抬起到只剩 0 指 */
  private pinchWasActive: boolean = false
  /** 双指落下那一刻的全局相机比例（每一帧都从它算，不吃上一帧的结果） */
  private pinchBaseScale: number = 1
  /** 双指落下那一刻的全局相机偏移 */
  private pinchBaseOffsetX: number = 0
  private pinchBaseOffsetY: number = 0
  // 捏合幅度直接来自 PinchGesture 事件的 event.scale（相对手势起点的累计比例），
  // 不再自己记两指距离。之前那套 fingerMap / fingerPoints 裸触摸模拟已整体删除（#818 复审）。

  // ─── 跨层连线状态 ───
  /** connect 起点：相对根星图的完整 StarMapTargetPath（#816 跨层连线靠它） */
  private connectSourcePath: StarMapTargetPath | null = null
  /** connect 起点在本 Scene 的对象 ID（日志用） */
  private connectSourceId: string = ''
  /** connect 起点类型 */
  private connectSourceKind: string = 'node'

  // ─── 对话框宿主（窗口层级，不参与 Scene Stack 布局） ───
  /** 新建节点对话框的 ComponentContent：openCustomDialog 需要它来关闭对话框 */
  private createNodeDialogContent: ComponentContent<CreateTitleDialogArgs> | null = null
  /** 新建子星图对话框的 ComponentContent */
  private createEmbedDialogContent: ComponentContent<CreateTitleDialogArgs> | null = null

  // ─── Canvas 上下文 ───
  private canvasSettings: RenderingContextSettings = new RenderingContextSettings(true)
  private canvasCtx: CanvasRenderingContext2D = new CanvasRenderingContext2D(this.canvasSettings)
  private previewCanvasSettings: RenderingContextSettings = new RenderingContextSettings(true)
  private previewCanvasCtx: CanvasRenderingContext2D = new CanvasRenderingContext2D(this.previewCanvasSettings)

  // ─── 生命周期 ───

  aboutToAppear() {
    this.canvasActive = true
    this.recomputeLayoutAndGeometry()
    this.registerSceneHandle()
    this.maybeFitView()
  }

  aboutToDisappear() {
    this.canvasActive = false
    this.unregisterSceneHandle()
    // 树里所有 Scene 共用一个 tracker。根 Scene 整页退出可以无条件清空；
    // 子 Scene 退出（Embed 被删、childGraph 重载、loading/error 分支切换）时，
    // 只有当前手势确实属于自己这棵子树才允许清，否则会把兄弟子树或根层
    // 正在进行的手势一起清掉（#816）。
    if (this.sceneDepth === 0 || this.gestureTracker.isOwnedBySceneSubtree(this.scenePath)) {
      this.gestureTracker.reset()
    }
    this.panActive = false
    this.gestureMode = 'idle'
    this.showNodeMenu = false
    this.showCreateMenu = false
    this.nodeMenuNodeId = ''
    this.closeCreateDialogs()
  }

  // ─── 视觉 LOD（#820）───

  /**
   * 某个 Embed 进入之后的 childScenePath 描述（'root' / 'root/embed:xxx'）。
   *
   * LOD 的逻辑层级按**它**判，不按 sceneDepth：
   * 递归链是 StarMapScene → StarMapEmbedScene → StarMapScene，
   * 一层嵌套要把 sceneDepth 加两次，用它算"第几层"会把深度翻倍。
   *
   * 焦点链比对的是**这颗 Embed 自己进入后的位置**，不是它所属的父 Scene：
   * 焦点在 `root/embed:a` 时只有 a 属于焦点链，同层的 b / c 仍按真实屏幕空间
   * 正常折叠；传父 Scene 的话初始焦点 `root` 会把整层 Embed 全部强制展开，
   * 根层 LOD 直接失效（#820 复核）。
   */
  private embedScenePathLabel(embedInstanceId: string): string {
    const childSegments: StarMapPathSegment[] = [
      ...this.scenePath,
      { type: 'enterEmbed', instanceId: embedInstanceId, nodeId: null }
    ]
    return describeScenePath(childSegments)
  }

  /** 根视口 + 焦点的 LOD 上下文。整棵递归树共用同一份，子 Scene 不许用自己的圆当视口。 */
  private visualLodContext(): VisualLodContext {
    return {
      viewportWidthVp: this.rootViewportWidth,
      viewportHeightVp: this.rootViewportHeight,
      focusScenePath: this.focusScenePath
    }
  }

  /**
   * 当前 Viewport 下展开态 Embed 的真实显示边长（局部坐标）。
   *
   * 全 Scene 唯一的展开尺寸来源（#820）：LOD 判定、显示边界、布局矩形、
   * 渲染盒子全部读它。不许在渲染里再写死 DEFAULT_EMBED_DIAMETER，
   * 否则就出现"LOD 按 320 算、UI 画 200"的第二套尺寸。
   */
  private expandedEmbedSizeVp(): number {
    return expandedEmbedSizeVp(this.visualLodContext())
  }

  /**
   * 某个 Embed 在本 Scene 里的视觉档位。
   *
   * 判据只有一条：它在**屏幕上**的圆内还剩多少真实可用空间
   * （`projectEmbedMetrics` 用本 Scene 的累计比例投影），
   * 放得下一个节点（DEFAULT_NODE_WIDTH）就展开，否则折叠成节点尺寸的摘要卡。
   *
   * 视口大小通过 `expandedEmbedSizeVp()` 进入判据，所以同一个设备
   * 改窗口大小会真的换档，而不是只有 coverage 在动（#820 复核）。
   *
   * ancestorCollapsed 恒为 false：祖先一旦折叠，父层就不会再实例化本 Scene，
   * 能走到这里的 Scene 说明整条祖先链都是展开的。
   */
  private embedLod(embedInstanceId: string): EmbedLodMode {
    const metrics = projectEmbedMetrics(
      this.expandedEmbedSizeVp(),
      this.viewportScaleValue(),
      this.visualLodContext(),
      EMBED_FIT_PADDING_VP
    )
    return resolveEmbedLodForScene(
      metrics,
      this.embedScenePathLabel(embedInstanceId),
      this.focusScenePath,
      false
    )
  }

  /** 某个 Embed 当前档位对应的显示边界（展开=正圆，折叠=节点尺寸摘要卡）。 */
  private embedDisplayBounds(embedInstanceId: string): EmbedDisplayBounds {
    return embedDisplayBoundsForLod(this.embedLod(embedInstanceId), this.visualLodContext())
  }

  /** 本 Scene 全部 Embed 的档位 → 显示边界映射，喂给 buildEmbedLayoutNodes。 */
  private embedDisplayBoundsMap(): Map<string, EmbedDisplayBounds> {
    const bounds = new Map<string, EmbedDisplayBounds>()
    if (!this.graph) {
      return bounds
    }
    for (const embed of this.graph.embeds) {
      bounds.set(embed.instanceId, this.embedDisplayBounds(embed.instanceId))
    }
    return bounds
  }

  /**
   * 按当前 LOD 重建 Embed 的布局矩形。
   *
   * 必须和渲染同步：画的是摘要卡而 rect 还是 200×200 的话，
   * 连线端点、递归命中、跨层拉线都会按一个不存在的圆算（#820）。
   *
   * 只改派生出来的显示矩形，**不回写 Core 的 authored position**：
   * 语义缩放是显示层的事，用户捏合不应该顺手改掉星图的节点位置。
   */
  private refreshEmbedLodLayout(): void {
    if (!this.graph) {
      return
    }
    this.embedLayoutNodes = buildEmbedLayoutNodes(this.graph.embeds, this.embedDisplayBoundsMap())
    this.recomputeGeometry()
  }

  /**
   * 本 Scene 的累计有效比例。
   *
   * 渲染、命中、边几何全部用这个值，界面上不允许再直接读 cameraScale / fitScale 当比例用。
   *
   * 为什么子 Scene 要乘 inheritedScale：ArkUI 里子 Scene 是父 Embed 盒子的子组件，
   * 父层写死了 `expandedEmbedSizeVp() * viewportScaleValue()`，盒子已经带着祖先比例变大了。
   * 子 Scene 若只用自己的 fitScale 绘制，圆壳会跟着相机变大、圆里的节点却纹丝不动。
   * 乘上 inheritedScale 之后，整条链才是
   * `child local × child fit × parent fit × … × global camera → screen`。
   *
   * 必须是**方法**，不能是 getter：ArkUI 状态管理 V1 的 @Component 不支持 get
   * 访问器，编译器会把它当成一个从没被赋值的普通属性，于是每次读都是 undefined。
   * #816 首轮真机复测就是这么塌的 —— viewportScale 恒为 undefined，往下传出去：
   *   - syncSceneHandle() 把 undefined 塞进 StarMapSceneNode.scale，递归命中
   *     读到的比例全是 undefined，screenToCanvas 出来是 NaN，命中全部落空成 blank；
   *   - computeCameraPinch 拿到 NaN 比例，cameraOffsetX / cameraOffsetY 变成 NaN；
   *   - 空白处长按换算出来的 createMenuCanvasPosition 是 NaN，序列化成 null，
   *     Core 解析节点报 PARSE_ERROR，新建节点 / 子星图一律失败。
   * 只有 @ComponentV2 才支持 getter（HomeScreen.ets 那种写法），这里是 V1，别改回去。
   */
  private viewportScaleValue(): number {
    return this.isCameraScene() ? this.cameraScale : this.fitScale * this.parentScale()
  }

  /** 本 Scene 的累计视口偏移 X：根用全局相机，子用 `局部 fit 偏移 × 祖先累计`。 */
  private viewportOffsetX(): number {
    return this.isCameraScene() ? this.cameraOffsetX : this.fitOffsetX * this.parentScale()
  }

  /** 本 Scene 的累计视口偏移 Y：根用全局相机，子用 `局部 fit 偏移 × 祖先累计`。 */
  private viewportOffsetY(): number {
    return this.isCameraScene() ? this.cameraOffsetY : this.fitOffsetY * this.parentScale()
  }

  /** 本 Scene 是不是相机本体（全树只有根 Scene 是）。 */
  private isCameraScene(): boolean {
    return this.sceneDepth === 0
  }

  /**
   * 祖先累计比例的兜底取值。
   *
   * 0 或负数只会来自上游传错／除零，除掉它会让整棵子树的节点尺寸塌成 0、
   * 递归命中全部算成 NaN，所以这里兜回 1（等价于"没有祖先缩放"）。
   */
  private parentScale(): number {
    return this.inheritedScale > 0 ? this.inheritedScale : 1
  }

  /**
   * 本 Scene 的**局部**尺寸：扣掉祖先累计比例后的原始盒子大小。
   *
   * 局部 fit 必须拿局部尺寸算，不能拿屏幕尺寸算：屏幕尺寸本身带着相机缩放，
   * 子图如果在 camera=2 时第一次加载，算出来的 fitScale 会比 camera=1 时加载小一半，
   * 同一张子图就会因为"什么时候打开"显示成不同大小（#818 复审）。
   */
  private localSceneSize(): number {
    const parentScale: number = this.parentScale()
    return this.sceneWidth / parentScale
  }

  /**
   * 本 Scene 现在能不能改视口。
   *
   * 双指进行中时谁都别动视口：相机只有一台，双指正在改它的时候再叠一次单指平移，
   * 屏幕上就是"缩放抖一下"。
   */
  private canMutateViewport(): boolean {
    return !this.gestureTracker.isPinching()
  }

  // ─── 递归 Scene 句柄与注册表 ───

  /**
   * 构造本 Scene 在注册表里的记录。
   *
   * 记录上的几何字段是**快照**：递归命中要读 rects / scale / offset，
   * 所以每次重算几何都重新 sync 一次，否则统一手势入口拿到的是上一次布局的旧数据，
   * 会把手指判到别的对象上。
   */
  private buildSceneHandle(): StarMapSceneNode {
    if (this.sceneHandle === null) {
      this.sceneHandle = new StarMapSceneNode(
        this.scenePath,
        this.starmapId,
        (): RecursiveSceneSource[] => this.collectChildEmbeds(),
        (from: StarMapTargetPath, to: StarMapTargetPath): Promise<boolean> => {
          return this.createEdgeBetween(from, to)
        },
        (): void => {
          this.drawEdges()
        }
      )
    }
    this.sceneHandle.sync(
      this.nodeRects, this.getEmbedInstanceIds(), this.localEdgeRenders,
      this.viewportScaleValue(), this.viewportOffsetX(), this.viewportOffsetY()
    )
    return this.sceneHandle
  }

  private registerSceneHandle(): void {
    if (this.sceneRegistry === null) {
      return
    }
    this.sceneRegistry.register(this.buildSceneHandle())
  }

  private unregisterSceneHandle(): void {
    if (this.sceneRegistry !== null) {
      this.sceneRegistry.unregister(this.scenePath)
    }
    this.sceneHandle = null
  }

  /**
   * 本层所有**已加载完成**的子 Scene 句柄。
   * 子图还没加载完就没有对应项，此时递归命中会停在"点在这个 Embed 上"，
   * 行为等价于"点子星图标题选中它"，不会退化成选空白。
   */
  private collectChildEmbeds(): RecursiveSceneSource[] {
    const out: RecursiveSceneSource[] = []
    if (this.sceneRegistry === null || !this.graph) {
      return out
    }
    for (const embed of this.graph.embeds) {
      const child: StarMapSceneNode | null =
        this.sceneRegistry.findChildEmbed(this.scenePath, embed.instanceId)
      if (child !== null) {
        out.push(child)
      }
    }
    return out
  }

  /**
   * 取整棵 Scene 树的递归上下文。统一手势入口用它做命中和坐标换算。
   * 没有注册表（脱离 StarMapScreen 单独复用）时返回 null，调用方退回本层行为。
   */
  private getRootContext(): RecursiveSceneContext | null {
    if (this.sceneRegistry === null) {
      return null
    }
    const root: StarMapSceneNode | null = this.sceneRegistry.getRoot()
    if (root === null) {
      return null
    }
    return buildRecursiveSceneContext(root)
  }

  // ─── 视口适配（fit view） ───

  /**
   * 首次布局完成时算一次 fit view。
   *
   * 只在"还没有算过"时算：移动已有对象不重算（每帧 clamp 就够了）。
   * 子层新建内容会走 refitAfterContentAdded() 显式重算一次——那是 local fit，
   * 和用户双指改的 global camera 是两回事，不会把用户缩放好的视角打回初始大小（#816 / #818 复审）。
   * 根 Scene 恒不自动适配（整屏显示，行为与之前一致）。
   */
  private maybeFitView(): void {
    if (this.hasFittedView) {
      return
    }
    // 根 Scene 不自动适配：它本来就整屏显示，工具栏 +/- 直接作用在它身上，
    // 自动适配会改掉这层一直以来的初始视图。
    if (this.sceneDepth === 0) {
      return
    }
    if (this.sceneWidth <= 0 || this.sceneHeight <= 0 || !this.graph) {
      return
    }
    this.fitView()
  }

  /**
   * 按当前内容重新适配视口（首次加载 / 显式 fit view）。
   *
   * 可用区域取父 Embed 圆形内容区里"手能可靠点到"的内接正方形边长：
   * 子 Scene 的布局是正方形，四角会超出圆被父层 clip 裁掉，
   * 所以按 1/√2 折算，内容不会铺到圆外被切掉。
   *
   * 这里算的是**局部** fit：先把 `sceneWidth / sceneHeight` 除掉 inheritedScale
   * 还原成本层的原始盒子尺寸，再用它算比例和居中偏移。
   * fitScale / fitOffset 一旦写成局部数据，就与"打开这张子图时相机是多少"无关，
   * 屏幕上的呈现由 `viewportScaleValue() = fitScale * inheritedScale` 统一乘出来（#818 复审）。
   */
  fitView(): void {
    this.hasFittedView = true
    const localWidth: number = this.localSceneSize()
    const localHeight: number = this.sceneHeight / this.parentScale()
    const bounds: ContentBounds | null = computeContentBounds(this.nodeRects)
    if (bounds === null) {
      this.fitScale = 1
      this.fitOffsetX = 0
      this.fitOffsetY = 0
      this.lastFittedSceneSize = localWidth
      this.syncSceneHandle()
      return
    }
    // 可用区 = 圆的内接正方形先扣掉父 Embed 的标题 / 边框交互壳，再扣留白，
    // 与 clampItemToLocalSafeArea 走同一个函数（#818 复审）：
    // 只用内接正方形的话内容会一直顶到圆边，子子星图在父圆里占得过满，
    // 父圆的"点标题 / 边框选中"入口也会被内容压住。
    const available: number = computeEmbedInnerContentSafeSide(
      Math.min(localWidth, localHeight), EMBED_FIT_PADDING_VP
    )
    // 比例按可用区算，偏移按 Scene 自己的尺寸算：
    // 子 Scene 组件是铺满整个圆的，中心在局部宽度/2、局部高度/2。
    // 拿内接正方形的左上角当原点会让内容整体偏左上（#816 复审第 4 条）。
    // available 里已经扣过 EMBED_FIT_PADDING_VP，这里再传一次就会重复扣，
    // 所以 padding 传 0：留白只在 computeEmbedInnerContentSafeSide 里扣一次。
    const fitted: Viewport = computeFittedViewport(
      bounds, available, available, 0, localWidth, localHeight
    )
    this.fitScale = fitted.zoomScale
    this.fitOffsetX = fitted.offsetX
    this.fitOffsetY = fitted.offsetY
    this.lastFittedSceneSize = localWidth
    // fitScale 变了，递归命中/坐标换算读的就是这份快照，必须立刻同步
    this.syncSceneHandle()
    // 本 Scene 的累计比例变了 → 它里面的 Embed 该展开还是该折叠要重算（#820）
    this.refreshEmbedLodLayout()
  }

  /**
   * 局部容器尺寸变化时重新居中（#818）。
   *
   * 判断依据是**局部**尺寸 `sceneWidth / inheritedScale`，不是屏幕上的 sceneWidth。
   * 相机从 1 放到 2 时：sceneWidth 从 200 变 400、inheritedScale 从 1 变 2，
   * 局部尺寸还是 200 → 局部 fit 什么都不用改，屏幕上的呈现由累计比例自然放大。
   * 只有父 Embed 的基础尺寸真的变了，局部尺寸才会变，这时才重算一次局部居中。
   *
   * 也不按屏幕尺寸比去缩放 fitScale：那样等于把相机的缩放抵消掉，
   * 用户双指放大整棵树时就会出现"外层圆变大、里面内容纹丝不动"。
   */
  private syncFitToSceneSize(): void {
    if (!this.hasFittedView || this.lastFittedSceneSize <= 0) {
      return
    }
    const localWidth: number = this.localSceneSize()
    if (Math.abs(localWidth - this.lastFittedSceneSize) <= 0.5) {
      return
    }
    this.lastFittedSceneSize = localWidth
    const localHeight: number = this.sceneHeight / this.parentScale()
    const bounds: ContentBounds | null = computeContentBounds(this.nodeRects)
    if (bounds === null) {
      this.fitOffsetX = 0
      this.fitOffsetY = 0
    } else {
      const offset: PlatformPoint = computeCenteredOffset(
        bounds, this.fitScale, localWidth, localHeight
      )
      this.fitOffsetX = offset.x
      this.fitOffsetY = offset.y
    }
    this.syncSceneHandle()
    // 局部容器尺寸变了：本 Scene 的累计比例跟着变，Embed 的展开/折叠重算（#820）
    this.refreshEmbedLodLayout()
  }

  /**
   * 把当前视口和矩形快照写回 Scene 句柄。
   *
   * 递归命中、坐标换算全都从注册表里读 Scene 的 scale/offset，
   * 而相机/局部适配改的是 @Prop 上的 cameraScale、@State 上的 fitScale/fitOffset，
   * 两者之间没有自动联动。这里是唯一把它们对齐的地方。
   */
  private syncSceneHandle(): void {
    if (this.sceneHandle === null) {
      return
    }
    this.sceneHandle.sync(
      this.nodeRects, this.getEmbedInstanceIds(), this.localEdgeRenders,
      this.viewportScaleValue(), this.viewportOffsetX(), this.viewportOffsetY()
    )
  }

  /** 改全局相机。只有根 Scene 能调，子 Scene 没有独立相机可改（#818）。 */
  private applyCameraTransform(scale: number, offsetX: number, offsetY: number): void {
    if (!this.isCameraScene()) {
      return
    }
    this.onCameraChanged(scale, offsetX, offsetY)
  }

  /**
   * 拖动画布 = 拖全局相机，任意一层都是（#818 复审）。
   *
   * 根 Scene 直接改全局相机；子 Scene 自己没有相机，把目标 offset 原样回传给根。
   * 子星图是父星图里的一个嵌入区域，不是独立窗口——手指在子图空白里拖，
   * 动的同样是整张星图的视角，所以这里不存在"改 child fitOffset"这条路径。
   *
   * 注意这里是"目标值"而不是"每帧增量"：ArkUI 的 PanGesture event.offsetX/Y
   * 是相对手势起点的累计位移，同一次手势里每帧都会重发从起点算起的总值
   * （华为官方示例也是 `offsetX = positionX + event.offsetX`）。
   * 如果这里写成 `camera += event.offsetX`，拖得越久累计得越离谱。
   * 正确写法是起手时记一份本次 Pan 的基准（panBaseCameraOffsetX/Y），
   * 更新时用 基准 + 累计位移 得到目标 offset。
   */
  private applyCameraPanTo(offsetX: number, offsetY: number): void {
    if (this.isCameraScene()) {
      this.applyCameraTransform(this.cameraScale, offsetX, offsetY)
      return
    }
    this.onCameraPanTo(offsetX, offsetY)
  }

  /**
   * 把一个 item 的候选坐标限制在本 Scene 的安全区内（#818 复审）。
   *
   * 根 Scene 的画布是整屏，没有圆壳，不需要限制。
   * 子 Scene 被父 Embed 的圆形外壳包着，超出内接正方形的部分会被 clip(true) 裁掉，
   * 真机上就表现为"节点/子星图一拖就凭空消失"。所以写入前先夹一次，
   * 夹完的结果既进 applyFreeformLayout 也存进 Core，数据里不留圆外坐标。
   */
  private clampItemToLocalSafeArea(x: number, y: number, width: number, height: number): PlatformPoint {
    if (this.isCameraScene()) {
      return { x: x, y: y }
    }
    const parentScale: number = this.parentScale()
    const localWidth: number = this.sceneWidth / parentScale
    const localHeight: number = this.sceneHeight / parentScale
    // 圆壳直径取局部宽高中较小的一边：Embed 组件是等宽高的正圆
    const localSceneSize: number = Math.min(localWidth, localHeight)
    return clampItemToEmbedSafeArea(
      x, y, width, height,
      this.fitScale, this.fitOffsetX, this.fitOffsetY,
      localSceneSize, EMBED_FIT_PADDING_VP
    )
  }

  /**
   * 某个 Embed 壳在这张图里的稳定输入节点 id（#818 复审）。
   *
   * id 的生成规则在 StarMapGeometry.describeEmbedInputNodeId（带 scenePath，
   * 因为同一个 instanceId 会在每一层父图里各出现一次），这里只做转发，
   * 保证 StarMapEmbedScene 那边报出来的 id 和这里设上去的完全一致。
   */
  private embedInputNodeId(embedInstanceId: string): string {
    return describeEmbedInputNodeId(this.scenePath, embedInstanceId)
  }

  /**
   * 本 Scene 自己的稳定输入节点 id（#818 复审）。
   *
   * 根 Stack 与子 Stack 都要有：ArkUI 的 onChildTouchTest 只会把**显式设了 .id() 的
   * 命名节点**放进回调参数，父 Scene 想指名把事件发给某个 child Scene，就必须先能
   * 按 id 认出它。不给 id 就只能靠 Stack 的默认命中顺序，嵌套一深就分不清是哪一层。
   *
   * 注意它和父层那颗 Embed 内壳的 id **不是同一个字符串**（#818 复审）：
   *   父层 Embed 内壳：starmap_embed_input_root_<instanceId>
   *   本 Scene 壳：    starmap_embed_input_root/embed:<instanceId>
   * 真正需要的是"稳定且在整棵递归树里不重名"，不是两端字符串相同；
   * 父层指名转发时用的是前者，两者靠各自前缀在同一 scenePath 段里唯一来区分。
   */
  private sceneInputNodeId(): string {
    return describeSceneInputNodeId(this.scenePath)
  }

  /**
   * 这颗 Embed 的内部区域是否应该继续下探到最深 child Scene（#818 复审）。
   *
   * #373 的统一输入规则在这一层落地：
   * - 1 指命中**标题带 / 圆环** → 不往下走，这一指属于"选中这个子星图"，归父 Scene；
   * - 1 指命中**内部** → 继续交给最深的 child Scene；
   * - 2 指（不论落在哪一层）→ 根 Scene 那唯一一份 PinchGesture 认领，改全局相机。
   *
   * **坐标口径（这里最容易写错，#818 复审专门盯这一点）**：
   * 这个回调挂在**单颗 Embed 自己的外层 Stack** 上，它给出的 `children` 是
   * Stack 里面那些命名子节点（也就是包住 child Scene 的那根 innerContent Column）。
   * ArkUI 对 TouchTestInfo 的定义是：
   * - `x / y` —— 触点相对**这个子组件**的左上角；
   * - `parentX / parentY` —— 触点相对**父组件**的左上角，而父组件就是这颗已经
   *   `.position(...)` 过的 Embed Stack。
   *
   * 所以这里**只能用 `child.x / child.y`**，它们已经是相对 Embed 壳内部的坐标。
   * 之前那版又减了一遍 `getEmbedScreenX/Y()`（父 Scene 里的 Embed 原点），
   * 等于把原点扣了两次：Embed 摆在父 Scene 的 (300,200)、手指点在圆心时，
   * parentX/parentY 是 (100,100)，再减一次就变成 (-200,-100)，圆命中直接失败，
   * 于是大部分子星图位置上 FORWARD_COMPETITION 根本走不到（#818 复审）。
   *
   * 圆半径同样直接取 `child.rect`：这个矩形就是这块 innerContent 的实际大小，
   * 不需要再拿 `expandedEmbedSizeVp() * viewportScaleValue()` 反推。
   *
   * 标题带与圆环的判定必须和 resolveRecursiveHit 完全一致，所以同样调
   * embedHitMetricsForScene，不在这里再抄一份 24 / 12。
   *
   * @param child 触摸测试命中的那一个 Embed 内壳节点（ArkUI 只收命名节点）
   * @returns 命中内部就 true（可以下探），标题带 / 圆环 / 圆外都 false
   */
  private shouldForwardTouchToChild(child: TouchTestInfo): boolean {
    if (child === undefined || child === null || child.rect === undefined || child.rect === null) {
      return false
    }
    const rectWidth: number = child.rect.width
    const rectHeight: number = child.rect.height
    if (!Number.isFinite(rectWidth) || !Number.isFinite(rectHeight) ||
      rectWidth <= 0 || rectHeight <= 0) {
      return false
    }
    const radius: number = Math.min(rectWidth, rectHeight) / 2
    // x / y 已经相对这个子组件左上角，直接就是 Embed 壳内部的局部坐标
    const localX: number = child.x
    const localY: number = child.y
    if (!Number.isFinite(localX) || !Number.isFinite(localY)) {
      return false
    }
    const metrics: EmbedHitMetrics = embedHitMetricsForScene(1, radius)
    const dx: number = localX - radius
    const dy: number = localY - radius
    const dist: number = Math.sqrt(dx * dx + dy * dy)
    if (dist > radius) {
      // 圆外：事件落回父星图
      return false
    }
    if (localY <= metrics.titleHitHeight) {
      // 标题带 = 选中这个子星图，不下沉
      return false
    }
    if (dist >= radius - metrics.borderHitWidth) {
      // 圆环 = 选中这个子星图，不下沉
      return false
    }
    return true
  }

  /**
   * onChildTouchTest 的统一入口（#818 复审）。
   *
   * 这里**只做事件分发**，不新增任何手势识别器：双指仍然只有根 Scene 那一份
   * PinchGesture。FORWARD_COMPETITION 的含义是"把事件投给这个被命中的子节点，
   * 同时系统仍决定要不要投给别的兄弟"，所以根的两指识别链不会被深层 touch-test 截断，
   * 同时 child 的单指手势也仍然留在竞争里。
   *
   * 明确不用 DEFAULT：DEFAULT 等于"自定义分发不生效、按命中状态由系统派"，
   * 嵌套一深就回到"被深层 touch-test 分流掉、触点回不到根 Pinch"的老问题。
   */
  private routeChildTouchTest(children: TouchTestInfo[]): TouchResult {
    if (children === undefined || children === null || children.length === 0) {
      return { strategy: TouchTestStrategy.DEFAULT }
    }
    for (const child of children) {
      if (child === undefined || child === null) {
        continue
      }
      // instanceId 只用来确认"这是当前 Scene 自己的 Embed 内壳节点"，
      // 不拿它去算坐标：坐标一律用 child.x / child.y（相对子组件原点）。
      const instanceId: string | null = extractEmbedInstanceIdFromInputNodeId(this.scenePath, child.id)
      if (instanceId === null) {
        continue
      }
      if (this.shouldForwardTouchToChild(child)) {
        return { strategy: TouchTestStrategy.FORWARD_COMPETITION, id: child.id }
      }
    }
    return { strategy: TouchTestStrategy.DEFAULT }
  }

  /**
 * 子 Scene 新增内容后重算一次 local fit（#818 复审）。
 *
 * 为什么只有 clamp 还不够：空子星图首次 fitView() 时没有内容，fitScale 会停在 1
 * 且 hasFittedView 已经是 true，之后第一次新建节点 / 二级子星图时
 * maybeFitView() 不会再跑，于是新对象可能比当前安全区还大。
 * 这里显式重算一次，把整张子内容重新塞回父 Embed 的安全区：
 *
 * - 空子星图第一次建节点 → local fit 缩小，节点不会被裁；
 * - 空子星图第一次建二级子星图 → 显示尺寸严格小于父 Embed；
 * - 已有内容再加 → 必要时整体缩小。
 *
 * 根 Scene 不需要：它的画布是整屏，没有圆壳，也没有 local fit。
 * 全局相机完全不受影响——用户双指改的是 global camera，这里改的是 local fit，
 * 两者不是一回事，所以不存在"重新 fit 会把用户缩放重置掉"的顾虑。
 */
private refitAfterContentAdded(): void {
    if (!this.isCameraScene()) {
      this.fitView()
    }
  }

  /**
   * 取一个 item 在画布坐标下的显示尺寸。
   *
   * 做边界限制时需要 item 的完整矩形（夹的是显示矩形，不是左上角坐标），
   * 查不到布局节点时退回调用方给的默认值。
   */
  private itemCanvasSize(itemId: string, fallbackWidth: number, fallbackHeight: number): PlatformPoint {
    const node: PlatformLayoutNode | null | undefined = this.getPlatformLayoutNode(itemId)
    if (node === null || node === undefined) {
      return { x: fallbackWidth, y: fallbackHeight }
    }
    return { x: node.width, y: node.height }
  }

  // ─── 高优先级双指缩放入口 = 全局相机（只在根 Scene 上生效，#818） ───

  /**
   * Pinch 认领：记下全局相机起点，并**原子接管**之前单指留下的业务状态（#818 复审）。
   *
   * 真机上第一根手指落下时，节点 / Embed / 空白各自的 Tap、LongPress、Pan 就已经
   * 在竞争了，第二根手指才让 Pinch 认领——也就是说我们接手时现场可能已经有：
   * 选中态、节点菜单、创建菜单、连线的预览线、甚至正在拖的画布。这些必须一起收掉，
   * 否则用户会看到"捏合的同时弹了个菜单""捏着捏着连上了线"。
   *
   * 之后所有单指回调入口都有 `if (this.gestureTracker.isPinching()) return` 兜底，
   * 而 tracker 的单指 begin* 也拒绝在 pinch 期间改写 mode，双向都堵住。
   */
  private beginPriorityPinch(event: GestureEvent): void {
    if (!this.isCameraScene()) {
      return
    }
    const centerX: number = event.pinchCenterX
    const centerY: number = event.pinchCenterY
    this.pinchWasActive = true
    this.pinchBaseScale = this.cameraScale > 0 ? this.cameraScale : 1
    this.pinchBaseOffsetX = this.cameraOffsetX
    this.pinchBaseOffsetY = this.cameraOffsetY
    // beginPinch 会重建整个 GestureState，原子覆盖掉当时的 pan / 菜单 / 连线 / 移动
    this.gestureTracker.beginPinch(centerX, centerY)
    this.closeSupersededConnectIfNeeded()
    this.clearConnectVisualState()
    this.motionPauseTracker.clear()
    this.panActive = false
    this.showNodeMenu = false
    this.showCreateMenu = false
    this.nodeMenuNodeId = ''
    logViewportState(this.pinchBaseScale, this.pinchBaseOffsetX, this.pinchBaseOffsetY)
    logPinchBegin(centerX, centerY, this.pinchBaseScale)
  }

  /**
   * Pinch 移动：改全局相机。
   *
   * `event.scale` 是相对本次手势起点的累计比例（1.0 = 两指刚落下），
   * 每一帧都拿它和 `pinchBaseScale` 重算，而不是在上一帧的结果上继续叠。
   * 锚点是 `event.pinchCenterX / pinchCenterY`，也就是双指中点在根组件坐标系里的位置。
   */
  private updatePriorityPinch(event: GestureEvent): void {
    if (!this.isCameraScene() || !this.pinchWasActive) {
      return
    }
    const ratio: number = event.scale
    if (!Number.isFinite(ratio) || ratio <= 0) {
      return
    }
    const next: GlobalCameraTransform = computeCameraPinch(
      {
        scale: this.pinchBaseScale,
        offsetX: this.pinchBaseOffsetX,
        offsetY: this.pinchBaseOffsetY
      },
      this.pinchBaseScale,
      1,
      ratio,
      event.pinchCenterX,
      event.pinchCenterY
    )
    this.applyCameraTransform(next.scale, next.offsetX, next.offsetY)
  }

  /** 双指抬起：清掉手势状态，避免下次沿用上一次的比例基准。 */
  private endUnifiedPinch(): void {
    if (!this.pinchWasActive) {
      return
    }
    logPinchEnd(this.cameraScale)
    this.pinchWasActive = false
    this.gestureTracker.endPinch()
    // 捏合收尾：投影比例稳下来了，再判一次视觉焦点（#820）。
    // 捏合过程中每帧都在改相机，但子 Scene 可能正在按新比例重建实例，
    // 手势结束那一刻注册表里的树才和最终相机一致。
    this.onFocusRecomputeRequested()
  }

  /** 按绝对路径查 Scene 句柄；查不到退回本层句柄。 */
  private findSceneHandle(scenePath: StarMapPathSegment[]): StarMapSceneNode | null {
    if (this.sceneRegistry === null) {
      return this.buildSceneHandle()
    }
    const found: StarMapSceneNode | null = this.sceneRegistry.find(scenePath)
    return found !== null ? found : this.buildSceneHandle()
  }

  // ─── 选中态 ───

  private isNodeSelected(nodeId: string): boolean {
    return this.selectionState.isSelected(this.scenePath, 'node', nodeId)
  }

  private isEmbedSelected(instanceId: string): boolean {
    return this.selectionState.isSelected(this.scenePath, 'embed', instanceId)
  }

  /**
   * 选中一个对象。
   *
   * 全树只有一份状态，且身份是 scenePath + kind + itemId：
   * 选子图里的节点会立刻让父层那个 Embed 取消选中（因为 scenePath 不同），
   * 选二级子星图的标题也只会选中那一个 Embed。
   */
  private selectItem(kind: 'node' | 'embed' | 'edge', itemId: string): void {
    logSelectionChanged(describeScenePath(this.scenePath), kind, itemId)
    this.selectionState.select(this.scenePath, kind, itemId)
    this.redrawAllSceneEdges()
  }

  /**
   * 共享选中一变就让**每一层**重画边。
   *
   * 边的高亮是 drawEdges 里读 selectionState 画的，而 ArkUI 的 Canvas onReady 只在
   * 首次准备好时触发一次，外部状态变化之后不会自动重放之前的绘图命令。原来能触发
   * 重画的只有视口变化、动画帧、图更新和首次 onReady —— 选中边以后如果动画是关的
   * （reduce motion / shouldAnimate 为 false），线可以一直停在旧颜色旧线宽，直到下一次
   * 缩放或平移才被改对。
   * 更要紧的是选中是整棵树共享的一份：先选子层的边再点根层的节点，子层那条高亮也得
   * 擦掉，所以只重画当前这层不够，必须遍历注册表里所有已注册的 Scene。
   */
  private redrawAllSceneEdges(): void {
    if (this.sceneRegistry === null) {
      // 没有注册表（独立使用本组件时）只能重画自己
      this.drawEdges()
      return
    }
    this.sceneRegistry.redrawAllEdges()
  }

  /**
   * 空白层单击：选中递归命中的目标，没有目标就清掉选中。
   *
   * 这一条同时补上了 #816 两个一直没闭环的地方：
   * - 边以前有选中状态、有选中画法，但**没有任何输入入口**能把它设成选中；
   * - clearSelection() 以前定义了但全项目没有调用点，点空白不会清选中。
   *
   * 两者共用同一个递归命中：点的位置换算到根坐标，resolveRecursiveHit 返回什么
   * 就选什么（含子层里的边，scenePath 自然是子层路径），返回 null 就清掉。
   * 节点 / Embed 自己的单击不走这里——它们各自组件在 Stack 上层，事件被它们收走，
   * 根本到不了背景层。
   */
  private handleBlankTap(screenX: number, screenY: number) {
    // 双指已接管时单击不能再改选中态（#818 复审）
    if (this.gestureTracker.isPinching()) {
      return
    }
    const context: RecursiveSceneContext | null = this.getRootContext()
    if (context === null) {
      return
    }
    const rootPoint: PlatformPoint = convertPointToRoot(context, this.scenePath, screenX, screenY)
    const hit: RecursiveHit = resolveRecursiveHit(context, rootPoint.x, rootPoint.y)
    if (hit.target === null) {
      this.clearSelection()
      return
    }
    this.selectionState.select(hit.target.scenePath, hit.target.objectKind, hit.target.objectId)
    logSelectionChanged(
      describeScenePath(hit.target.scenePath), hit.target.objectKind, hit.target.objectId
    )
    this.redrawAllSceneEdges()
  }

  /** 点空白：清空全树选中。 */
  private clearSelection(): void {
    if (!this.selectionState.hasSelection()) {
      return
    }
    logSelectionChanged(describeScenePath(this.scenePath), 'none', '')
    this.selectionState.clear()
    this.redrawAllSceneEdges()
  }

  // ─── @Watch 处理 ───

  onTransformChange() {
    // 相机变了（#818）：递归命中和坐标换算读的是注册表里的快照，
    // 不同步的话缩放之后命中判定就一直用的是旧视口（#816 复审第 2 条）。
    this.syncSceneHandle()
    this.drawEdges()
    // 祖先累计比例变化 = 局部尺寸跟着父层重新算一次。
    // onAreaChange 和 @Watch 谁先到不保证，先按当前这份比例校一遍局部适配：
    // 局部尺寸没变就什么都不做（相机缩放的正常情况），变了才重新居中。
    this.syncFitToSceneSize()
    // 相机一变，每个 Embed 的屏幕投影就变了，展开/折叠要就地重算（#820）。
    // 这就是语义缩放：小视口 + 小相机比例 → 折起来；放大 → 自动展开。
    this.refreshEmbedLodLayout()
  }

  /**
   * 视觉焦点变了（#820）：焦点链上的 Embed 强制展开，离链的重新按屏幕空间判。
   * 焦点只改显示，不动 Core 的 StarMap / Embed 关系，也不改相机。
   */
  onFocusScenePathChange() {
    this.refreshEmbedLodLayout()
  }

  /**
   * 根视口尺寸变了（分屏、折叠屏展开、横竖屏切换）（#820）。
   *
   * 这里只重算派生显示状态：相机保持不动、focusScenePath 保持不动，
   * 不重新 fit 根图，也不把用户拉回根层——同一个星图换个窗口大小就该接着看。
   */
  onRootViewportChange() {
    this.refreshEmbedLodLayout()
  }

  onMotionProgressChange() {
    this.drawEdges()
    if (this.gestureMode === 'connect') {
      this.drawPreviewLine()
    }
  }

  onGraphChange() {
    this.recomputeLayoutAndGeometry()
    // 图第一次到位时布局区可能已经有尺寸了，onAreaChange 不一定还会再来一次
    this.maybeFitView()
  }

  // ─── 布局与几何计算 ───

  /**
   * 从 graph 重建布局节点和几何数据。
   * 在 graph 变化、节点/Embed 移动后调用。
   */
  recomputeLayoutAndGeometry() {
    if (!this.graph) {
      this.platformLayoutNodes = []
      this.embedLayoutNodes = []
      this.nodeRects = []
      this.localEdgeRenders = []
      return
    }
    this.platformLayoutNodes = buildFreeformLayout(this.graph.nodes)
    this.embedLayoutNodes = buildEmbedLayoutNodes(this.graph.embeds, this.embedDisplayBoundsMap())
    this.recomputeGeometry()
  }

  /**
   * 重新计算几何：nodeRects + localEdgeRenders。
   * 在节点位置变化、graph 加载后调用。
   */
  recomputeGeometry() {
    if (!this.graph) {
      this.nodeRects = []
      this.localEdgeRenders = []
      return
    }
    const nodeRects: PlatformNodeRect[] = this.platformLayoutNodes.map((ln: PlatformLayoutNode) => deriveNodeRectFromLayoutNode(ln))
    const embedRects: PlatformNodeRect[] = this.embedLayoutNodes.map((ln: PlatformLayoutNode) => deriveNodeRectFromLayoutNode(ln))
    this.nodeRects = nodeRects.concat(embedRects)
    const rectMap = new Map<string, PlatformNodeRect>()
    for (const rect of this.nodeRects) {
      rectMap.set(rect.nodeId, rect)
    }
    this.localEdgeRenders = computeAllEdgeRenders(this.graph.edges, rectMap, this.starmapId, this.getEmbedInstanceIds())
    // 布局变了就同步快照：递归命中要读的是最新 rects
    this.buildSceneHandle()
    this.drawEdges()
  }

  // ─── 坐标计算 ───

  /**
   * 获取平台层布局节点（普通节点或 Embed）
   */
  private getPlatformLayoutNode(nodeId: string): PlatformLayoutNode | undefined {
    const node = this.platformLayoutNodes.find(ln => ln.nodeId === nodeId)
    return node !== undefined ? node : this.embedLayoutNodes.find(ln => ln.nodeId === nodeId)
  }

  /**
   * 计算节点在屏幕上的最终 x 坐标（含 zoom、offset、wobble）
   */
  private getNodeScreenX(nodeId: string): number {
    const ln = this.getPlatformLayoutNode(nodeId)
    const baseX = ln ? ln.x : 0
    const wobbleDx = wobbleXFromProgress(nodeId, this.motionProgress, this.motionConfig, this.gestureTracker.getState().activeItemId)
    return (baseX + wobbleDx) * this.viewportScaleValue() + this.viewportOffsetX()
  }

  /**
   * 计算节点在屏幕上的最终 y 坐标（含 zoom、offset、wobble）
   */
  private getNodeScreenY(nodeId: string): number {
    const ln = this.getPlatformLayoutNode(nodeId)
    const baseY = ln ? ln.y : 0
    const wobbleDy = wobbleYFromProgress(nodeId, this.motionProgress, this.motionConfig, this.gestureTracker.getState().activeItemId)
    return (baseY + wobbleDy) * this.viewportScaleValue() + this.viewportOffsetY()
  }

  /**
   * 把预览连线的起点更新到"源对象边界上朝向手指的那个点"。
   *
   * 必须复用 geometry 的 edgeEndpointBoundaryPoint（正式边渲染也走它）：
   * 早期版本这里直接取对象中心当起点，导致拉线预览从黑圆正中心出发，
   * 松手后正式边又落在圆周，中间跳一下（Issue #813 复审）。
   * 普通节点和 Embed 统一走这个入口，两种形状的边界算法由 geometry 决定。
   *
   * @param sourceId 连线源对象 ID（普通节点或 Embed）
   * @param pointerScreen 当前手指位置的屏幕坐标
   */
  private updatePreviewLineFrom(sourceId: string, pointerScreen: PlatformPoint) {
    const ln = this.getPlatformLayoutNode(sourceId)
    if (!ln) return
    const rect: PlatformNodeRect = deriveNodeRectFromLayoutNode(ln)
    const pointerCanvas: PlatformPoint = screenToCanvas(
      pointerScreen.x, pointerScreen.y, this.viewportScaleValue(), this.viewportOffsetX(), this.viewportOffsetY()
    )
    const start: PlatformPoint = edgeEndpointBoundaryPoint(
      rect, this.isEmbed(sourceId), pointerCanvas.x, pointerCanvas.y
    )
    this.previewLineFrom = {
      x: start.x * this.viewportScaleValue() + this.viewportOffsetX(),
      y: start.y * this.viewportScaleValue() + this.viewportOffsetY()
    }
  }

  /**
   * 计算 Embed 在屏幕上的 x 坐标（无 wobble）
   */
  private getEmbedScreenX(embedId: string): number {
    const ln = this.getPlatformLayoutNode(embedId)
    const baseX = ln ? ln.x : 0
    return baseX * this.viewportScaleValue() + this.viewportOffsetX()
  }

  /**
   * 计算 Embed 在屏幕上的 y 坐标
   */
  private getEmbedScreenY(embedId: string): number {
    const ln = this.getPlatformLayoutNode(embedId)
    const baseY = ln ? ln.y : 0
    return baseY * this.viewportScaleValue() + this.viewportOffsetY()
  }

  /**
   * 判断 nodeId 是否是 Embed
   */
  private isEmbed(nodeId: string): boolean {
    if (!this.graph) return false
    return this.graph.embeds.some(e => e.instanceId === nodeId)
  }

  // ─── 手势辅助 ───

  /**
   * 手势触点的屏幕坐标。
   */
  private gestureScreenPoint(
    event: GestureEvent | undefined,
    anchorX: number,
    anchorY: number
  ): PlatformPoint {
    if (!event || !event.fingerList || event.fingerList.length === 0) {
      return { x: anchorX, y: anchorY }
    }
    const finger = event.fingerList[0]
    if (!finger) {
      return { x: anchorX, y: anchorY }
    }
    return {
      x: anchorX + finger.localX,
      y: anchorY + finger.localY
    }
  }

  // ─── Mutation 方法 ───

  /**
   * 构造 item 的 StarMapTargetPath 端点，**统一相对根星图**。
   *
   * 之前这里是以"当前 Scene 的 starmapId + 空 segments"起手，于是每层的端点路径
   * 只在自己那一层里有效，"父节点拖到子节点上"这种跨层连线根本无法表达。
   * 现在起点统一相对根星图：segments 就是本 Scene 的绝对路径（Embed 再追加一段 enterEmbed），
   * 之后 planCrossLayerEdge 负责把它换算到真正的宿主 Scene。
   */
  private buildItemTargetPath(itemId: string): StarMapTargetPath {
    return buildTargetPathForHost(this.rootStarmapId, [], {
      scenePath: this.scenePath,
      starmapId: this.starmapId,
      kind: this.isEmbed(itemId) ? 'embed' : 'node',
      itemId: itemId
    })
  }

  // ─── 跨层连线 ───

  /**
   * 开始连线：记下起点相对**根星图**的完整路径。
   *
   * 起点必须存完整路径而不是"本层的 itemId"：跨层连线要靠它和终点的路径
   * 求最近公共祖先，宿主是父层还是祖父层完全由这两条路径决定。
   */
  beginConnectFrom(itemId: string, point: PlatformPoint) {
    const sourcePath: StarMapTargetPath = this.buildItemTargetPath(itemId)
    this.gestureTracker.beginConnect(this.scenePath, itemId, point.x, point.y)
    this.connectSourcePath = sourcePath
    this.connectSourceId = itemId
    this.connectSourceKind = this.isEmbed(itemId) ? 'embed' : 'node'
    this.previewCanvasReady = false
    this.gestureMode = 'connect'
    this.showNodeMenu = false
    this.updatePreviewLineFrom(itemId, point)
    this.previewLineTo = point
    logConnectBegin(sourcePath)
  }

  /**
   * 结束连线：用递归命中解析终点，再按最近公共祖先决定边落在哪张图上。
   *
   * 五种情形（#816 明确要求覆盖）都收敛到同一条规则：
   * 父 Node → 子 Node、子 Node → 父 Node、同一 Embed 里的两个子 Node、
   * 两个不同 Embed 里的 Node、子 Node → 更深一层的子 Embed/Node。
   * 同层连线算出来的方案和之前完全一致，所以不需要第二套分支。
   */
  async finishConnect(localScreenX: number, localScreenY: number) {
    const sourcePath: StarMapTargetPath | null = this.connectSourcePath
    if (sourcePath === null) {
      return
    }
    const context: RecursiveSceneContext | null = this.getRootContext()
    if (context === null) {
      logConnectEnd(sourcePath, null, '', false, 'no_scene_context')
      return
    }

    // 本 Scene 局部坐标 → 根 Scene 局部坐标，递归命中只能在根坐标系里做
    const rootPoint: PlatformPoint = convertPointToRoot(
      context, this.scenePath, localScreenX, localScreenY
    )
    const hit: RecursiveHit = resolveRecursiveHit(context, rootPoint.x, rootPoint.y)
    logRecursiveHit(hit)

    if (hit.target === null) {
      logConnectEnd(sourcePath, null, '', false, 'blank_target')
      return
    }
    if (hit.target.objectKind === 'embed' && hit.target.hitRegion === 'innerContent' &&
      this.isSamePath(hit.target.scenePath, this.scenePath) && hit.target.objectId === this.connectSourceId) {
      logConnectEnd(sourcePath, null, '', false, 'self')
      return
    }
    // 递归命中现在能返回 edge，但边不是合法的连线端点：把 edgeId 当成 nodeId 递给
    // Core 就是拿一条不存在的节点去建边。统一命中可以共用，可命中的对象集合却不能
    // 直接当成连线的合法端点集合（#816 复审第 8 条）。
    if (hit.target.objectKind === 'edge') {
      logConnectEnd(sourcePath, null, '', false, 'edge_target')
      return
    }

    const targetPath: StarMapTargetPath = buildTargetPathForHost(this.rootStarmapId, [], {
      scenePath: hit.target.scenePath,
      starmapId: hit.target.starmapId,
      kind: hit.target.objectKind === 'embed' ? 'embed' : 'node',
      itemId: hit.target.objectId
    })
    const plan = planCrossLayerEdge(context, sourcePath, targetPath)
    if (plan === null) {
      logConnectEnd(sourcePath, targetPath, '', false, 'same_object')
      return
    }

    // 边只能落在宿主 Scene 上：把两条相对宿主的端点路径交给它去写 Core
    const hostHandle: StarMapSceneNode | null = this.findSceneHandle(plan.hostScenePath)
    if (hostHandle === null) {
      logConnectEnd(sourcePath, targetPath, plan.hostStarmapId, false, 'no_host_scene')
      return
    }
    // 必须 await：诊断要能区分"递归命中没找到目标"和"路径算出来了但 Core 拒绝"，
    // 不等结果的话 connect_end 只会在取消路径上被写到（#816 复审第 6 条）。
    const created: boolean = await hostHandle.createEdgeBetween(plan.from, plan.to)
    logConnectEnd(sourcePath, targetPath, plan.hostStarmapId, created, created ? '' : 'core_failed')
  }

  private isSamePath(a: StarMapPathSegment[], b: StarMapPathSegment[]): boolean {
    if (a.length !== b.length) {
      return false
    }
    for (let i = 0; i < a.length; i++) {
      if (a[i].type !== b[i].type || a[i].instanceId !== b[i].instanceId || a[i].nodeId !== b[i].nodeId) {
        return false
      }
    }
    return true
  }

  /**
   * 连线收尾：只有在全局状态**仍然**是我发起的 connect 时才落边并清空 tracker。
   *
   * 第二根手指落下后根 Scene 会把全局状态切成 pinch，这时旧 connect 的
   * onActionEnd / onActionCancel 只准清自己的预览，不准碰全局 tracker，
   * 否则刚认领的双指缩放会被一起清掉（#816）。
   */
  private finishOwnedConnect() {
    const state: GestureState = this.gestureTracker.getState()
    if (state.mode === 'connect' && this.gestureTracker.isConnectOwnedByScene(this.scenePath)) {
      // 终点在根坐标系里做递归命中，父子跨层的边由最近公共祖先那张图承接
      void this.finishConnect(this.previewLineTo.x, this.previewLineTo.y)
      this.gestureTracker.reset()
    } else {
      // 已被 pinch 接管：不动全局，但要把这条 connect 的结束边界补上
      this.closeSupersededConnectIfNeeded()
    }
    this.clearConnectVisualState()
  }

  /**
   * 连线被**新手势接管**（这里只可能是 pinch）时，补齐这条 connect 的结束边界日志。
   *
   * #816 要求 connect_begin / connect_end 成对出现。真机上会出现这样的时序：
   * beginConnect → logConnectBegin → 第二根手指落下 → beginPinch → 旧 connect 的
   * onActionEnd / onActionCancel 晚到。此时 clearIfOwnedBy('connect') 返回 false，
   * 只清预览不打日志，诊断包里就只剩一个没有下文的 connect_begin，下一次看诊断
   * 分不清是"连线卡死"还是"被新手势接管"。
   *
   * 这里刻意和"能不能清新 tracker"分成两件事：
   * - 接管后**绝不能** reset 当前的 pinch，那会把刚认领的缩放一起清掉。
   * - 但"不碰 tracker"不等于"不能给自己这条旧连线写结束日志"。
   *
   * 记完立刻把 connectSourcePath 置空，保证同一次 connect 只记一条；
   * 迟到的 End 和 Cancel 都可能到达，只该有一条。
   */
  private closeSupersededConnectIfNeeded() {
    const sourcePath: StarMapTargetPath | null = this.connectSourcePath
    if (sourcePath === null) {
      return
    }
    if (this.gestureTracker.getState().mode !== 'pinch') {
      return
    }
    this.connectSourcePath = null
    logConnectEnd(sourcePath, null, '', false, 'superseded_by_pinch')
  }

  /**
   * 连线被**取消**（触摸被系统取消，不是抬手）时的收尾。
   *
   * 取消时如果全局仍然是自己发起的 connect，必须把 tracker 清回 idle ——
   * 否则 tracker 会卡在 connect，后续别层手势会因为"不是 idle、owner 也不是我"
   * 被全部挡住。已经被 pinch 接管时 clearIfOwnedBy 返回 false，只清旧预览。
   *
   * 边界日志：清掉 connectSourcePath 之前先取快照。真机上系统把手势 Cancel 掉时，
   * 诊断包里原本只会有 connect_begin 后面接一个 connect_end 也看不到的空洞，
   * 分不清是程序卡死、事件没回来，还是系统正常取消。这里补一条 cancelled 记录。
   * 被 pinch 接管时改为补 superseded_by_pinch：同样要成对，但只记边界，不动全局
   * tracker（当前跑的是 pinch，reset 它等于把刚认领的缩放清掉）。
   */
  private cancelOwnedConnect() {
    const sourcePath: StarMapTargetPath | null = this.connectSourcePath
    const cancelledOwnConnect: boolean = this.gestureTracker.clearIfOwnedBy('connect', this.scenePath)
    if (cancelledOwnConnect && sourcePath !== null) {
      logConnectEnd(sourcePath, null, '', false, 'cancelled')
    } else {
      // 已被 pinch 接管：只补旧 connect 的结束日志，不碰全局 tracker
      this.closeSupersededConnectIfNeeded()
    }
    this.clearConnectVisualState()
  }

  /**
   * 只清连线预览的本地痕迹（起点路径、预览画布、菜单标记）。
   * 这里刻意不碰 gestureTracker：手势可能已经被 pinch 接管。
   */
  private clearConnectVisualState() {
    this.connectSourcePath = null
    this.connectSourceId = ''
    this.deactivatePreviewCanvas()
    this.gestureMode = 'idle'
    this.showNodeMenu = false
    this.nodeMenuNodeId = ''
  }

  /**
   * 单指拖动画布的收尾。
   *
   * 分两件事看：
   * - 「清不清全局 tracker」需要看 ownership。beginPinch 会把 ownerScenePath
   *   也写成归属层，同一层的旧 pan 到这里 isOwnedByScene 仍是 true，所以光靠
   *   owner 判断挡不住，必须靠 clearIfOwnedBy 的 mode 核对（#816）。
   * - 「本层 UI 状态收不收」不看 ownership。旧 handler 已经结束了，本地
   *   gestureMode 没有任何理由继续停在 panCanvas，否则全局是 pinch、本地还是
   *   panCanvas，两边分叉。
   */
  private finishOwnedPan() {
    this.panActive = false
    this.gestureTracker.clearIfOwnedBy('panCanvas', this.scenePath)
    this.gestureMode = 'idle'
  }

  /**
   * 移动节点 / 移动 Embed 的收尾。
   *
   * 只有 mode 和操作对象都还是自己时才落盘；被 pinch 接管时只恢复动画，不落盘
   * 也不清全局。但本层 gestureMode 一定要收回 idle，理由同 finishOwnedPan。
   */
  private finishOwnedMove(moveMode: StarMapGestureMode, itemId: string, saveFn: () => void) {
    const state: GestureState = this.gestureTracker.getState()
    const owns: boolean = state.mode === moveMode && state.activeItemId === itemId &&
      this.gestureTracker.isActiveItemInScene(this.scenePath)
    this.panActive = false
    if (owns) {
      saveFn()
      this.gestureTracker.reset()
    }
    // 动画一定要恢复，否则节点会一直停在 pause 状态（取消路径尤其容易漏）
    this.motionPauseTracker.resumeNode(itemId)
    this.gestureMode = 'idle'
  }

  /**
   * Node 上那个 PanGesture 的收尾入口。
   *
   * 这一个 handler 有两条起手路径：菜单选了"移动"走 moveNode，否则走 panCanvas
   * 拖画布。结束时不能再固定当成 moveNode 收 —— 真走的是 panCanvas 时
   * finishOwnedMove('moveNode', ...) 会 owns=false，tracker 就永远卡在 panCanvas，
   * 之后别层交互全被"不是 idle、owner 也不是我"挡住。
   *
   * 分流看**本层**的 gestureMode，不用当前全局 mode：全局可能已经被 pinch 接管，
   * 那时读全局只会猜不出"当初这个 handler 走的是哪条路"。本层 gestureMode 才是
   * 起手时自己写下的，是唯一可靠的来源（#816）。
   */
  private finishNodePan(nodeId: string, cancelled: boolean) {
    if (this.gestureMode === 'moveNode') {
      this.finishOwnedMove('moveNode', nodeId, cancelled ? () => {} : () => {
        void this.saveNodePosition(nodeId)
      })
      return
    }
    this.finishOwnedPan()
  }

  /** Embed 上那个 PanGesture 的收尾入口，分流理由同 finishNodePan。 */
  private finishEmbedPan(embedInstanceId: string, cancelled: boolean) {
    if (this.gestureMode === 'moveEmbed') {
      this.finishOwnedMove('moveEmbed', embedInstanceId, cancelled ? () => {} : () => {
        void this.saveEmbedPosition(embedInstanceId)
      })
      return
    }
    this.finishOwnedPan()
  }

  /**
   * 落一条边：通过 bridge 的星图 CRUD API 写回**宿主 Scene**的图。
   *
   * 端点路径已经由 planCrossLayerEdge 换算成相对本 Scene，所以这里直接透传，
   * 不再做任何坐标或层级判断。边一定存进宿主那张图，这是 Core 的既有语义
   * （facade/starmap_ops/graph.rs::add_starmap_edge 把边写进 starmap_id 自己的图）。
   */
  async createEdgeBetween(from: StarMapTargetPath, to: StarMapTargetPath): Promise<boolean> {
    if (!this.bridge) {
      return false
    }
    const result = await this.bridge.addStarMapEdge(this.starmapId, from, to, 'RelatedTo', null)
    if (result.success && result.data) {
      // 成功分支必须留痕：Issue #813 的诊断包里只有 create_starmap_child_embed，
      // 无法判断是"没画线"还是"画了线但没落库"。这条日志同时覆盖普通节点和 Embed 端点。
      hilog.info(
        0x0000, 'StarMapScene',
        `createEdge ok: starmapId=${this.starmapId}, edgeId=${result.data.id}`
      )
      if (this.graph) {
        this.graph = {
          schemaVersion: this.graph.schemaVersion,
          starmapId: this.graph.starmapId,
          nodes: this.graph.nodes,
          edges: [...this.graph.edges, result.data],
          embeds: this.graph.embeds,
          links: this.graph.links,
          hyperlinks: this.graph.hyperlinks
        }
        this.recomputeGeometry()
      }
      this.onCreateEdge(this.scenePath, this.starmapId, from.target.nodeId ?? '', to.target.nodeId ?? '')
      return true
    }
    hilog.error(0x0000, 'StarMapScene', `createEdge failed: ${result.errorCode}, ${result.userMessage}`)
    return false
  }

  /**
   * 保存节点位置：通过 bridge 的星图节点更新 API 写回。
   * 使用 this.starmapId。
   */
  async saveNodePosition(nodeId: string) {
    const ln = this.getPlatformLayoutNode(nodeId)
    if (!ln || !this.bridge) return
    hilog.info(0x0000, 'StarMapScene', `saveNodePosition: ${nodeId} → (${ln.x}, ${ln.y})`)
    const result = await this.bridge.updateStarMapNode(this.starmapId, nodeId, { x: ln.x, y: ln.y })
    if (!result.success) {
      hilog.error(0x0000, 'StarMapScene', `saveNodePosition failed: ${result.errorCode ?? ''}, ${result.userMessage}`)
    }
    this.onMoveNode(this.scenePath, this.starmapId, nodeId, { x: ln.x, y: ln.y })
  }

  /**
   * 保存 Embed 位置：通过 bridge 的 Embed update API 写回。
   * 使用 this.starmapId。
   *
   * 布局节点里的 x/y 是**以 authored 中心为锚点派生出来的显示矩形左上角**（#820），
   * 尺寸也会随 LOD 和视口变。写回 Core 必须先反解回 authored 口径，
   * 绝不能把 displayX / displayY 直接存成 authored position。
   */
  async saveEmbedPosition(embedId: string) {
    const ln = this.getPlatformLayoutNode(embedId)
    if (!ln || !this.bridge) return
    const authored = embedAuthoredPositionFromDisplay(ln.x, ln.y, ln.width, ln.height)
    hilog.info(0x0000, 'StarMapScene',
      `saveEmbedPosition: ${embedId} → display(${ln.x}, ${ln.y}) → authored(${authored.x}, ${authored.y})`)
    const result = await this.bridge.updateStarMapEmbed(this.starmapId, embedId,
      { x: authored.x, y: authored.y })
    if (!result.success) {
      hilog.error(0x0000, 'StarMapScene', `saveEmbedPosition failed: ${result.errorCode ?? ''}, ${result.userMessage}`)
    }
    this.onMoveEmbed(this.scenePath, this.starmapId, embedId, { x: authored.x, y: authored.y })
  }

  /**
   * 执行新建节点：通过 bridge API 创建，使用 this.starmapId。
   * 标题由窗口层级的对话框通过参数传入。
   *
   * 新建点同样要过安全区（#818 复审）：在子星图边缘长按新建，菜单锚点虽然在圆内，
   * 但 160×80 的整块节点可能已经伸出圆外被 clip 掉；存进 Core 的也必须是夹好的坐标。
   */
  async performCreateNode(title: string) {
    const trimmed: string = title.trim()
    if (trimmed.length === 0) return
    if (!this.bridge) return
    const candidate = this.createMenuCanvasPosition
    const clampedNode = this.clampItemToLocalSafeArea(candidate.x, candidate.y,
      DEFAULT_NODE_WIDTH, DEFAULT_NODE_HEIGHT)
    const position: StarMapPoint = {
      x: clampedNode.x,
      y: clampedNode.y
    }
    const result = await this.bridge.addStarMapNode(this.starmapId, trimmed, 'Custom', position)
    if (result.success && result.data) {
      if (this.graph) {
        this.graph = {
          schemaVersion: this.graph.schemaVersion,
          starmapId: this.graph.starmapId,
          nodes: [...this.graph.nodes, result.data],
          edges: this.graph.edges,
          embeds: this.graph.embeds,
          links: this.graph.links,
          hyperlinks: this.graph.hyperlinks
        }
        this.platformLayoutNodes = buildFreeformLayout(this.graph.nodes)
        this.recomputeGeometry()
        // 子层新增内容后重算 local fit：空子星图首次 fitView 时没有内容，fitScale = 1，
        // 新建的节点 / 子星图会大过当前安全区。全局相机不受影响（#818 复审）
        this.refitAfterContentAdded()
      }
      this.selectItem('node', result.data.id)
      this.onCreateNode(this.scenePath, this.starmapId, trimmed, position)
      logCreateResult('create_node', describeScenePath(this.scenePath), this.starmapId,
        trimmed, position.x, position.y, true, `id=${result.data.id}`)
    } else {
      hilog.error(0x0000, 'StarMapScene', `addStarMapNode failed: ${result.errorCode}, ${result.userMessage}`)
      logCreateResult('create_node', describeScenePath(this.scenePath), this.starmapId,
        trimmed, position.x, position.y, false, `${result.errorCode}, ${result.userMessage}`)
    }
  }

  /**
   * 执行新建子星图：通过 bridge API 创建，使用 this.starmapId。
   * 标题由窗口层级的对话框通过参数传入。
   *
   * 新建点同样要过安全区（#818 复审）：二级子星图直径就是 DEFAULT_EMBED_DIAMETER，
   * 在父圆边缘直接出生会大到顶满父圆甚至被裁掉，必须夹进圆内接正方形并让 local fit 缩小。
   */
  async performCreateEmbed(title: string) {
    const trimmed: string = title.trim()
    if (trimmed.length === 0) return
    if (!this.bridge) return
    const candidate = this.createMenuCanvasPosition
    // 新建子星图的位置是 **authored 口径**（Core 的 embed.position 定义就是
    // "DEFAULT_EMBED_DIAMETER 方框的左上角"），所以 clamp 也要用基准直径，
    // 不能用当前视口下的展开显示尺寸 —— 后者是派生显示矩形的事（#820）。
    const clampedEmbed = this.clampItemToLocalSafeArea(candidate.x, candidate.y,
      DEFAULT_EMBED_DIAMETER, DEFAULT_EMBED_DIAMETER)
    const position: StarMapPoint = {
      x: clampedEmbed.x,
      y: clampedEmbed.y
    }
    const result = await this.bridge.createStarmapChildEmbed(this.starmapId, trimmed, position)
    if (result.success && result.data) {
      if (this.graph) {
        this.graph = {
          schemaVersion: this.graph.schemaVersion,
          starmapId: this.graph.starmapId,
          nodes: this.graph.nodes,
          edges: this.graph.edges,
          embeds: [...this.graph.embeds, result.data.embed],
          links: this.graph.links,
          hyperlinks: this.graph.hyperlinks
        }
        this.embedLayoutNodes = buildEmbedLayoutNodes(this.graph.embeds, this.embedDisplayBoundsMap())
        this.recomputeGeometry()
        // 子层新增内容后重算 local fit，让整棵子内容重新塞进父圆的安全区（#818 复审）
        this.refitAfterContentAdded()
      }
      this.onCreateEmbed(this.scenePath, this.starmapId, trimmed, position)
      logCreateResult('create_embed', describeScenePath(this.scenePath), this.starmapId,
        trimmed, position.x, position.y, true, `instanceId=${result.data.embed.instanceId}`)
    } else {
      hilog.error(0x0000, 'StarMapScene', `createStarmapChildEmbed failed: ${result.errorCode ?? 'unknown'}`)
      logCreateResult('create_embed', describeScenePath(this.scenePath), this.starmapId,
        trimmed, position.x, position.y, false, `${result.errorCode ?? 'unknown'}`)
    }
  }

  // ─── 窗口层级对话框 ───

  /**
   * 打开标题输入对话框。
   * 对话框挂在窗口层级（openCustomDialog），所以不会被圆形 Embed 的 clip(true) 裁掉。
   * 连续点击时先关掉旧对话框，避免叠多层。
   */
  private openCreateDialog(args: CreateTitleDialogArgs, isEmbedDialog: boolean) {
    const content: ComponentContent<CreateTitleDialogArgs> = createTitleDialogContent(this.getUIContext(), args)
    if (isEmbedDialog) {
      if (this.createEmbedDialogContent !== null) {
        this.getUIContext().getPromptAction().closeCustomDialog(this.createEmbedDialogContent)
      }
      this.createEmbedDialogContent = content
    } else {
      if (this.createNodeDialogContent !== null) {
        this.getUIContext().getPromptAction().closeCustomDialog(this.createNodeDialogContent)
      }
      this.createNodeDialogContent = content
    }
    // openCustomDialog 的 ComponentContent 版本完全跟随内容样式（等价 customStyle=true），
    // 对话框本身不画背景，遮罩由 maskColor 控制。
    this.getUIContext().getPromptAction().openCustomDialog(content, {
      alignment: DialogAlignment.Center,
      maskColor: '#80000000',
      autoCancel: true,
      onDidDisappear: () => {
        if (this.createNodeDialogContent === content) {
          this.createNodeDialogContent = null
        }
        if (this.createEmbedDialogContent === content) {
          this.createEmbedDialogContent = null
        }
      }
    })
  }

  /** 关闭本 Scene 打开的所有对话框（Scene 消失时也调用）。 */
  private closeCreateDialogs() {
    if (this.createNodeDialogContent !== null) {
      this.getUIContext().getPromptAction().closeCustomDialog(this.createNodeDialogContent)
      this.createNodeDialogContent = null
    }
    if (this.createEmbedDialogContent !== null) {
      this.getUIContext().getPromptAction().closeCustomDialog(this.createEmbedDialogContent)
      this.createEmbedDialogContent = null
    }
  }

  /** 打开新建节点对话框。 */
  private openCreateNodeDialog() {
    this.openCreateDialog({
      title: '新建节点',
      placeholder: '请输入节点名称',
      onConfirm: (title: string) => {
        this.closeCreateDialogs()
        void this.performCreateNode(title)
      },
      onCancel: () => {
        this.closeCreateDialogs()
      }
    }, false)
  }

  /** 打开新建子星图对话框。 */
  private openCreateEmbedDialog() {
    this.openCreateDialog({
      title: '新建子星图',
      placeholder: '请输入子星图名称',
      onConfirm: (title: string) => {
        this.closeCreateDialogs()
        void this.performCreateEmbed(title)
      },
      onCancel: () => {
        this.closeCreateDialogs()
      }
    }, true)
  }

  // ─── Canvas 绘制 ───

  private deactivatePreviewCanvas(): void {
    this.previewCanvasReady = false
  }

  /**
   * 绘制边：使用本地 localEdgeRenders。
   * wobble 偏移在节点位置计算中已包含，此处只需应用 zoom/offset 变换。
   */
  drawEdges() {
    if (!this.canvasActive || !this.canvasReady || !this.graph) return
    const ctx = this.canvasCtx

    ctx.clearRect(0, 0, ctx.width, ctx.height)

    const progress = this.motionProgress
    const wobbleMap = new Map<string, WobbleOffset>()
    for (const node of this.graph.nodes) {
      const dx = wobbleXFromProgress(node.id, progress, this.motionConfig, this.gestureTracker.getState().activeItemId)
      const dy = wobbleYFromProgress(node.id, progress, this.motionConfig, this.gestureTracker.getState().activeItemId)
      wobbleMap.set(node.id, { dx, dy })
    }

    for (const edge of this.localEdgeRenders) {
      const graphEdge = this.graph.edges.find(e => e.id === edge.edgeId)
      if (!graphEdge) continue

      const fromId = resolveEdgeEndpointAnchor(graphEdge.from, this.starmapId) ?? ''
      const toId = resolveEdgeEndpointAnchor(graphEdge.to, this.starmapId) ?? ''
      const defaultWobble: WobbleOffset = { dx: 0, dy: 0 }
      const fromWobble = wobbleMap.get(fromId) || defaultWobble
      const toWobble = wobbleMap.get(toId) || defaultWobble

      const sx = (edge.startX + fromWobble.dx) * this.viewportScaleValue() + this.viewportOffsetX()
      const sy = (edge.startY + fromWobble.dy) * this.viewportScaleValue() + this.viewportOffsetY()
      const ex = (edge.endX + toWobble.dx) * this.viewportScaleValue() + this.viewportOffsetX()
      const ey = (edge.endY + toWobble.dy) * this.viewportScaleValue() + this.viewportOffsetY()

      // 选中的边用 (scenePath, kind, itemId) 判定，不看端点 id：
      // 同层里"起点是我选中的节点"和"这条边本身被选中"是两件事。
      const isSelected = this.selectionState.isSelected(this.scenePath, 'edge', graphEdge.id)

      ctx.beginPath()
      ctx.moveTo(sx, sy)
      ctx.lineTo(ex, ey)
      ctx.strokeStyle = isSelected ? this.themeConfig.colors.primary : this.themeConfig.colors.outline
      ctx.lineWidth = isSelected ? 2.5 : 1.5

      if (graphEdge.kind === 'DependsOn' || graphEdge.kind === 'ConflictsWith') {
        ctx.setLineDash([6, 4])
      } else {
        ctx.setLineDash([])
      }
      ctx.stroke()
      ctx.setLineDash([])

      // 箭头
      const atx = (edge.arrowTipX + toWobble.dx) * this.viewportScaleValue() + this.viewportOffsetX()
      const aty = (edge.arrowTipY + toWobble.dy) * this.viewportScaleValue() + this.viewportOffsetY()
      const alx = (edge.arrowLeftX + toWobble.dx) * this.viewportScaleValue() + this.viewportOffsetX()
      const aly = (edge.arrowLeftY + toWobble.dy) * this.viewportScaleValue() + this.viewportOffsetY()
      const arx = (edge.arrowRightX + toWobble.dx) * this.viewportScaleValue() + this.viewportOffsetX()
      const ary = (edge.arrowRightY + toWobble.dy) * this.viewportScaleValue() + this.viewportOffsetY()

      ctx.beginPath()
      ctx.moveTo(atx, aty)
      ctx.lineTo(alx, aly)
      ctx.moveTo(atx, aty)
      ctx.lineTo(arx, ary)
      ctx.strokeStyle = isSelected ? this.themeConfig.colors.primary : this.themeConfig.colors.outline
      ctx.lineWidth = isSelected ? 2.5 : 1.5
      ctx.stroke()

      // 边标签
      if (graphEdge.label && edge.labelX !== undefined && edge.labelY !== undefined) {
        const lx = (edge.labelX + (fromWobble.dx + toWobble.dx) / 2) * this.viewportScaleValue() + this.viewportOffsetX()
        const ly = (edge.labelY + (fromWobble.dy + toWobble.dy) / 2) * this.viewportScaleValue() + this.viewportOffsetY()
        ctx.font = '11px HarmonyOS Sans'
        ctx.fillStyle = isSelected ? this.themeConfig.colors.primary : this.themeConfig.colors.onSurfaceVariant
        ctx.textAlign = 'center'
        ctx.textBaseline = 'bottom'
        ctx.fillText(graphEdge.label, lx, ly - 6)
      }
    }
  }

  /**
   * 绘制预览连线：connect 模式下从源节点到当前手指位置。
   */
  drawPreviewLine() {
    if (!this.canvasActive || !this.previewCanvasReady) return
    const ctx = this.previewCanvasCtx

    ctx.clearRect(0, 0, ctx.width, ctx.height)

    ctx.beginPath()
    ctx.moveTo(this.previewLineFrom.x, this.previewLineFrom.y)
    ctx.lineTo(this.previewLineTo.x, this.previewLineTo.y)
    ctx.strokeStyle = this.themeConfig.colors.primary
    ctx.lineWidth = 2
    ctx.setLineDash([8, 4])
    ctx.stroke()
    ctx.setLineDash([])
  }

  // ─── 节点颜色 ───

  getNodeColor(node: StarMapNode): string {
    const kindColors: Record<string, string> = {
      'Character': '#4A90D9',
      'Location': '#F39C12',
      'Event': '#9B59B6',
      'Theme': '#2ECC71',
      'Plot': '#E74C3C',
      'Item': '#1ABC9C',
      'Organization': '#3498DB',
      'Custom': '#95A5A6'
    }
    return kindColors[node.kind] || '#4A90D9'
  }

  // ─── @Builder：节点组件 ───

  /**
   * 节点组件：长按 → 菜单（连线预备态）→ 继续拖动 → connect。
   * 所有 gestureTracker.begin* 调用传入 this.scenePath 作为 ownerScenePath。
   */
  @Builder
  StarMapNodeItem(node: StarMapNode) {
    Column() {
      Circle({ width: 56 * this.viewportScaleValue(), height: 56 * this.viewportScaleValue() })
        .fill(this.getNodeColor(node))
        .stroke(this.isNodeSelected(node.id) ? STARMAP_SELECTED_STROKE_COLOR : 'transparent')
        .strokeWidth(4)
        .shadow({
          radius: this.gestureTracker.isDraggingNode() && this.gestureTracker.getState().activeItemId === node.id ?
            DesignTokens.shadowStrong.radius :
            (this.isNodeSelected(node.id) ? DesignTokens.shadowMedium.radius : DesignTokens.shadowLight.radius),
          color: this.gestureTracker.isDraggingNode() && this.gestureTracker.getState().activeItemId === node.id ?
            '#40000000' :
            (this.isNodeSelected(node.id) ? '#40FF6B6B' : '#20000000'),
          offsetX: 0,
          offsetY: this.gestureTracker.isDraggingNode() && this.gestureTracker.getState().activeItemId === node.id ?
            DesignTokens.shadowStrong.offsetY : DesignTokens.shadowLight.offsetY
        })

      Text(node.title)
        .fontSize(13 * this.viewportScaleValue())
        .fontWeight(this.isNodeSelected(node.id) ? FontWeight.Bold : FontWeight.Normal)
        .margin({ top: 6 * this.viewportScaleValue() })
        .maxLines(2)
        .textAlign(TextAlign.Center)
        .width(80 * this.viewportScaleValue())
    }
    .width(160 * this.viewportScaleValue())
    .height(80 * this.viewportScaleValue())
    .justifyContent(FlexAlign.Center)
    .position({
      x: this.getNodeScreenX(node.id),
      y: this.getNodeScreenY(node.id)
    })
    .scale(
      this.gestureTracker.isDraggingNode() && this.gestureTracker.getState().activeItemId === node.id
        ? { x: this.motionConfig.dragLiftScale, y: this.motionConfig.dragLiftScale }
        : { x: 1, y: 1 }
    )
    .onClick(() => {
              // 双指已接管时不再改选中态（#818 复审）
              if (this.gestureTracker.isPinching()) return
              this.selectItem('node', node.id)
            })
    // 菜单不挂在本 Column 上：bindMenu 按被绑定组件定位，节点本体位置会随节点移动而变化，
    // 而且 Scene 栈里的 1vp 锚点才能让菜单贴着实际长按的手指位置弹出（见 build() 的锚点）。
    // 这里只负责翻转 showNodeMenu 并记下锚点，菜单内容通过 this.nodeMenuNodeId 区分节点 / Embed。
    .gesture(
      GestureGroup(GestureMode.Exclusive,
        // 长按 → 菜单（连线预备态）→ 继续拖动 → connect
        GestureGroup(GestureMode.Sequence,
          LongPressGesture({ repeat: false, duration: 500 })
            .onAction((event: GestureEvent) => {
              if (this.gestureTracker.isPinching()) return
              if (!this.gestureTracker.isOwnedByScene(this.scenePath) && !this.gestureTracker.isIdle()) return
              const point = this.gestureScreenPoint(event, this.getNodeScreenX(node.id), this.getNodeScreenY(node.id))
              this.gestureTracker.beginNodeMenu(this.scenePath, node.id, point.x, point.y)
              this.gestureMode = 'nodeMenu'
              // 先记录目标再翻显示标记：菜单内容要在 showNodeMenu 变 true 时就读到 nodeMenuNodeId
              this.nodeMenuNodeId = node.id
              // 锚点记手指实际位置，菜单从这儿弹出而不是从节点中心弹出
              this.nodeMenuAnchor = { x: point.x, y: point.y }
              this.showNodeMenu = true
              this.selectItem('node', node.id)
            }),
          PanGesture()
            .onActionStart((event: GestureEvent) => {
              if (this.gestureTracker.isPinching()) return
              if (!this.gestureTracker.isOwnedByScene(this.scenePath) && !this.gestureTracker.isIdle()) return
              const point = this.gestureScreenPoint(event, this.getNodeScreenX(node.id), this.getNodeScreenY(node.id))
              // 起点存完整路径，跨层连线要靠它和终点求最近公共祖先（#816）
              this.beginConnectFrom(node.id, point)
            })
            .onActionUpdate((event: GestureEvent) => {
              if (this.gestureTracker.isPinching()) return
              if (!this.gestureTracker.isOwnedByScene(this.scenePath)) return
              const state = this.gestureTracker.getState()
              if (state.mode === 'connect') {
                this.previewLineTo = this.gestureScreenPoint(event, this.getNodeScreenX(node.id), this.getNodeScreenY(node.id))
                // 起点随手指方向实时重算，始终贴在源对象边界上
                this.updatePreviewLineFrom(node.id, this.previewLineTo)
                this.drawPreviewLine()
              }
            })
            .onActionEnd(() => {
              // 统一收尾：mode 已经不是 connect（被 pinch 接管）时只清预览，不碰全局 tracker
              this.finishOwnedConnect()
            })
            .onActionCancel(() => {
              // 触摸被系统取消：仍是自己的 connect 就清回 idle，已被 pinch 接管就只清预览（#816）
              this.cancelOwnedConnect()
            })
        ),
        // 没有长按成功时的普通滑动
        PanGesture()
          .onActionStart((event: GestureEvent) => {
            // 双指一旦接管，移动节点和拖画布都要让位（#818 复审）
            if (this.gestureTracker.isPinching()) return
            const state = this.gestureTracker.getState()
            if (state.mode === 'moveNode' && state.activeItemId === node.id) {
              this.motionPauseTracker.pauseNode(node.id)
              const ln = this.getPlatformLayoutNode(node.id)
              if (ln) {
                this.moveNodeStartX = ln.x
                this.moveNodeStartY = ln.y
              }
            } else {
              if (!this.gestureTracker.isOwnedByScene(this.scenePath) && !this.gestureTracker.isIdle()) return
              this.panActive = true
              const point = this.gestureScreenPoint(event, 0, 0)
              this.gestureTracker.beginPanCanvas(this.scenePath, point.x, point.y)
              this.gestureMode = 'panCanvas'
              this.panBaseCameraOffsetX = this.cameraOffsetX
              this.panBaseCameraOffsetY = this.cameraOffsetY
            }
          })
          .onActionUpdate((event: GestureEvent) => {
            if (this.gestureTracker.isPinching()) return
            if (!this.gestureTracker.isOwnedByScene(this.scenePath)) return
            const state = this.gestureTracker.getState()
            if (state.mode === 'moveNode' && state.activeItemId === node.id) {
              const movingNodeSize = this.itemCanvasSize(node.id, 0, 0)
              const newDx = this.moveNodeStartX + event.offsetX / this.viewportScaleValue()
              const newDy = this.moveNodeStartY + event.offsetY / this.viewportScaleValue()
              // 子星图里移动节点必须夹在本层圆内接正方形里，否则会被父圆 clip 掉（#818 复审）
              const clamped = this.clampItemToLocalSafeArea(newDx, newDy,
                movingNodeSize.x, movingNodeSize.y)
              const positions = new Map<string, PlatformPoint>()
              positions.set(node.id, { x: clamped.x, y: clamped.y })
              this.platformLayoutNodes = applyFreeformLayout(this.platformLayoutNodes, positions)
              this.recomputeGeometry()
            } else if (state.mode === 'panCanvas' && this.panActive &&
              this.gestureTracker.isPanOwnedByScene(this.scenePath) && this.canMutateViewport()) {
              // 拖的是整张星图的视角；event.offset 是累计位移，所以用起手基准 + 累计值算目标（#818 复审）
              this.applyCameraPanTo(this.panBaseCameraOffsetX + event.offsetX,
                this.panBaseCameraOffsetY + event.offsetY)
            }
          })
          .onActionEnd(() => {
            // 起手可能是移动节点，也可能是拖画布，按本地 gestureMode 分流收尾
            this.finishNodePan(node.id, false)
          })
          .onActionCancel(() => {
            this.finishNodePan(node.id, true)
          })
      )
    )
  }

  // ─── Embed 手势组 ───

  /**
   * Embed 手势组：选中/菜单/连线/移动/画布拖动。
   * title Row 和边框命中条共享此手势组。
   * Pan 手势：moveEmbed 模式时移动 Embed，否则按 #373 规则拖动当前 Scene 画布。
   * innerContent 不挂父手势，由 child Scene 自己处理。
   * 所有 onActionStart 检查 isOwnedByScene，防止别层正在用手势时覆盖 owner。
   *
   * 单击和双击都必须落在同一个 Embed 选中上（#816）：双击声明在单击之前，
   * 双击的第二次按下不触发单击识别，所以单击不会被双击抢走。
   * 两者都只是选中这个 Embed，都不开新页面。
   */
  private embedGestureGroup(embed: StarMapEmbed): GestureGroupInterface {
    return GestureGroup(GestureMode.Exclusive,
      // 双击 → 选中 Embed（和单击同一个结果）
      TapGesture({ count: 2 })
        .onAction(() => {
          if (this.gestureTracker.isPinching()) return
          this.selectItem('embed', embed.instanceId)
        }),
      // 单击 → 选中 Embed
      TapGesture({ count: 1 })
        .onAction(() => {
          if (this.gestureTracker.isPinching()) return
          this.selectItem('embed', embed.instanceId)
        }),
      // 长按 → 菜单（连线预备态）→ 继续拖动 → connect
      GestureGroup(GestureMode.Sequence,
        LongPressGesture({ repeat: false, duration: 500 })
          .onAction((event: GestureEvent) => {
            if (this.gestureTracker.isPinching()) return
            if (!this.gestureTracker.isOwnedByScene(this.scenePath) && !this.gestureTracker.isIdle()) return
            const point = this.gestureScreenPoint(event, this.getEmbedScreenX(embed.instanceId), this.getEmbedScreenY(embed.instanceId))
            this.gestureTracker.beginNodeMenu(this.scenePath, embed.instanceId, point.x, point.y)
            this.gestureMode = 'nodeMenu'
            // 菜单内容在 showNodeMenu 翻 true 的同一帧就要读到 nodeMenuNodeId，所以先赋值再翻标记
            this.nodeMenuNodeId = embed.instanceId
            // 和普通节点共用同一个锚点：长按点边框时菜单也从实际按下处弹出
            this.nodeMenuAnchor = { x: point.x, y: point.y }
            this.showNodeMenu = true
          }),
        PanGesture()
          .onActionStart((event: GestureEvent) => {
            if (this.gestureTracker.isPinching()) return
            if (!this.gestureTracker.isOwnedByScene(this.scenePath) && !this.gestureTracker.isIdle()) return
            const point = this.gestureScreenPoint(event, this.getEmbedScreenX(embed.instanceId), this.getEmbedScreenY(embed.instanceId))
            this.beginConnectFrom(embed.instanceId, point)
          })
          .onActionUpdate((event: GestureEvent) => {
            if (this.gestureTracker.isPinching()) return
            if (!this.gestureTracker.isOwnedByScene(this.scenePath)) return
            const state = this.gestureTracker.getState()
            if (state.mode === 'connect') {
              this.previewLineTo = this.gestureScreenPoint(event, this.getEmbedScreenX(embed.instanceId), this.getEmbedScreenY(embed.instanceId))
              // Embed 起点同样走圆周交点，不会再从黑圆中心拉出虚线
              this.updatePreviewLineFrom(embed.instanceId, this.previewLineTo)
              this.drawPreviewLine()
            }
          })
          .onActionEnd(() => {
            // 与 Node 的 connect 走同一条收尾：被 pinch 接管时只清预览
            this.finishOwnedConnect()
          })
          .onActionCancel(() => {
            // 与 Node 的 connect 走同一条取消收尾
            this.cancelOwnedConnect()
          })
      ),
      // 没有长按成功时的普通滑动（moveEmbed 或画布拖动，与 #373 规则一致）
      PanGesture()
        .onActionStart((event: GestureEvent) => {
          // 双指一旦接管，移动 Embed 和拖画布都要让位（#818 复审）
          if (this.gestureTracker.isPinching()) return
          const state = this.gestureTracker.getState()
          if (state.mode === 'moveEmbed' && state.activeItemId === embed.instanceId) {
            this.motionPauseTracker.pauseNode(embed.instanceId)
            const ln = this.getPlatformLayoutNode(embed.instanceId)
            if (ln) {
              this.moveEmbedStartX = ln.x
              this.moveEmbedStartY = ln.y
            }
          } else {
            if (!this.gestureTracker.isOwnedByScene(this.scenePath) && !this.gestureTracker.isIdle()) return
            this.panActive = true
            const point = this.gestureScreenPoint(event, 0, 0)
            this.gestureTracker.beginPanCanvas(this.scenePath, point.x, point.y)
            this.gestureMode = 'panCanvas'
            this.panBaseCameraOffsetX = this.cameraOffsetX
            this.panBaseCameraOffsetY = this.cameraOffsetY
          }
        })
        .onActionUpdate((event: GestureEvent) => {
          if (this.gestureTracker.isPinching()) return
          if (!this.gestureTracker.isOwnedByScene(this.scenePath)) return
          const state = this.gestureTracker.getState()
          if (state.mode === 'moveEmbed' && state.activeItemId === embed.instanceId) {
            const movingEmbedSize = this.itemCanvasSize(embed.instanceId,
              this.expandedEmbedSizeVp(), this.expandedEmbedSizeVp())
            const newDx = this.moveEmbedStartX + event.offsetX / this.viewportScaleValue()
            const newDy = this.moveEmbedStartY + event.offsetY / this.viewportScaleValue()
            // 子星图里移动 Embed 同样要夹在圆内，否则半个圆壳会被父圆 clip 掉（#818 复审）
            const clamped = this.clampItemToLocalSafeArea(newDx, newDy,
              movingEmbedSize.x, movingEmbedSize.y)
            const positions = new Map<string, PlatformPoint>()
            positions.set(embed.instanceId, { x: clamped.x, y: clamped.y })
            this.embedLayoutNodes = applyFreeformLayout(this.embedLayoutNodes, positions)
            this.recomputeGeometry()
          } else if (state.mode === 'panCanvas' && this.panActive &&
            this.gestureTracker.isPanOwnedByScene(this.scenePath) && this.canMutateViewport()) {
            // 拖的是整张星图的视角；event.offset 是累计位移，所以用起手基准 + 累计值算目标（#818 复审）
            this.applyCameraPanTo(this.panBaseCameraOffsetX + event.offsetX,
              this.panBaseCameraOffsetY + event.offsetY)
          }
        })
        .onActionEnd(() => {
          // 与 Node 同一个分流：起手可能是移动 Embed，也可能是拖画布
          this.finishEmbedPan(embed.instanceId, false)
        })
        .onActionCancel(() => {
          this.finishEmbedPan(embed.instanceId, true)
        })
    )
  }

  // ─── @Builder：Embed 组件 ───

  /**
   * Embed 的渲染入口：按视觉 LOD 分派（#820）。
   *
   * 档位是**渲染前**算好的（embedLod），这里只负责按档位挂不同的组件树：
   * - expanded：完整正圆 + StarMapEmbedScene 递归实例化 child Scene
   * - collapsed：节点尺寸的摘要卡（标题 + 子星图身份 + 选中态），
   *   **不实例化** child Scene，所以它不占任何递归层级的渲染开销
   * - hidden：祖先已折叠，本层连摘要卡都不画（数据仍在 Core 里）
   *
   * 关键结构：只有 expanded 分支里才出现 StarMapEmbedScene。
   * 折叠态绝不能"先建好再缩到看不见"（那是白花开销），也不用 opacity(0) 假装折叠。
   */
  @Builder
  StarMapEmbedItem(embed: StarMapEmbed) {
    if (this.embedLod(embed.instanceId) === 'expanded') {
      this.StarMapExpandedEmbed(embed)
    } else if (this.embedLod(embed.instanceId) === 'collapsed') {
      this.StarMapCollapsedEmbed(embed)
    }
  }

  /**
   * 展开态 Embed：主题容器色正圆，画布上唯一的外接形状是圆（#818 底色已跟随主题）。
   *
   * 尺寸只有一个来源：`expandedEmbedSizeVp()`（视口显示预算 × 基准直径），
   * 宽=高=边长，圆角=边长/2。这里禁止再出现 200 / 120 / 12 这类手写尺寸。
   *
   * 命中分层不再用"三根矩形窄条 + clip 裁圆环"：clip(true) 只会把矩形条裁成圆内的几段直线，
   * 斜着点边框（例如 45° 方向）既不在 title 也不在任何窄条上，事件会掉进 child Scene，
   * 于是"几何算出来是 embedBorder、实际却进了子图"。现在两层一起改：
   * - responseRegion 把真实触摸热区切成圆（默认热区是整个矩形，clip 不裁热区）
   * - 外层 Stack 的 onTouchIntercept 按真实圆距离动态分流：
   *   title 条或真实圆环 → HitTestMode.Block，只给外层 Embed，child Scene 不参与
   *   圆外 / 圆内其余区域 → HitTestMode.None，事件落回父星图或交给 child StarMapScene
   */
  @Builder
  StarMapExpandedEmbed(embed: StarMapEmbed) {
    Stack() {
      // 1. innerContent 区域（占满整个 Embed 内容区，放 child StarMapEmbedScene）
      // 不挂任何手势，能不能收到事件由下面的 onTouchIntercept 决定
      Column() {
        StarMapEmbedScene({
          rootStarmapId: this.rootStarmapId,
          scenePath: this.scenePath,
          embed: embed,
          // 子 Scene 的深度是父层 +1：只有根 Scene 是全局相机本体（#818）
          sceneDepth: this.sceneDepth + 1,
          themeConfig: this.themeConfig,
          motionConfig: this.motionConfig,
          motionProgress: this.motionProgress,
          bridge: this.bridge,
          gestureTracker: this.gestureTracker,
          // 全树共用的选中状态和 Scene 注册表
          selectionState: this.selectionState,
          sceneRegistry: this.sceneRegistry,
          // 全局相机原样透传：子 Scene 是相机里的内容，不持有自己的缩放（#818）
          cameraScale: this.cameraScale,
          cameraOffsetX: this.cameraOffsetX,
          cameraOffsetY: this.cameraOffsetY,
          onCameraChanged: this.onCameraChanged,
          // 空白拖动继续往下传：子星图里拖动的也是整张星图的视角（#818 复审）
          onCameraPanTo: this.onCameraPanTo,
          // 根视口与视觉焦点原样透传：整棵递归树共用同一份（#820）。
          // 子 Scene 不许拿自己那个圆当视口去判 LOD，否则"够不够展开"变成自我实现。
          rootViewportWidth: this.rootViewportWidth,
          rootViewportHeight: this.rootViewportHeight,
          focusScenePath: this.focusScenePath,
          // 祖先累计比例：子 Scene 的有效比例 = 它的 fitScale × 这一份（#818 复审）
          inheritedScale: this.viewportScaleValue(),
        })
      }
      .width('100%')
      .height('100%')
      // 稳定输入节点 id（#818 复审）：onChildTouchTest 只能看到显式设了 .id() 的命名节点，
      // 没有它父级就分不清该转发给哪一层，只能退回 Stack 默认命中顺序。
      .id(this.embedInputNodeId(embed.instanceId))

      // 2. title 视觉层（纯显示，不挂手势也不挂菜单）
      // 手势统一由外层 Stack 的 embedGestureGroup + onTouchIntercept 分流，
      // 菜单由 Scene 栈里的 1vp 锚点承载（菜单若挂在这个 title 上，
      // 位置会被节点本体绑死，点圆环时菜单不从按下处弹出）。
      Row() {
        Text(embed.label ?? embed.targetStarmapId)
          .fontSize(14 * this.viewportScaleValue())
          .fontWeight(FontWeight.Medium)
          // 跟随主题：外壳底色已经是主题 surfaceContainer，写死白色在浅色主题下会看不清（#818）
          .fontColor(this.themeConfig.colors.onSurface)
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          // 圆顶部的可见宽度远小于直径：左对齐 + padding 的文字前半截会被圆裁掉，必须居中
          .width(80 * this.viewportScaleValue())
          .textAlign(TextAlign.Center)
      }
      .width('100%')
      .height(EMBED_TITLE_HIT_HEIGHT * this.viewportScaleValue())
      .justifyContent(FlexAlign.Center)
      .alignItems(VerticalAlign.Center)
      .position({ x: 0, y: 0 })
      .hitTestBehavior(HitTestMode.None)
    }
    .width(projectLocalLengthToScreen(this.expandedEmbedSizeVp(), this.viewportScaleValue()))
    .height(projectLocalLengthToScreen(this.expandedEmbedSizeVp(), this.viewportScaleValue()))
    .borderRadius(projectLocalLengthToScreen(this.expandedEmbedSizeVp() / 2, this.viewportScaleValue()))
    // 跟随主题：写死黑色时浅色主题下整颗子星图是一块黑饼，标题白字也压不出来（#818）
    .backgroundColor(this.themeConfig.colors.surfaceContainer)
    // 未选中也要有一条细描边，否则圆壳在同色底上看不出边界；选中时加粗。
    // 选中色用 STARMAP_SELECTED_STROKE_COLOR（和普通节点一致），不能取 themeConfig.colors.primary：
    // 主色跟着配色走，可能和外壳底色接近，选中态就糊了。
    .border({
      width: this.isEmbedSelected(embed.instanceId) ? 3 : 1,
      color: this.isEmbedSelected(embed.instanceId)
        ? STARMAP_SELECTED_STROKE_COLOR
        : this.themeConfig.colors.outlineVariant
    })
    .position({
      x: this.getEmbedScreenX(embed.instanceId),
      y: this.getEmbedScreenY(embed.instanceId)
    })
    // 子 Scene 必须真正被裁到圆内，否则内部会溢出圆外看起来还是方形
    .clip(true)
    // 圆形热区：ArkUI 组件默认热区是整个矩形，borderRadius + clip 只裁视觉不裁热区，
    // 不显式声明的话圆外四个方角依然可点。切成若干条水平带逼近圆（见 buildCircleResponseBands）。
    .responseRegion(this.buildEmbedCircleResponseRegions())
    .gesture(this.embedGestureGroup(embed))
    // 动态事件分流（坐标是相对本组件的局部坐标，圆心即 直径/2）：
    // - 圆外：不属于 Embed，热区已排除，这里再兜一层，事件落回父星图
    // - 圆内 title / 真实圆环：Block，只给外层 Embed，不再让 child Scene 参与手势竞争
    //   （Default 是"自己和子节点都参加"，父 Embed 和 child Scene 会一起进竞争）
    // - 圆内其余：None，交给 child StarMapScene
    .onTouchIntercept((event: TouchEvent): HitTestMode => {
      const touch = event.changedTouches[0]
      if (!touch) {
        return HitTestMode.None
      }
      // 相对坐标字段在真机上不总是有值，这里只吃有限数，
      // 缺了就直接放行，不要拿 NaN 去算圆环命中。
      const localX: number = touch.x
      const localY: number = touch.y
      if (!Number.isFinite(localX) || !Number.isFinite(localY)) {
        return HitTestMode.None
      }
      const diameter: number = this.expandedEmbedSizeVp() * this.viewportScaleValue()
      const radius: number = diameter / 2
      const dx: number = localX - radius
      const dy: number = localY - radius
      const dist: number = Math.sqrt(dx * dx + dy * dy)
      if (dist > radius) {
        return HitTestMode.None
      }
      // 这里的 touch.x / touch.y 已经是**组件局部坐标**，而组件本身按
      // `expandedEmbedSizeVp() * viewportScaleValue()` 显示，所以这一格
      // 1 单位 = 屏幕 1vp。热区规格 24 / 12 是屏幕侧的，除以 1 就是本组件坐标下的值——
      // 再乘一次 viewportScaleValue() 才是错的（#818 复审）：那会让标题带和圆环
      // 随递归 local fit 一起缩水，深一层就少一半。
      // 组件坐标 → 画布坐标的换算封装在 embedHitMetricsForScene 里，
      // 和 resolveRecursiveHit 用的是同一个函数，两边不可能再各算各的。
      const metrics = embedHitMetricsForScene(1, radius)
      const inTitle: boolean = localY <= metrics.titleHitHeight
      const inBorder: boolean = dist >= radius - metrics.borderHitWidth
      return (inTitle || inBorder) ? HitTestMode.Block : HitTestMode.None
    })
    // 父子触摸竞争（#818 复审）：
    // 上面 onTouchIntercept 只决定"要不要让 child 参与"，它管不了"深层 child 被 touch-test
    // 分流掉之后，根 Scene 的两指还认不认得到"。ArkUI 是先触摸测试收集响应组件、再分发手势，
    // 嵌套越深这条链越长，根的 priorityGesture 只能决定**已经进同一竞争链**之后的优先级，
    // 不能保证被深层 touch-test 截断的触点回到根 Pinch。
    //
    // 所以这里显式声明：被命中的那个 Embed 输入节点继续参加竞争
    // （FORWARD_COMPETITION = 把事件投给它，同时系统仍决定要不要投给别的兄弟），
    // 根的两指链不被截断；子 Scene 的单指手势也仍然留在竞争里。
    // 注意这只做"事件分发"，不新增任何双指识别器——两指仍然只有根那一份 PinchGesture。
    .onChildTouchTest((children: TouchTestInfo[]): TouchResult => {
      return this.routeChildTouchTest(children)
    })
  }

  /**
   * 生成 Embed 圆形的真实触摸热区：把圆切成若干条水平矩形带。
   * 几何在 StarMapGeometry.buildCircleResponseBands（纯函数，可测），
   * 这里只负责转成 ArkUI 的 Rectangle。
   */
  private buildEmbedCircleResponseRegions(): Rectangle[] {
    const bands = buildCircleResponseBands(
      projectLocalLengthToScreen(this.expandedEmbedSizeVp(), this.viewportScaleValue())
    )
    const regions: Rectangle[] = []
    for (const band of bands) {
      regions.push({
        x: band.x,
        y: band.y,
        width: band.width,
        height: band.height
      })
    }
    return regions
  }

  /**
   * 折叠态 Embed：和普通节点同尺寸的摘要卡（#820）。
   *
   * 保留三样身份信息：标题、"这是一个子星图"、选中态。
   * 丢掉的是 child Scene——**不创建**，所以深层递归的渲染开销是真的降下来了，
   * 而不是"建好了再缩到看不见"。
   *
   * 热区整块归这个 Embed：点它就是选中这个子星图，不存在"点进去编辑"这一层
   * （里面本来就没有 child Scene）。所以这里不用圆形 responseRegion、
   * 不用 onTouchIntercept 分流，也不用 onChildTouchTest —— 卡里没有子组件参与竞争。
   *
   * 位置仍然来自 embed.position（Core 的 authored position）：
   * 语义缩放只改显示，不回写数据，用户捏合不该顺手改掉星图布局。
   */
  @Builder
  StarMapCollapsedEmbed(embed: StarMapEmbed) {
    Column() {
      Text(embed.label ?? embed.targetStarmapId)
        .fontSize(14)
        .fontWeight(FontWeight.Medium)
        .fontColor(this.themeConfig.colors.onSurface)
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
        .width('100%')
        .textAlign(TextAlign.Center)

      Text($r('app.string.starmap_sub_starmap'))
        .fontSize(11)
        .fontColor(this.themeConfig.colors.onSurfaceVariant)
        .maxLines(1)
        .width('100%')
        .textAlign(TextAlign.Center)
    }
    .width(projectLocalLengthToScreen(DEFAULT_NODE_WIDTH, this.viewportScaleValue()))
    .height(projectLocalLengthToScreen(DEFAULT_NODE_HEIGHT, this.viewportScaleValue()))
    .justifyContent(FlexAlign.Center)
    .backgroundColor(this.themeConfig.colors.surfaceContainer)
    .borderRadius(projectLocalLengthToScreen(DEFAULT_NODE_RADIUS, this.viewportScaleValue()))
    .border({
      width: this.isEmbedSelected(embed.instanceId) ? 3 : 1,
      color: this.isEmbedSelected(embed.instanceId)
        ? STARMAP_SELECTED_STROKE_COLOR
        : this.themeConfig.colors.outlineVariant
    })
    .position({
      x: this.getEmbedScreenX(embed.instanceId),
      y: this.getEmbedScreenY(embed.instanceId)
    })
    .clip(true)
    .gesture(this.embedGestureGroup(embed))
  }

  // ─── 菜单显示状态回写 ───

  /**
   * 节点/Embed 菜单消失后的状态回写（MenuOptions.onDisappear）。
   *
   * 程序化 bindMenu(isShow, ...) 的 isShow 不会自动双向同步：用户点菜单外部关掉菜单后，
   * showNodeMenu 可能还停在 true。下一次长按同一个对象再赋一次 true，状态没有变化，
   * 菜单就可能不再重新打开。所以必须在这里把状态写回。
   *
   * 不能无脑 reset 手势：长按后继续拖动（进 connect）和点"移动"（进 moveNode/moveEmbed）
   * 也会主动关菜单，这两种情况下 gestureMode 已经不是 nodeMenu，所以只在 nodeMenu 态才 reset。
   */
  private onNodeMenuDisappear() {
    this.showNodeMenu = false
    // 本地 gestureMode 和全局 mode 都得是 nodeMenu 才清全局：
    // 只看本地的话，本地标记还没更新到位就会把别的手势（尤其是刚认领的 pinch）清掉。
    if (this.gestureTracker.clearIfOwnedBy('nodeMenu', this.scenePath)) {
      this.gestureMode = 'idle'
    }
    this.nodeMenuNodeId = ''
  }

  /** 新建菜单消失后的状态回写（MenuOptions.onDisappear）。 */
  private onCreateMenuDisappear() {
    this.showCreateMenu = false
  }

  // ─── @Builder：节点菜单（bindMenu 内容） ───

  /**
   * 节点/Embed 菜单内容：交给 ArkUI 程序化 bindMenu 渲染。
   * 不再用 Column + Button + .position() 自绘弹层：自绘弹层是递归 Scene 的普通子组件，
   * 会被圆形 Embed 的 clip(true) 裁掉，也会因为用 Scene 局部坐标定位而越出屏幕（Issue #813）。
   * 程序化 bindMenu 由系统定位并自动避让屏幕边缘，菜单挂在哪个组件上就出现在哪附近。
   *
   * bindMenu 只负责"显示"：长按和后续 Pan 仍然由 GestureGroup 状态机负责，
   * 所以 #373 的"长按出菜单、继续移动拉线"不受影响。
   */
  @Builder
  NodeMenuContent() {
    Menu() {
      if (this.isEmbed(this.nodeMenuNodeId)) {
        MenuItem({ content: $r('app.string.move_embed') })
          .onClick(() => {
            this.gestureTracker.beginMoveEmbed(this.scenePath, this.nodeMenuNodeId, 0, 0)
            this.gestureMode = 'moveEmbed'
            this.showNodeMenu = false
            this.motionPauseTracker.pauseNode(this.nodeMenuNodeId)
          })
      } else {
        MenuItem({ content: $r('app.string.move_node') })
          .onClick(() => {
            this.gestureTracker.beginMoveNode(this.scenePath, this.nodeMenuNodeId, 0, 0)
            this.gestureMode = 'moveNode'
            this.showNodeMenu = false
            this.motionPauseTracker.pauseNode(this.nodeMenuNodeId)
          })
      }
    }
  }

  // ─── @Builder：新建菜单（bindMenu 内容） ───

  /**
   * 空白处新建菜单内容：只有"新建节点"和"新建子星图"两项。
   * 点击后先关菜单，再打开窗口层级的对话框（不会被 Embed 的 clip 裁掉）。
   */
  @Builder
  CreateMenuContent() {
    Menu() {
      MenuItem({ content: $r('app.string.create_node') })
        .onClick(() => {
          this.showCreateMenu = false
          this.openCreateNodeDialog()
        })

      MenuItem({ content: $r('app.string.create_embed') })
        .onClick(() => {
          this.showCreateMenu = false
          this.openCreateEmbedDialog()
        })
    }
  }

  // ─── 辅助方法 ───

  /**
   * 获取当前星图所有 Embed 的 instanceId 集合，用于 hitTestWithScene 区分节点和 Embed。
   */
  private getEmbedInstanceIds(): Set<string> {
    if (!this.graph) return new Set()
    const ids = new Set<string>()
    for (const embed of this.graph.embeds) {
      ids.add(embed.instanceId)
    }
    return ids
  }

  // ─── build ───

  /**
   * Scene 尺寸变化时的适配收尾。
   *
   * 第一次到位 = 首次适配；之后尺寸变化 = 父层缩放带动容器变大变小，
   * 要把已有适配同比带过去，而不是重新适配（重新适配会重置用户刚捏出来的缩放）。
   */
  private handleSceneAreaChange(newArea: Area): void {
    this.sceneWidth = Number(newArea.width)
    this.sceneHeight = Number(newArea.height)
    if (this.hasFittedView) {
      this.syncFitToSceneSize()
    } else {
      this.maybeFitView()
    }
    // 容器尺寸变了：本 Scene 的累计比例变了，Embed 的展开/折叠重算（#820）。
    // 根视口（rootViewportWidth / Height）的变化另有 onRootViewportChange 接。
    this.refreshEmbedLodLayout()
  }

  build() {
    // 只有根 Scene 挂高优先级双指缩放（#818 复审）。
    // child Scene 不挂：它不是独立窗口，双指缩的是整棵树的视角，
    // 在每层都挂一份只会让相机被同一双手指改好几次。
    if (this.isCameraScene()) {
      this.RootSceneStack()
    } else {
      this.ChildSceneStack()
    }
  }

  /** 根 Scene 的最外层 Stack：唯一挂高优先级 PinchGesture 的地方。 */
  @Builder
  RootSceneStack() {
    Stack() {
      this.SceneLayers()
    }
    .width('100%')
    .height('100%')
    // priorityGesture 让父组件优先识别这个手势，抢在节点 / Embed / 空白各自的
    // Tap、LongPress、Pan 之前。普通 gesture() 是反过来：子组件优先（#818 复审）。
    .priorityGesture(
      PinchGesture({ fingers: 2, distance: 1 })
        .onActionStart((event: GestureEvent) => {
          this.beginPriorityPinch(event)
        })
        .onActionUpdate((event: GestureEvent) => {
          this.updatePriorityPinch(event)
        })
        .onActionEnd(() => {
          this.endUnifiedPinch()
        })
        .onActionCancel(() => {
          this.endUnifiedPinch()
        })
    )
    .onAreaChange((_oldArea: Area, newArea: Area) => {
      this.handleSceneAreaChange(newArea)
    })
    // 本 Scene 自己的稳定输入节点 id（#818 复审）：它是递归链的"子端"名字。
    // 注意和父层那颗 Embed 内壳的 id 不是同一个字符串：父层指名转发用的是
    // embedInputNodeId（starmap_embed_input_<scenePath>_<instanceId>），
    // 这里给的是本 Scene 壳自己的名字（starmap_embed_input_<scenePath>）。
    // 需要的只是"稳定 + 全树不重名"，不是两端字符串相同。
    .id(this.sceneInputNodeId())
  }

  /** 子 Scene 的最外层 Stack：没有独立相机，也没有独立双指缩放（#818 复审）。 */
  @Builder
  ChildSceneStack() {
    Stack() {
      this.SceneLayers()
    }
    .width('100%')
    .height('100%')
    .onAreaChange((_oldArea: Area, newArea: Area) => {
      this.handleSceneAreaChange(newArea)
    })
    .id(this.sceneInputNodeId())
  }

  /** 背景层、画布、节点、Embed、连线预览和菜单锚点，根层与子层完全一致。 */
  @Builder
  SceneLayers() {
      // 0. 背景命中层（只处理当前 Scene 空白区域的 Pinch/Pan/LongPress）
      // 位于 Stack 最底层，子组件在上层优先接收触摸事件
      // 空白区域的触摸事件穿透到此层
      Column() {}
      .width('100%')
      .height('100%')
      .gesture(
        // 空白区域手势：Pan（画布拖动）+ LongPress（长按新建）
        //
        // Pinch 不在这里（#816/#818 复审）：这个 Column 只是 Stack 里的一个兄弟节点，
        // 手指落在子星图上时根本收不到事件。双指统一走根 Scene 最外层 Stack 的
        // priorityGesture(PinchGesture)，那里才拿得到跨层的双指事件。
        GestureGroup(GestureMode.Parallel,
          TapGesture()
            .onAction((event: GestureEvent) => {
              // 空白层单击：选中递归命中的目标（含子层里的边），没有目标就清选中。
              // 节点 / Embed 在 Stack 上层，事件被它们收走，不会走到这里（#816）。
              const point = this.gestureScreenPoint(event, 0, 0)
              this.handleBlankTap(point.x, point.y)
            }),
          PanGesture()
            .onActionStart((event: GestureEvent) => {
              if (this.gestureTracker.isPinching()) return
              if (!this.gestureTracker.isOwnedByScene(this.scenePath) && !this.gestureTracker.isIdle()) return
              this.panActive = true
              const point = this.gestureScreenPoint(event, 0, 0)
              this.gestureTracker.beginPanCanvas(this.scenePath, point.x, point.y)
              this.gestureMode = 'panCanvas'
              this.panBaseCameraOffsetX = this.cameraOffsetX
              this.panBaseCameraOffsetY = this.cameraOffsetY
            })
            .onActionUpdate((event: GestureEvent) => {
              if (this.gestureTracker.isPinching()) return
              if (!this.gestureTracker.isOwnedByScene(this.scenePath)) return
              if (!this.panActive) return
              if (!this.canMutateViewport()) return
              if (this.gestureMode === 'panCanvas') {
                // 拖的是整张星图的视角；event.offset 是累计位移，根相机整份覆盖而不是叠加（#818 复审）
                this.applyCameraPanTo(this.panBaseCameraOffsetX + event.offsetX,
                  this.panBaseCameraOffsetY + event.offsetY)
              }
            })
            .onActionEnd(() => {
              // mode 还是 panCanvas 且归属本层才清全局；被 pinch 接管时不动它
              this.finishOwnedPan()
            })
            .onActionCancel(() => {
              this.finishOwnedPan()
            }),
          LongPressGesture({ repeat: false, duration: 500 })
            .onAction((event: GestureEvent) => {
              if (this.gestureTracker.isPinching()) return
              if (!this.gestureTracker.isOwnedByScene(this.scenePath) && !this.gestureTracker.isIdle()) return
              const point = this.gestureScreenPoint(event, 0, 0)
              const canvasPoint = screenToCanvas(point.x, point.y, this.viewportScaleValue(), this.viewportOffsetX(), this.viewportOffsetY())
              // 空白判定和选中/连线共用同一个递归命中：点在子星图内部时父 Scene 不该弹新建菜单
              const context: RecursiveSceneContext | null = this.getRootContext()
              let isBlank: boolean = false
              if (context !== null) {
                const rootPoint: PlatformPoint = convertPointToRoot(context, this.scenePath, point.x, point.y)
                isBlank = resolveRecursiveHit(context, rootPoint.x, rootPoint.y).target === null
              }
              // 空白区域长按 → 弹出新建菜单。
              // 菜单挂在 Scene 栈里的 1vp 锚点上（不挂在本背景层）：
              // bindMenu 按被绑定组件定位，挂全屏背景层会让菜单相对整张 Scene 放，
              // 不再相对手指长按的位置放（Issue #813 复审）。
              if (isBlank) {
                this.createMenuAnchor = { x: point.x, y: point.y }
                this.createMenuCanvasPosition = { x: canvasPoint.x, y: canvasPoint.y }
                this.showCreateMenu = true
              }
            })
        )
      )
      .hitTestBehavior(HitTestMode.Default)

      // 1. Canvas：绘制当前层的边（只负责画，不参与触摸测试）
      Canvas(this.canvasCtx)
        .width('100%')
        .height('100%')
        .hitTestBehavior(HitTestMode.None)
        .onReady(() => {
          this.canvasReady = true
          this.drawEdges()
        })

      // 2. 节点层
      if (this.graph) {
        ForEach(this.graph.nodes, (node: StarMapNode) => {
          this.StarMapNodeItem(node)
        }, (node: StarMapNode) => node.id)

        // 3. Embed 层
        ForEach(this.graph.embeds, (embed: StarMapEmbed) => {
          this.StarMapEmbedItem(embed)
        }, (embed: StarMapEmbed) => embed.instanceId)
      }

      // 4. 预览连线 Canvas（connect 模式，只负责画，不参与触摸测试）
      if (this.gestureMode === 'connect') {
        Canvas(this.previewCanvasCtx)
          .width('100%')
          .height('100%')
          .hitTestBehavior(HitTestMode.None)
          .onReady(() => {
            this.previewCanvasReady = true
            this.drawPreviewLine()
          })
      }

      // 5. 菜单锚点：1vp×1vp 透明锚点，hitTestBehavior(None) 保证它自己不吃事件，
      // 只用来给 bindMenu 提供"贴着手指弹出"的定位基准。
      // 菜单浮层由系统绘制并自动避让屏幕边缘，锚点本身在圆形 Embed 内也不会被 clip(true) 裁掉。
      // onDisappear 必须回写显示标记：程序化 bindMenu 的 isShow 不会自动同步，
      // 不回写的话第二次长按同一对象会因状态没变化而弹不出来（Issue #813 复审）。
      Row() {}
        .width(1)
        .height(1)
        .position({ x: this.nodeMenuAnchor.x, y: this.nodeMenuAnchor.y })
        .hitTestBehavior(HitTestMode.None)
        .bindMenu(this.showNodeMenu, this.NodeMenuContent, {
          onDisappear: () => {
            this.onNodeMenuDisappear()
          }
        })

      Row() {}
        .width(1)
        .height(1)
        .position({ x: this.createMenuAnchor.x, y: this.createMenuAnchor.y })
        .hitTestBehavior(HitTestMode.None)
        .bindMenu(this.showCreateMenu, this.CreateMenuContent, {
          onDisappear: () => {
            this.onCreateMenuDisappear()
          }
        })

      // 对话框不走这里：新建对话框挂在窗口层级（openCustomDialog）。
      // 菜单和对话框本体都不放进 Scene 的 Stack，自绘弹层会被圆形 Embed 的 clip(true) 裁掉（Issue #813）。
  }
}
