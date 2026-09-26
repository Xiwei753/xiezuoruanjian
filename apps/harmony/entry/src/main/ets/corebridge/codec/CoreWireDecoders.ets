// ─── Core Wire Decoders ───
// Issue #753：Harmony 侧运行时数据解码。
//
// decoder 只认 Core 的 canonical camelCase DTO，不兼容页面私有别名。
// 发现契约不一致（字段缺失、类型错误）直接返回 `{ ok: false, error }`，
// Bridge 层据此构造 `SCHEMA_MISMATCH` ResultEnvelope，阻止 undefined 流到 UI。
//
// 设计原则：
// 1. Core DTO 是唯一数据结构，decoder 逐字段校验类型和存在性。
// 2. 不做 `as T` 整体强转——那是导致闪退的根因（字段不匹配时 as 不报错，
//    实际是 undefined，UI 访问 undefined 属性就崩）。
// 3. 必填字段缺失或类型错误 → 返回 error；可选字段缺失 → 赋 undefined。
// 4. 嵌套对象递归 decode；数组逐元素 decode。

import { LocalSettings, SyncableSettings, ThemePaletteRecordDto, ThemeColorSchemeDto, BuiltinThemeDto } from '../dto/SettingsDtos'
import { Project, ProjectSummary, Volume, ChapterMeta, ChapterContent, ChapterSaveReceipt, ProjectTree, VolumeTree, ProjectStats, WritingStats } from '../dto/ProjectDtos'
import { StarMapMeta, StarMapGraph, StarMapNode, StarMapEdge, StarMapEmbed, StarMapLink, StarMapHyperlink, StarMapLayout, StarMapLayoutNode, StarMapEdgeRender, StarMapMotionPolicy, NodeType, EdgeKind } from '../dto/StarMapDtos'
import { SyncConfig, SyncState, SyncConflict, SyncResult, SyncDiagnosticsResult, FullSyncResult, FullSyncTargetResult, FullSyncDryRunResult, FullSyncTargetPlan, SyncPlan, FullSyncDiagnosticsResult, ProviderConfig } from '../dto/SyncDtos'
import { RecentEdit, AppSummary } from '../dto/ResultEnvelope'
import { LayoutContract, ScreenPolicy, ShellMode, WorkspaceLayoutMode, PrimaryNavigationPlacement, ScreenRole, ActionSlot, ActionRole, ActionTarget, ActionRegion, LayoutMetrics } from '../dto/PlatformDtos'

// ─── Decode Result ───

export interface DecodeResult<T> {
  ok: boolean
  value?: T
  error?: string
}

function ok<T>(value: T): DecodeResult<T> {
  return { ok: true, value: value }
}

function err<T>(message: string): DecodeResult<T> {
  return { ok: false, error: message }
}

// ─── Field Access Helpers ───
// null = 必填字段缺失或类型错误；undefined = 可选字段缺失。

function reqStr(obj: Object, key: string): string | null {
  const rec = obj as Record<string, Object>
  const v: Object | undefined = rec[key]
  if (typeof v === 'string') { return v }
  return null
}

function reqNum(obj: Object, key: string): number | null {
  const rec = obj as Record<string, Object>
  const v: Object | undefined = rec[key]
  if (typeof v === 'number') { return v }
  return null
}

function reqBool(obj: Object, key: string): boolean | null {
  const rec = obj as Record<string, Object>
  const v: Object | undefined = rec[key]
  if (typeof v === 'boolean') { return v }
  return null
}

function reqObj(obj: Object, key: string): Object | null {
  const rec = obj as Record<string, Object>
  const v: Object | undefined = rec[key]
  if (typeof v === 'object' && v !== null && !Array.isArray(v)) { return v }
  return null
}

// 可选对象字段。对应 Core 侧 `#[serde(default, skip_serializing_if = "Option::is_none")]`：
// canonical wire 只有两种合法情况——键不存在（合法缺省 → undefined）或键存在且为对象。
// 键存在但值为 null / 字符串 / 数字 / 数组都属于契约错误，返回 err 让 Bridge 层报
// SCHEMA_MISMATCH，不再把显式 null 静默吞成 undefined（Issue #753 评论 5815903726）。
function optObj(obj: Object, key: string): DecodeResult<Object | undefined> {
  const rec = obj as Record<string, Object>
  const v: Object | undefined = rec[key]
  if (v === undefined) { return ok<Object | undefined>(undefined) }
  if (v !== null && typeof v === 'object' && !Array.isArray(v)) { return ok<Object | undefined>(v) }
  return err<Object | undefined>(`${key}: expected object or absent, got ${v === null ? 'null' : typeof v}`)
}

function reqArray(obj: Object, key: string): Array<Object> | null {
  const rec = obj as Record<string, Object>
  const v: Object | undefined = rec[key]
  if (Array.isArray(v)) { return v as Array<Object> }
  return null
}

// ─── Nullable Field Helpers ───
// Core 侧 `Option<T>` 未标 `skip_serializing_if` 时，该键一定出现，值是 `T` 或 `null`。
// 这类字段在 ArkTS interface 上写 `T | null`，必须用 null* 取值：
// 用 opt* 会得到 undefined，赋给 `T | null` 就丢失了 null 语义，UI 读出来是空。
//
// 三态语义（Issue #753 评论 5814008597 问题 4）：
// - 键不存在（v === undefined）→ err('key missing')：契约不一致，上层报 SCHEMA_MISMATCH。
// - 键存在且为 null（v === null）→ ok(null)：合法的空值。
// - 键存在且为目标类型 → ok(value)。
// - 键存在但类型错误 → err('type error')：契约不一致。

function nullStr(obj: Object, key: string): DecodeResult<string | null> {
  const rec = obj as Record<string, Object>
  const v: Object | undefined = rec[key]
  if (v === undefined) { return err<string | null>(`${key} missing`) }
  if (v === null) { return ok<string | null>(null) }
  if (typeof v === 'string') { return ok<string | null>(v) }
  return err<string | null>(`${key}: expected string, got ${typeof v}`)
}

function nullNum(obj: Object, key: string): DecodeResult<number | null> {
  const rec = obj as Record<string, Object>
  const v: Object | undefined = rec[key]
  if (v === undefined) { return err<number | null>(`${key} missing`) }
  if (v === null) { return ok<number | null>(null) }
  if (typeof v === 'number') { return ok<number | null>(v) }
  return err<number | null>(`${key}: expected number, got ${typeof v}`)
}

function nullObj(obj: Object, key: string): DecodeResult<Object | null> {
  const rec = obj as Record<string, Object>
  const v: Object | undefined = rec[key]
  if (v === undefined) { return err<Object | null>(`${key} missing`) }
  if (v === null) { return ok<Object | null>(null) }
  if (typeof v === 'object' && !Array.isArray(v)) { return ok<Object | null>(v) }
  return err<Object | null>(`${key}: expected object, got ${typeof v}`)
}

function nullArray(obj: Object, key: string): DecodeResult<Array<Object> | null> {
  const rec = obj as Record<string, Object>
  const v: Object | undefined = rec[key]
  if (v === undefined) { return err<Array<Object> | null>(`${key} missing`) }
  if (v === null) { return ok<Array<Object> | null>(null) }
  if (Array.isArray(v)) { return ok<Array<Object> | null>(v as Array<Object>) }
  return err<Array<Object> | null>(`${key}: expected array, got ${typeof v}`)
}

// 非空 `string[]` 字段：Core 恒发该键，任一元素不是字符串都算契约不一致。
function reqStrArray(obj: Object, key: string): string[] | null {
  const arr: Array<Object> | null = reqArray(obj, key)
  if (arr === null) { return null }
  const out: string[] = []
  for (let i = 0; i < arr.length; i++) {
    const v: Object = arr[i]
    if (typeof v !== 'string') { return null }
    out.push(v as string)
  }
  return out
}

// ─── Generic Array Decoder ───

export function decodeArray<T>(data: Object, decodeItem: (item: Object) => DecodeResult<T>, typeName: string): DecodeResult<T[]> {
  if (!Array.isArray(data)) { return err<T[]>(`expected array for ${typeName}`) }
  const src: Array<Object> = data as Array<Object>
  const arr: T[] = []
  for (let i = 0; i < src.length; i++) {
    const itemR: DecodeResult<T> = decodeItem(src[i])
    if (!itemR.ok || itemR.value === undefined) {
      return err<T[]>(`${typeName}[${i}]: ${itemR.error ?? 'decode failed'}`)
    }
    arr.push(itemR.value)
  }
  return ok<T[]>(arr)
}

// ─── Nullable Nested DTO Decoder ───
// Core 侧 `Option<Dto>` 未标 skip_serializing_if 时，键一定出现：值为对象或 null。
// 值为 null 时直接返回 null，不调用子 decoder —— 这样嵌套 DTO 也不会把 undefined 漏给 UI。

export function decodeOptional<T>(data: Object, key: string, decodeItem: (item: Object) => DecodeResult<T>): DecodeResult<T | null> {
  const innerR: DecodeResult<Object | null> = nullObj(data, key)
  if (!innerR.ok || innerR.value === undefined) { return err<T | null>(`${key}: ${innerR.error ?? 'decode failed'}`) }
  if (innerR.value === null) { return ok<T | null>(null) }
  const decoded: DecodeResult<T> = decodeItem(innerR.value)
  if (!decoded.ok || decoded.value === undefined) {
    return err<T | null>(decoded.error ?? 'decode failed')
  }
  return ok<T | null>(decoded.value)
}

// ─── Settings Decoders ───

export function decodeLocalSettings(data: Object): DecodeResult<LocalSettings> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<LocalSettings>('LocalSettings: expected object')
  }
  const appearanceMode: string | null = reqStr(data, 'appearanceMode')
  if (appearanceMode === null) { return err<LocalSettings>('LocalSettings.appearanceMode missing or not string') }
  const colorSource: string | null = reqStr(data, 'colorSource')
  if (colorSource === null) { return err<LocalSettings>('LocalSettings.colorSource missing or not string') }
  const dynamicColorEnabled: boolean | null = reqBool(data, 'dynamicColorEnabled')
  if (dynamicColorEnabled === null) { return err<LocalSettings>('LocalSettings.dynamicColorEnabled missing or not boolean') }
  const selectedBuiltinThemeId: string | null = reqStr(data, 'selectedBuiltinThemeId')
  if (selectedBuiltinThemeId === null) { return err<LocalSettings>('LocalSettings.selectedBuiltinThemeId missing or not string') }
  const selectedPaletteId: string | null = reqStr(data, 'selectedPaletteId')
  if (selectedPaletteId === null) { return err<LocalSettings>('LocalSettings.selectedPaletteId missing or not string') }
  const autoSaveEnabled: boolean | null = reqBool(data, 'autoSaveEnabled')
  if (autoSaveEnabled === null) { return err<LocalSettings>('LocalSettings.autoSaveEnabled missing or not boolean') }
  const editorFontSize: number | null = reqNum(data, 'editorFontSize')
  if (editorFontSize === null) { return err<LocalSettings>('LocalSettings.editorFontSize missing or not number') }
  const editorLineSpacingMultiplier: number | null = reqNum(data, 'editorLineSpacingMultiplier')
  if (editorLineSpacingMultiplier === null) { return err<LocalSettings>('LocalSettings.editorLineSpacingMultiplier missing or not number') }
  const windowWidth: number | null = reqNum(data, 'windowWidth')
  if (windowWidth === null) { return err<LocalSettings>('LocalSettings.windowWidth missing or not number') }
  const windowHeight: number | null = reqNum(data, 'windowHeight')
  if (windowHeight === null) { return err<LocalSettings>('LocalSettings.windowHeight missing or not number') }
  const autoSaveDelayMs: number | null = reqNum(data, 'autoSaveDelayMs')
  if (autoSaveDelayMs === null) { return err<LocalSettings>('LocalSettings.autoSaveDelayMs missing or not number') }
  const autoIndentEnabled: boolean | null = reqBool(data, 'autoIndentEnabled')
  if (autoIndentEnabled === null) { return err<LocalSettings>('LocalSettings.autoIndentEnabled missing or not boolean') }
  const autoIndentWidth: number | null = reqNum(data, 'autoIndentWidth')
  if (autoIndentWidth === null) { return err<LocalSettings>('LocalSettings.autoIndentWidth missing or not number') }
  const editorTypingAnimationEnabled: boolean | null = reqBool(data, 'editorTypingAnimationEnabled')
  if (editorTypingAnimationEnabled === null) { return err<LocalSettings>('LocalSettings.editorTypingAnimationEnabled missing or not boolean') }
  const editorSmoothCursorEnabled: boolean | null = reqBool(data, 'editorSmoothCursorEnabled')
  if (editorSmoothCursorEnabled === null) { return err<LocalSettings>('LocalSettings.editorSmoothCursorEnabled missing or not boolean') }
  const editorTypingAnimationDurationMs: number | null = reqNum(data, 'editorTypingAnimationDurationMs')
  if (editorTypingAnimationDurationMs === null) { return err<LocalSettings>('LocalSettings.editorTypingAnimationDurationMs missing or not number') }
  const editorSmoothCursorDurationMs: number | null = reqNum(data, 'editorSmoothCursorDurationMs')
  if (editorSmoothCursorDurationMs === null) { return err<LocalSettings>('LocalSettings.editorSmoothCursorDurationMs missing or not number') }
  const aiEnabled: boolean | null = reqBool(data, 'aiEnabled')
  if (aiEnabled === null) { return err<LocalSettings>('LocalSettings.aiEnabled missing or not boolean') }
  const desktopSidebarWidth: number | null = reqNum(data, 'desktopSidebarWidth')
  if (desktopSidebarWidth === null) { return err<LocalSettings>('LocalSettings.desktopSidebarWidth missing or not number') }
  const desktopEditorWidth: number | null = reqNum(data, 'desktopEditorWidth')
  if (desktopEditorWidth === null) { return err<LocalSettings>('LocalSettings.desktopEditorWidth missing or not number') }
  const editorCoordinatedTextCursorAnimationEnabled: boolean | null = reqBool(data, 'editorCoordinatedTextCursorAnimationEnabled')
  if (editorCoordinatedTextCursorAnimationEnabled === null) { return err<LocalSettings>('LocalSettings.editorCoordinatedTextCursorAnimationEnabled missing or not boolean') }
  const diagnosticsEnabled: boolean | null = reqBool(data, 'diagnosticsEnabled')
  if (diagnosticsEnabled === null) { return err<LocalSettings>('LocalSettings.diagnosticsEnabled missing or not boolean') }
  const diagnosticsVerbose: boolean | null = reqBool(data, 'diagnosticsVerbose')
  if (diagnosticsVerbose === null) { return err<LocalSettings>('LocalSettings.diagnosticsVerbose missing or not boolean') }

  const themeModeR: DecodeResult<string | null> = nullStr(data, 'themeMode')
  if (!themeModeR.ok || themeModeR.value === undefined) { return err<LocalSettings>(`LocalSettings.${themeModeR.error ?? 'themeMode decode failed'}`) }
  const localeR: DecodeResult<string | null> = nullStr(data, 'locale')
  if (!localeR.ok || localeR.value === undefined) { return err<LocalSettings>(`LocalSettings.${localeR.error ?? 'locale decode failed'}`) }
  const statsDeviceIdR: DecodeResult<string | null> = nullStr(data, 'statsDeviceId')
  if (!statsDeviceIdR.ok || statsDeviceIdR.value === undefined) { return err<LocalSettings>(`LocalSettings.${statsDeviceIdR.error ?? 'statsDeviceId decode failed'}`) }

  const value: LocalSettings = {
    themeMode: themeModeR.value,
    appearanceMode: appearanceMode,
    colorSource: colorSource,
    dynamicColorEnabled: dynamicColorEnabled,
    selectedBuiltinThemeId: selectedBuiltinThemeId,
    selectedPaletteId: selectedPaletteId,
    locale: localeR.value,
    autoSaveEnabled: autoSaveEnabled,
    editorFontSize: editorFontSize,
    editorLineSpacingMultiplier: editorLineSpacingMultiplier,
    windowWidth: windowWidth,
    windowHeight: windowHeight,
    autoSaveDelayMs: autoSaveDelayMs,
    autoIndentEnabled: autoIndentEnabled,
    autoIndentWidth: autoIndentWidth,
    editorTypingAnimationEnabled: editorTypingAnimationEnabled,
    editorSmoothCursorEnabled: editorSmoothCursorEnabled,
    editorTypingAnimationDurationMs: editorTypingAnimationDurationMs,
    editorSmoothCursorDurationMs: editorSmoothCursorDurationMs,
    aiEnabled: aiEnabled,
    statsDeviceId: statsDeviceIdR.value,
    desktopSidebarWidth: desktopSidebarWidth,
    desktopEditorWidth: desktopEditorWidth,
    editorCoordinatedTextCursorAnimationEnabled: editorCoordinatedTextCursorAnimationEnabled,
    diagnosticsEnabled: diagnosticsEnabled,
    diagnosticsVerbose: diagnosticsVerbose
  }
  return ok<LocalSettings>(value)
}

export function decodeSyncableSettings(data: Object): DecodeResult<SyncableSettings> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<SyncableSettings>('SyncableSettings: expected object')
  }
  const fontSize: number | null = reqNum(data, 'fontSize')
  if (fontSize === null) { return err<SyncableSettings>('SyncableSettings.fontSize missing or not number') }
  const themeMode: string | null = reqStr(data, 'themeMode')
  if (themeMode === null) { return err<SyncableSettings>('SyncableSettings.themeMode missing or not string') }
  const monetColor: string | null = reqStr(data, 'monetColor')
  if (monetColor === null) { return err<SyncableSettings>('SyncableSettings.monetColor missing or not string') }
  const themePaletteJson: string | null = reqStr(data, 'themePaletteJson')
  if (themePaletteJson === null) { return err<SyncableSettings>('SyncableSettings.themePaletteJson missing or not string') }
  const value: SyncableSettings = {
    fontSize: fontSize,
    themeMode: themeMode,
    monetColor: monetColor,
    themePaletteJson: themePaletteJson
  }
  return ok<SyncableSettings>(value)
}

// ─── Theme Palette Decoders ───

function decodeThemeColorScheme(data: Object): DecodeResult<ThemeColorSchemeDto> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<ThemeColorSchemeDto>('ThemeColorScheme: expected object')
  }
  const required: string[] = [
    'primary', 'on_primary', 'primary_container', 'on_primary_container', 'inverse_primary',
    'secondary', 'on_secondary', 'secondary_container', 'on_secondary_container',
    'tertiary', 'on_tertiary', 'tertiary_container', 'on_tertiary_container',
    'background', 'on_background', 'surface', 'on_surface', 'surface_variant', 'on_surface_variant',
    'surface_tint', 'surface_dim', 'surface_bright',
    'surface_container_lowest', 'surface_container_low', 'surface_container',
    'surface_container_high', 'surface_container_highest',
    'inverse_surface', 'inverse_on_surface',
    'error', 'on_error', 'error_container', 'on_error_container',
    'outline', 'outline_variant', 'scrim',
    'primary_fixed', 'primary_fixed_dim', 'on_primary_fixed', 'on_primary_fixed_variant',
    'secondary_fixed', 'secondary_fixed_dim', 'on_secondary_fixed', 'on_secondary_fixed_variant',
    'tertiary_fixed', 'tertiary_fixed_dim', 'on_tertiary_fixed', 'on_tertiary_fixed_variant'
  ]
  const rec: Record<string, string> = {} as Record<string, string>
  for (let i = 0; i < required.length; i++) {
    const key: string = required[i]
    const v: string | null = reqStr(data, key)
    if (v === null) { return err<ThemeColorSchemeDto>(`ThemeColorScheme.${key} missing or not string`) }
    rec[key] = v
  }
  const value: ThemeColorSchemeDto = rec as Object as ThemeColorSchemeDto
  return ok<ThemeColorSchemeDto>(value)
}

export function decodeThemePaletteRecord(data: Object): DecodeResult<ThemePaletteRecordDto> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<ThemePaletteRecordDto>('ThemePaletteRecord: expected object')
  }
  const schemaVersion: number | null = reqNum(data, 'schema_version')
  if (schemaVersion === null) { return err<ThemePaletteRecordDto>('schema_version missing or not number') }
  const paletteId: string | null = reqStr(data, 'palette_id')
  if (paletteId === null) { return err<ThemePaletteRecordDto>('palette_id missing or not string') }
  const paletteFingerprint: string | null = reqStr(data, 'palette_fingerprint')
  if (paletteFingerprint === null) { return err<ThemePaletteRecordDto>('palette_fingerprint missing or not string') }
  const source: string | null = reqStr(data, 'source')
  if (source === null) { return err<ThemePaletteRecordDto>('source missing or not string') }
  const sourcePlatform: string | null = reqStr(data, 'source_platform')
  if (sourcePlatform === null) { return err<ThemePaletteRecordDto>('source_platform missing or not string') }
  const sourceDeviceId: string | null = reqStr(data, 'source_device_id')
  if (sourceDeviceId === null) { return err<ThemePaletteRecordDto>('source_device_id missing or not string') }
  const sourceDeviceClass: string | null = reqStr(data, 'source_device_class')
  if (sourceDeviceClass === null) { return err<ThemePaletteRecordDto>('source_device_class missing or not string') }
  const capturedAtMs: number | null = reqNum(data, 'captured_at_ms')
  if (capturedAtMs === null) { return err<ThemePaletteRecordDto>('captured_at_ms missing or not number') }
  const variant: string | null = reqStr(data, 'variant')
  if (variant === null) { return err<ThemePaletteRecordDto>('variant missing or not string') }
  const lightSchemeObj: Object | null = reqObj(data, 'light_scheme')
  if (lightSchemeObj === null) { return err<ThemePaletteRecordDto>('light_scheme missing or not object') }
  const lightR: DecodeResult<ThemeColorSchemeDto> = decodeThemeColorScheme(lightSchemeObj)
  if (!lightR.ok || lightR.value === undefined) { return err<ThemePaletteRecordDto>(`light_scheme: ${lightR.error}`) }
  const darkSchemeObj: Object | null = reqObj(data, 'dark_scheme')
  if (darkSchemeObj === null) { return err<ThemePaletteRecordDto>('dark_scheme missing or not object') }
  const darkR: DecodeResult<ThemeColorSchemeDto> = decodeThemeColorScheme(darkSchemeObj)
  if (!darkR.ok || darkR.value === undefined) { return err<ThemePaletteRecordDto>(`dark_scheme: ${darkR.error}`) }
  const value: ThemePaletteRecordDto = {
    schema_version: schemaVersion,
    palette_id: paletteId,
    palette_fingerprint: paletteFingerprint,
    source: source,
    source_platform: sourcePlatform,
    source_device_id: sourceDeviceId,
    source_device_class: sourceDeviceClass,
    captured_at_ms: capturedAtMs,
    variant: variant,
    light_scheme: lightR.value,
    dark_scheme: darkR.value
  }
  return ok<ThemePaletteRecordDto>(value)
}

export function decodeBuiltinTheme(data: Object): DecodeResult<BuiltinThemeDto> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<BuiltinThemeDto>('BuiltinTheme: expected object')
  }
  const themeId: string | null = reqStr(data, 'theme_id')
  if (themeId === null) { return err<BuiltinThemeDto>('theme_id missing or not string') }
  const name: string | null = reqStr(data, 'name')
  if (name === null) { return err<BuiltinThemeDto>('name missing or not string') }
  const lightSchemeObj: Object | null = reqObj(data, 'light_scheme')
  if (lightSchemeObj === null) { return err<BuiltinThemeDto>('light_scheme missing or not object') }
  const lightR: DecodeResult<ThemeColorSchemeDto> = decodeThemeColorScheme(lightSchemeObj)
  if (!lightR.ok || lightR.value === undefined) { return err<BuiltinThemeDto>(`light_scheme: ${lightR.error}`) }
  const darkSchemeObj: Object | null = reqObj(data, 'dark_scheme')
  if (darkSchemeObj === null) { return err<BuiltinThemeDto>('dark_scheme missing or not object') }
  const darkR: DecodeResult<ThemeColorSchemeDto> = decodeThemeColorScheme(darkSchemeObj)
  if (!darkR.ok || darkR.value === undefined) { return err<BuiltinThemeDto>(`dark_scheme: ${darkR.error}`) }
  const value: BuiltinThemeDto = {
    theme_id: themeId,
    name: name,
    light_scheme: lightR.value,
    dark_scheme: darkR.value
  }
  return ok<BuiltinThemeDto>(value)
}

// ─── Project / Volume / Chapter Decoders ───

export function decodeProject(data: Object): DecodeResult<Project> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<Project>('Project: expected object')
  }
  const id: string | null = reqStr(data, 'id')
  if (id === null) { return err<Project>('Project.id missing or not string') }
  const title: string | null = reqStr(data, 'title')
  if (title === null) { return err<Project>('Project.title missing or not string') }
  const createdAt: string | null = reqStr(data, 'createdAt')
  if (createdAt === null) { return err<Project>('Project.createdAt missing or not string') }
  const updatedAt: string | null = reqStr(data, 'updatedAt')
  if (updatedAt === null) { return err<Project>('Project.updatedAt missing or not string') }
  return ok<Project>({ id: id, title: title, createdAt: createdAt, updatedAt: updatedAt })
}

export function decodeProjectSummary(data: Object): DecodeResult<ProjectSummary> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<ProjectSummary>('ProjectSummary: expected object')
  }
  const id: string | null = reqStr(data, 'id')
  if (id === null) { return err<ProjectSummary>('ProjectSummary.id missing or not string') }
  const title: string | null = reqStr(data, 'title')
  if (title === null) { return err<ProjectSummary>('ProjectSummary.title missing or not string') }
  const createdAt: string | null = reqStr(data, 'createdAt')
  if (createdAt === null) { return err<ProjectSummary>('ProjectSummary.createdAt missing or not string') }
  const updatedAt: string | null = reqStr(data, 'updatedAt')
  if (updatedAt === null) { return err<ProjectSummary>('ProjectSummary.updatedAt missing or not string') }
  const totalWordCount: number | null = reqNum(data, 'totalWordCount')
  if (totalWordCount === null) { return err<ProjectSummary>('ProjectSummary.totalWordCount missing or not number') }
  const volumeCount: number | null = reqNum(data, 'volumeCount')
  if (volumeCount === null) { return err<ProjectSummary>('ProjectSummary.volumeCount missing or not number') }
  const chapterCount: number | null = reqNum(data, 'chapterCount')
  if (chapterCount === null) { return err<ProjectSummary>('ProjectSummary.chapterCount missing or not number') }
  return ok<ProjectSummary>({
    id: id,
    title: title,
    createdAt: createdAt,
    updatedAt: updatedAt,
    totalWordCount: totalWordCount,
    volumeCount: volumeCount,
    chapterCount: chapterCount
  })
}

export function decodeVolume(data: Object): DecodeResult<Volume> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<Volume>('Volume: expected object')
  }
  const id: string | null = reqStr(data, 'id')
  if (id === null) { return err<Volume>('Volume.id missing or not string') }
  const title: string | null = reqStr(data, 'title')
  if (title === null) { return err<Volume>('Volume.title missing or not string') }
  const createdAt: string | null = reqStr(data, 'createdAt')
  if (createdAt === null) { return err<Volume>('Volume.createdAt missing or not string') }
  const updatedAt: string | null = reqStr(data, 'updatedAt')
  if (updatedAt === null) { return err<Volume>('Volume.updatedAt missing or not string') }
  const order: number | null = reqNum(data, 'order')
  if (order === null) { return err<Volume>('Volume.order missing or not number') }
  return ok<Volume>({ id: id, title: title, createdAt: createdAt, updatedAt: updatedAt, order: order })
}

export function decodeChapterMeta(data: Object): DecodeResult<ChapterMeta> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<ChapterMeta>('ChapterMeta: expected object')
  }
  const id: string | null = reqStr(data, 'id')
  if (id === null) { return err<ChapterMeta>('ChapterMeta.id missing or not string') }
  const title: string | null = reqStr(data, 'title')
  if (title === null) { return err<ChapterMeta>('ChapterMeta.title missing or not string') }
  const createdAt: string | null = reqStr(data, 'createdAt')
  if (createdAt === null) { return err<ChapterMeta>('ChapterMeta.createdAt missing or not string') }
  const updatedAt: string | null = reqStr(data, 'updatedAt')
  if (updatedAt === null) { return err<ChapterMeta>('ChapterMeta.updatedAt missing or not string') }
  const order: number | null = reqNum(data, 'order')
  if (order === null) { return err<ChapterMeta>('ChapterMeta.order missing or not number') }
  const wordCount: number | null = reqNum(data, 'wordCount')
  if (wordCount === null) { return err<ChapterMeta>('ChapterMeta.wordCount missing or not number') }
  const hash: string | null = reqStr(data, 'hash')
  if (hash === null) { return err<ChapterMeta>('ChapterMeta.hash missing or not string') }
  const noteR: DecodeResult<string | null> = nullStr(data, 'note')
  if (!noteR.ok || noteR.value === undefined) { return err<ChapterMeta>(`ChapterMeta.${noteR.error ?? 'decode failed'}`) }
  const value: ChapterMeta = {
    id: id,
    title: title,
    createdAt: createdAt,
    updatedAt: updatedAt,
    order: order,
    wordCount: wordCount,
    hash: hash,
    note: noteR.value
  }
  return ok<ChapterMeta>(value)
}

export function decodeChapterContent(data: Object): DecodeResult<ChapterContent> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<ChapterContent>('ChapterContent: expected object')
  }
  const metaObj: Object | null = reqObj(data, 'meta')
  if (metaObj === null) { return err<ChapterContent>('ChapterContent.meta missing or not object') }
  const metaR: DecodeResult<ChapterMeta> = decodeChapterMeta(metaObj)
  if (!metaR.ok || metaR.value === undefined) { return err<ChapterContent>(`meta: ${metaR.error}`) }
  const content: string | null = reqStr(data, 'content')
  if (content === null) { return err<ChapterContent>('ChapterContent.content missing or not string') }
  return ok<ChapterContent>({ meta: metaR.value, content: content })
}

export function decodeChapterSaveReceipt(data: Object): DecodeResult<ChapterSaveReceipt> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<ChapterSaveReceipt>('ChapterSaveReceipt: expected object')
  }
  const chapterRelativePath: string | null = reqStr(data, 'chapterRelativePath')
  if (chapterRelativePath === null) { return err<ChapterSaveReceipt>('chapterRelativePath missing or not string') }
  const contentLen: number | null = reqNum(data, 'contentLen')
  if (contentLen === null) { return err<ChapterSaveReceipt>('contentLen missing or not number') }
  const contentHash: string | null = reqStr(data, 'contentHash')
  if (contentHash === null) { return err<ChapterSaveReceipt>('contentHash missing or not string') }
  const metaHash: string | null = reqStr(data, 'metaHash')
  if (metaHash === null) { return err<ChapterSaveReceipt>('metaHash missing or not string') }
  const updatedAt: string | null = reqStr(data, 'updatedAt')
  if (updatedAt === null) { return err<ChapterSaveReceipt>('updatedAt missing or not string') }
  const wordCount: number | null = reqNum(data, 'wordCount')
  if (wordCount === null) { return err<ChapterSaveReceipt>('wordCount missing or not number') }
  const value: ChapterSaveReceipt = {
    chapterRelativePath: chapterRelativePath,
    contentLen: contentLen,
    contentHash: contentHash,
    metaHash: metaHash,
    updatedAt: updatedAt,
    wordCount: wordCount
  }
  return ok<ChapterSaveReceipt>(value)
}

export function decodeProjectStats(data: Object): DecodeResult<ProjectStats> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<ProjectStats>('ProjectStats: expected object')
  }
  const totalWordCount: number | null = reqNum(data, 'totalWordCount')
  if (totalWordCount === null) { return err<ProjectStats>('totalWordCount missing or not number') }
  const volumeCount: number | null = reqNum(data, 'volumeCount')
  if (volumeCount === null) { return err<ProjectStats>('volumeCount missing or not number') }
  const chapterCount: number | null = reqNum(data, 'chapterCount')
  if (chapterCount === null) { return err<ProjectStats>('chapterCount missing or not number') }
  return ok<ProjectStats>({ totalWordCount: totalWordCount, volumeCount: volumeCount, chapterCount: chapterCount })
}

export function decodeWritingStats(data: Object): DecodeResult<WritingStats> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<WritingStats>('WritingStats: expected object')
  }
  const rangeObj: Object | null = reqObj(data, 'range')
  if (rangeObj === null) { return err<WritingStats>('range missing or not object') }
  const startDate: string | null = reqStr(rangeObj, 'startDate')
  if (startDate === null) { return err<WritingStats>('range.startDate missing or not string') }
  const endDate: string | null = reqStr(rangeObj, 'endDate')
  if (endDate === null) { return err<WritingStats>('range.endDate missing or not string') }
  const totalHumanTypedChars: number | null = reqNum(data, 'totalHumanTypedChars')
  if (totalHumanTypedChars === null) { return err<WritingStats>('totalHumanTypedChars missing or not number') }
  const totalPastedChars: number | null = reqNum(data, 'totalPastedChars')
  if (totalPastedChars === null) { return err<WritingStats>('totalPastedChars missing or not number') }
  const totalDeletedChars: number | null = reqNum(data, 'totalDeletedChars')
  if (totalDeletedChars === null) { return err<WritingStats>('totalDeletedChars missing or not number') }
  const totalAiInsertedChars: number | null = reqNum(data, 'totalAiInsertedChars')
  if (totalAiInsertedChars === null) { return err<WritingStats>('totalAiInsertedChars missing or not number') }
  const totalNetDeltaChars: number | null = reqNum(data, 'totalNetDeltaChars')
  if (totalNetDeltaChars === null) { return err<WritingStats>('totalNetDeltaChars missing or not number') }
  const totalActiveSeconds: number | null = reqNum(data, 'totalActiveSeconds')
  if (totalActiveSeconds === null) { return err<WritingStats>('totalActiveSeconds missing or not number') }
  const totalSessions: number | null = reqNum(data, 'totalSessions')
  if (totalSessions === null) { return err<WritingStats>('totalSessions missing or not number') }
  const daysCount: number | null = reqNum(data, 'daysCount')
  if (daysCount === null) { return err<WritingStats>('daysCount missing or not number') }
  const value: WritingStats = {
    range: { startDate: startDate, endDate: endDate },
    totalHumanTypedChars: totalHumanTypedChars,
    totalPastedChars: totalPastedChars,
    totalDeletedChars: totalDeletedChars,
    totalAiInsertedChars: totalAiInsertedChars,
    totalNetDeltaChars: totalNetDeltaChars,
    totalActiveSeconds: totalActiveSeconds,
    totalSessions: totalSessions,
    daysCount: daysCount
  }
  return ok<WritingStats>(value)
}

export function decodeProjectTree(data: Object): DecodeResult<ProjectTree> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<ProjectTree>('ProjectTree: expected object')
  }
  const projectObj: Object | null = reqObj(data, 'project')
  if (projectObj === null) { return err<ProjectTree>('project missing or not object') }
  const projectR: DecodeResult<Project> = decodeProject(projectObj)
  if (!projectR.ok || projectR.value === undefined) { return err<ProjectTree>(`project: ${projectR.error}`) }
  const volumesArr: Array<Object> | null = reqArray(data, 'volumes')
  if (volumesArr === null) { return err<ProjectTree>('volumes missing or not array') }
  const volumes: VolumeTree[] = []
  for (let i = 0; i < volumesArr.length; i++) {
    const vtObj: Object = volumesArr[i]
    if (typeof vtObj !== 'object' || vtObj === null || Array.isArray(vtObj)) {
      return err<ProjectTree>(`volumes[${i}]: not object`)
    }
    const volObj: Object | null = reqObj(vtObj, 'volume')
    if (volObj === null) { return err<ProjectTree>(`volumes[${i}].volume missing or not object`) }
    const volR: DecodeResult<Volume> = decodeVolume(volObj)
    if (!volR.ok || volR.value === undefined) { return err<ProjectTree>(`volumes[${i}].volume: ${volR.error}`) }
    const chArr: Array<Object> | null = reqArray(vtObj, 'chapters')
    if (chArr === null) { return err<ProjectTree>(`volumes[${i}].chapters missing or not array`) }
    const chR: DecodeResult<ChapterMeta[]> = decodeArray<ChapterMeta>(chArr as Object, decodeChapterMeta, `volumes[${i}].chapters`)
    if (!chR.ok || chR.value === undefined) { return err<ProjectTree>(chR.error ?? 'decode failed') }
    volumes.push({ volume: volR.value, chapters: chR.value })
  }
  const statsObj: Object | null = reqObj(data, 'stats')
  if (statsObj === null) { return err<ProjectTree>('stats missing or not object') }
  const statsR: DecodeResult<ProjectStats> = decodeProjectStats(statsObj)
  if (!statsR.ok || statsR.value === undefined) { return err<ProjectTree>(`stats: ${statsR.error}`) }
  const value: ProjectTree = {
    project: projectR.value,
    volumes: volumes,
    stats: statsR.value
  }
  return ok<ProjectTree>(value)
}

// ─── StarMap Decoders ───

export function decodeStarMapMeta(data: Object): DecodeResult<StarMapMeta> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<StarMapMeta>('StarMapMeta: expected object')
  }
  const starmapId: string | null = reqStr(data, 'starmapId')
  if (starmapId === null) { return err<StarMapMeta>('starmapId missing or not string') }
  const title: string | null = reqStr(data, 'title')
  if (title === null) { return err<StarMapMeta>('title missing or not string') }
  const description: string | null = reqStr(data, 'description')
  if (description === null) { return err<StarMapMeta>('description missing or not string') }
  const projectIdR: DecodeResult<string | null> = nullStr(data, 'projectId')
  if (!projectIdR.ok || projectIdR.value === undefined) { return err<StarMapMeta>(`StarMapMeta.${projectIdR.error ?? 'decode failed'}`) }
  const parentStarmapIdR: DecodeResult<string | null> = nullStr(data, 'parentStarmapId')
  if (!parentStarmapIdR.ok || parentStarmapIdR.value === undefined) { return err<StarMapMeta>(`StarMapMeta.${parentStarmapIdR.error ?? 'decode failed'}`) }
  const isMainForProject: boolean | null = reqBool(data, 'isMainForProject')
  if (isMainForProject === null) { return err<StarMapMeta>('isMainForProject missing or not boolean') }
  const accentColor: string | null = reqStr(data, 'accentColor')
  if (accentColor === null) { return err<StarMapMeta>('accentColor missing or not string') }
  const createdAt: number | null = reqNum(data, 'createdAt')
  if (createdAt === null) { return err<StarMapMeta>('createdAt missing or not number') }
  const updatedAt: number | null = reqNum(data, 'updatedAt')
  if (updatedAt === null) { return err<StarMapMeta>('updatedAt missing or not number') }
  const nodeCount: number | null = reqNum(data, 'nodeCount')
  if (nodeCount === null) { return err<StarMapMeta>('nodeCount missing or not number') }
  const edgeCount: number | null = reqNum(data, 'edgeCount')
  if (edgeCount === null) { return err<StarMapMeta>('edgeCount missing or not number') }
  const linkedChapterCount: number | null = reqNum(data, 'linkedChapterCount')
  if (linkedChapterCount === null) { return err<StarMapMeta>('linkedChapterCount missing or not number') }
  const childStarmapCount: number | null = reqNum(data, 'childStarmapCount')
  if (childStarmapCount === null) { return err<StarMapMeta>('childStarmapCount missing or not number') }
  const value: StarMapMeta = {
    starmapId: starmapId,
    title: title,
    description: description,
    projectId: projectIdR.value,
    parentStarmapId: parentStarmapIdR.value,
    isMainForProject: isMainForProject,
    accentColor: accentColor,
    createdAt: createdAt,
    updatedAt: updatedAt,
    nodeCount: nodeCount,
    edgeCount: edgeCount,
    linkedChapterCount: linkedChapterCount,
    childStarmapCount: childStarmapCount
  }
  return ok<StarMapMeta>(value)
}

function decodeStarMapNodeContent(data: Object): DecodeResult<import('../dto/StarMapDtos').StarMapNodeContent> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err('StarMapNodeContent: expected object')
  }
  const type: string | null = reqStr(data, 'type')
  if (type === null) { return err('type missing') }
  const summaryR = nullStr(data, 'summary')
  if (!summaryR.ok || summaryR.value === undefined) { return err(`StarMapNodeContent.${summaryR.error ?? 'decode failed'}`) }
  const bodyR = nullStr(data, 'body')
  if (!bodyR.ok || bodyR.value === undefined) { return err(`StarMapNodeContent.${bodyR.error ?? 'decode failed'}`) }
  const projectIdR = nullStr(data, 'projectId')
  if (!projectIdR.ok || projectIdR.value === undefined) { return err(`StarMapNodeContent.${projectIdR.error ?? 'decode failed'}`) }
  const volumeIdR = nullStr(data, 'volumeId')
  if (!volumeIdR.ok || volumeIdR.value === undefined) { return err(`StarMapNodeContent.${volumeIdR.error ?? 'decode failed'}`) }
  const chapterIdR = nullStr(data, 'chapterId')
  if (!chapterIdR.ok || chapterIdR.value === undefined) { return err(`StarMapNodeContent.${chapterIdR.error ?? 'decode failed'}`) }
  const rangeStartR = nullNum(data, 'rangeStart')
  if (!rangeStartR.ok || rangeStartR.value === undefined) { return err(`StarMapNodeContent.${rangeStartR.error ?? 'decode failed'}`) }
  const rangeEndR = nullNum(data, 'rangeEnd')
  if (!rangeEndR.ok || rangeEndR.value === undefined) { return err(`StarMapNodeContent.${rangeEndR.error ?? 'decode failed'}`) }
  const entityTypeR = nullStr(data, 'entityType')
  if (!entityTypeR.ok || entityTypeR.value === undefined) { return err(`StarMapNodeContent.${entityTypeR.error ?? 'decode failed'}`) }
  const entityIdR = nullStr(data, 'entityId')
  if (!entityIdR.ok || entityIdR.value === undefined) { return err(`StarMapNodeContent.${entityIdR.error ?? 'decode failed'}`) }
  const uriR = nullStr(data, 'uri')
  if (!uriR.ok || uriR.value === undefined) { return err(`StarMapNodeContent.${uriR.error ?? 'decode failed'}`) }
  const labelR = nullStr(data, 'label')
  if (!labelR.ok || labelR.value === undefined) { return err(`StarMapNodeContent.${labelR.error ?? 'decode failed'}`) }
  const value: import('../dto/StarMapDtos').StarMapNodeContent = {
    type: type,
    summary: summaryR.value,
    body: bodyR.value,
    projectId: projectIdR.value,
    volumeId: volumeIdR.value,
    chapterId: chapterIdR.value,
    rangeStart: rangeStartR.value,
    rangeEnd: rangeEndR.value,
    entityType: entityTypeR.value,
    entityId: entityIdR.value,
    uri: uriR.value,
    label: labelR.value
  }; return ok(value)
}

function decodeStarMapAnchorTarget(data: Object): DecodeResult<import('../dto/StarMapDtos').StarMapAnchorTarget> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err('StarMapAnchorTarget: expected object')
  }
  const type: string | null = reqStr(data, 'type')
  if (type === null) { return err('type missing') }
  const projectIdR = nullStr(data, 'projectId')
  if (!projectIdR.ok || projectIdR.value === undefined) { return err(`StarMapAnchorTarget.${projectIdR.error ?? 'decode failed'}`) }
  const volumeIdR = nullStr(data, 'volumeId')
  if (!volumeIdR.ok || volumeIdR.value === undefined) { return err(`StarMapAnchorTarget.${volumeIdR.error ?? 'decode failed'}`) }
  const chapterIdR = nullStr(data, 'chapterId')
  if (!chapterIdR.ok || chapterIdR.value === undefined) { return err(`StarMapAnchorTarget.${chapterIdR.error ?? 'decode failed'}`) }
  const rangeStartR = nullNum(data, 'rangeStart')
  if (!rangeStartR.ok || rangeStartR.value === undefined) { return err(`StarMapAnchorTarget.${rangeStartR.error ?? 'decode failed'}`) }
  const rangeEndR = nullNum(data, 'rangeEnd')
  if (!rangeEndR.ok || rangeEndR.value === undefined) { return err(`StarMapAnchorTarget.${rangeEndR.error ?? 'decode failed'}`) }
  const entityIdR = nullStr(data, 'entityId')
  if (!entityIdR.ok || entityIdR.value === undefined) { return err(`StarMapAnchorTarget.${entityIdR.error ?? 'decode failed'}`) }
  const entityTypeR = nullStr(data, 'entityType')
  if (!entityTypeR.ok || entityTypeR.value === undefined) { return err(`StarMapAnchorTarget.${entityTypeR.error ?? 'decode failed'}`) }
  const starmapIdR = nullStr(data, 'starmapId')
  if (!starmapIdR.ok || starmapIdR.value === undefined) { return err(`StarMapAnchorTarget.${starmapIdR.error ?? 'decode failed'}`) }
  const uriR = nullStr(data, 'uri')
  if (!uriR.ok || uriR.value === undefined) { return err(`StarMapAnchorTarget.${uriR.error ?? 'decode failed'}`) }
  const payloadR = nullStr(data, 'payload')
  if (!payloadR.ok || payloadR.value === undefined) { return err(`StarMapAnchorTarget.${payloadR.error ?? 'decode failed'}`) }
  const value: import('../dto/StarMapDtos').StarMapAnchorTarget = {
    type: type,
    projectId: projectIdR.value,
    volumeId: volumeIdR.value,
    chapterId: chapterIdR.value,
    rangeStart: rangeStartR.value,
    rangeEnd: rangeEndR.value,
    entityId: entityIdR.value,
    entityType: entityTypeR.value,
    starmapId: starmapIdR.value,
    uri: uriR.value,
    payload: payloadR.value
  }; return ok(value)
}

function decodeStarMapAnchor(data: Object): DecodeResult<import('../dto/StarMapDtos').StarMapAnchor> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err('StarMapAnchor: expected object')
  }
  const anchorId: string | null = reqStr(data, 'anchorId')
  if (anchorId === null) { return err('anchorId missing') }
  const targetObj: Object | null = reqObj(data, 'target')
  if (targetObj === null) { return err('target missing or not object') }
  const targetR = decodeStarMapAnchorTarget(targetObj)
  if (!targetR.ok || targetR.value === undefined) { return err(`target: ${targetR.error}`) }
  const role: string | null = reqStr(data, 'role')
  if (role === null) { return err('role missing') }
  const labelR = nullStr(data, 'label')
  if (!labelR.ok || labelR.value === undefined) { return err(`StarMapAnchor.${labelR.error ?? 'decode failed'}`) }
  const value: import('../dto/StarMapDtos').StarMapAnchor = {
    anchorId: anchorId,
    target: targetR.value,
    label: labelR.value,
    role: role as import('../dto/StarMapDtos').AnchorRole
  }; return ok(value)
}

function decodeStarMapPortal(data: Object): DecodeResult<import('../dto/StarMapDtos').StarMapPortal> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err('StarMapPortal: expected object')
  }
  const targetStarmapId: string | null = reqStr(data, 'targetStarmapId')
  if (targetStarmapId === null) { return err('targetStarmapId missing') }
  const mode: string | null = reqStr(data, 'mode')
  if (mode === null) { return err('mode missing') }
  const previewPolicy: string | null = reqStr(data, 'previewPolicy')
  if (previewPolicy === null) { return err('previewPolicy missing') }
  const deepTargetObjR: DecodeResult<Object | null> = nullObj(data, 'deepTarget')
  if (!deepTargetObjR.ok || deepTargetObjR.value === undefined) { return err(`StarMapPortal.${deepTargetObjR.error ?? 'decode failed'}`) }
  let deepTarget: import('../dto/StarMapDtos').StarMapDeepTarget | null = null
  if (deepTargetObjR.value !== null) {
    const deepTargetR = decodeStarMapDeepTarget(deepTargetObjR.value)
    if (!deepTargetR.ok || deepTargetR.value === undefined) { return err(`deepTarget: ${deepTargetR.error}`) }
    deepTarget = deepTargetR.value
  }
  const value: import('../dto/StarMapDtos').StarMapPortal = {
    targetStarmapId: targetStarmapId,
    deepTarget: deepTarget,
    mode: mode as import('../dto/StarMapDtos').PortalMode,
    previewPolicy: previewPolicy as import('../dto/StarMapDtos').PortalPreviewPolicy
  }; return ok(value)
}

function decodeStarMapDisplayPolicy(data: Object): DecodeResult<import('../dto/StarMapDtos').StarMapDisplayPolicy> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err('StarMapDisplayPolicy: expected object')
  }
  const importance: number | null = reqNum(data, 'importance')
  if (importance === null) { return err('importance missing') }
  const minVisibleScale: number | null = reqNum(data, 'minVisibleScale')
  if (minVisibleScale === null) { return err('minVisibleScale missing') }
  const titleScale: number | null = reqNum(data, 'titleScale')
  if (titleScale === null) { return err('titleScale missing') }
  const summaryScale: number | null = reqNum(data, 'summaryScale')
  if (summaryScale === null) { return err('summaryScale missing') }
  const detailScale: number | null = reqNum(data, 'detailScale')
  if (detailScale === null) { return err('detailScale missing') }
  const maxPreviewChars: number | null = reqNum(data, 'maxPreviewChars')
  if (maxPreviewChars === null) { return err('maxPreviewChars missing') }
  const minReadablePx: number | null = reqNum(data, 'minReadablePx')
  if (minReadablePx === null) { return err('minReadablePx missing') }
  const value: import('../dto/StarMapDtos').StarMapDisplayPolicy = {
    importance: importance, minVisibleScale: minVisibleScale, titleScale: titleScale,
    summaryScale: summaryScale, detailScale: detailScale, maxPreviewChars: maxPreviewChars,
    minReadablePx: minReadablePx
  }; return ok(value)
}

function decodeStarMapProvenance(data: Object): DecodeResult<import('../dto/StarMapDtos').StarMapProvenance> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err('StarMapProvenance: expected object')
  }
  const source: string | null = reqStr(data, 'source')
  if (source === null) { return err('source missing') }
  const reviewStatus: string | null = reqStr(data, 'reviewStatus')
  if (reviewStatus === null) { return err('reviewStatus missing') }
  const sourceIdR = nullStr(data, 'sourceId')
  if (!sourceIdR.ok || sourceIdR.value === undefined) { return err(`StarMapProvenance.${sourceIdR.error ?? 'decode failed'}`) }
  const generatedByR = nullStr(data, 'generatedBy')
  if (!generatedByR.ok || generatedByR.value === undefined) { return err(`StarMapProvenance.${generatedByR.error ?? 'decode failed'}`) }
  const promptIdR = nullStr(data, 'promptId')
  if (!promptIdR.ok || promptIdR.value === undefined) { return err(`StarMapProvenance.${promptIdR.error ?? 'decode failed'}`) }
  const createdFromAnchorR = nullStr(data, 'createdFromAnchor')
  if (!createdFromAnchorR.ok || createdFromAnchorR.value === undefined) { return err(`StarMapProvenance.${createdFromAnchorR.error ?? 'decode failed'}`) }
  const value: import('../dto/StarMapDtos').StarMapProvenance = {
    source: source as import('../dto/StarMapDtos').SourceKind,
    sourceId: sourceIdR.value,
    generatedBy: generatedByR.value,
    promptId: promptIdR.value,
    reviewStatus: reviewStatus as import('../dto/StarMapDtos').ReviewStatus,
    createdFromAnchor: createdFromAnchorR.value
  }; return ok(value)
}

function decodeStarMapNode(data: Object): DecodeResult<StarMapNode> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<StarMapNode>('StarMapNode: expected object')
  }
  const id: string | null = reqStr(data, 'id')
  if (id === null) { return err<StarMapNode>('id missing or not string') }
  const title: string | null = reqStr(data, 'title')
  if (title === null) { return err<StarMapNode>('title missing or not string') }
  const kind: string | null = reqStr(data, 'kind')
  if (kind === null) { return err<StarMapNode>('kind missing or not string') }
  const createdAt: number | null = reqNum(data, 'createdAt')
  if (createdAt === null) { return err<StarMapNode>('createdAt missing or not number') }
  const updatedAt: number | null = reqNum(data, 'updatedAt')
  if (updatedAt === null) { return err<StarMapNode>('updatedAt missing or not number') }
  const contentObj: Object | null = reqObj(data, 'content')
  if (contentObj === null) { return err<StarMapNode>('content missing or not object') }
  const contentR = decodeStarMapNodeContent(contentObj)
  if (!contentR.ok || contentR.value === undefined) { return err<StarMapNode>(`content: ${contentR.error}`) }
  const anchorsArr: Array<Object> | null = reqArray(data, 'anchors')
  if (anchorsArr === null) { return err<StarMapNode>('anchors missing or not array') }
  const anchors: import('../dto/StarMapDtos').StarMapAnchor[] = []
  for (let i = 0; i < anchorsArr.length; i++) {
    const aR = decodeStarMapAnchor(anchorsArr[i])
    if (!aR.ok || aR.value === undefined) { return err<StarMapNode>(`anchors[${i}]: ${aR.error}`) }
    anchors.push(aR.value)
  }
  const displayPolicyObj: Object | null = reqObj(data, 'displayPolicy')
  if (displayPolicyObj === null) { return err<StarMapNode>('displayPolicy missing or not object') }
  const displayPolicyR = decodeStarMapDisplayPolicy(displayPolicyObj)
  if (!displayPolicyR.ok || displayPolicyR.value === undefined) { return err<StarMapNode>(`displayPolicy: ${displayPolicyR.error}`) }
  const openBehavior: string | null = reqStr(data, 'openBehavior')
  if (openBehavior === null) { return err<StarMapNode>('openBehavior missing or not string') }
  const provenanceObj: Object | null = reqObj(data, 'provenance')
  if (provenanceObj === null) { return err<StarMapNode>('provenance missing or not object') }
  const provenanceR = decodeStarMapProvenance(provenanceObj)
  if (!provenanceR.ok || provenanceR.value === undefined) { return err<StarMapNode>(`provenance: ${provenanceR.error}`) }
  const tags: string[] | null = reqStrArray(data, 'tags')
  if (tags === null) { return err<StarMapNode>('tags missing or not string[]') }
  const portalObjR: DecodeResult<Object | null> = nullObj(data, 'portal')
  if (!portalObjR.ok || portalObjR.value === undefined) { return err<StarMapNode>(`StarMapNode.${portalObjR.error ?? 'decode failed'}`) }
  let portal: import('../dto/StarMapDtos').StarMapPortal | null = null
  if (portalObjR.value !== null) {
    const portalR = decodeStarMapPortal(portalObjR.value)
    if (!portalR.ok || portalR.value === undefined) { return err<StarMapNode>(`portal: ${portalR.error}`) }
    portal = portalR.value
  }
  const payloadR: DecodeResult<string | null> = nullStr(data, 'payload')
  if (!payloadR.ok || payloadR.value === undefined) { return err<StarMapNode>(`StarMapNode.${payloadR.error ?? 'decode failed'}`) }
  const value: StarMapNode = {
    id: id,
    title: title,
    kind: kind as NodeType,
    payload: payloadR.value,
    tags: tags,
    content: contentR.value,
    anchors: anchors,
    portal: portal,
    displayPolicy: displayPolicyR.value,
    openBehavior: openBehavior as import('../dto/StarMapDtos').OpenBehavior,
    provenance: provenanceR.value,
    createdAt: createdAt,
    updatedAt: updatedAt
  }
  return ok<StarMapNode>(value)
}

function decodeStarMapEdge(data: Object): DecodeResult<StarMapEdge> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<StarMapEdge>('StarMapEdge: expected object')
  }
  const id: string | null = reqStr(data, 'id')
  if (id === null) { return err<StarMapEdge>('id missing or not string') }
  const kind: string | null = reqStr(data, 'kind')
  if (kind === null) { return err<StarMapEdge>('kind missing or not string') }
  const createdAt: number | null = reqNum(data, 'createdAt')
  if (createdAt === null) { return err<StarMapEdge>('createdAt missing or not number') }
  const updatedAt: number | null = reqNum(data, 'updatedAt')
  if (updatedAt === null) { return err<StarMapEdge>('updatedAt missing or not number') }
  const fromTarget: DecodeResult<import('../dto/StarMapDtos').StarMapDeepTarget | null> = decodeOptional(data, 'fromTarget', decodeStarMapDeepTarget)
  if (!fromTarget.ok || fromTarget.value === undefined) { return err<StarMapEdge>(`fromTarget: ${fromTarget.error ?? 'decode failed'}`) }
  const toTarget: DecodeResult<import('../dto/StarMapDtos').StarMapDeepTarget | null> = decodeOptional(data, 'toTarget', decodeStarMapDeepTarget)
  if (!toTarget.ok || toTarget.value === undefined) { return err<StarMapEdge>(`toTarget: ${toTarget.error ?? 'decode failed'}`) }
  const fromEndpoint = decodeOptional(data, 'fromEndpoint', decodeStarMapEdgeEndpoint)
  if (!fromEndpoint.ok || fromEndpoint.value === undefined) { return err<StarMapEdge>(`fromEndpoint: ${fromEndpoint.error ?? 'decode failed'}`) }
  const toEndpoint = decodeOptional(data, 'toEndpoint', decodeStarMapEdgeEndpoint)
  if (!toEndpoint.ok || toEndpoint.value === undefined) { return err<StarMapEdge>(`toEndpoint: ${toEndpoint.error ?? 'decode failed'}`) }
  const fromEndpointPath = decodeOptional(data, 'fromEndpointPath', decodeStarMapEndpointPath)
  if (!fromEndpointPath.ok || fromEndpointPath.value === undefined) { return err<StarMapEdge>(`fromEndpointPath: ${fromEndpointPath.error ?? 'decode failed'}`) }
  const toEndpointPath = decodeOptional(data, 'toEndpointPath', decodeStarMapEndpointPath)
  if (!toEndpointPath.ok || toEndpointPath.value === undefined) { return err<StarMapEdge>(`toEndpointPath: ${toEndpointPath.error ?? 'decode failed'}`) }
  const fromR: DecodeResult<string | null> = nullStr(data, 'from')
  if (!fromR.ok || fromR.value === undefined) { return err<StarMapEdge>(`StarMapEdge.${fromR.error ?? 'decode failed'}`) }
  const toR: DecodeResult<string | null> = nullStr(data, 'to')
  if (!toR.ok || toR.value === undefined) { return err<StarMapEdge>(`StarMapEdge.${toR.error ?? 'decode failed'}`) }
  const labelR: DecodeResult<string | null> = nullStr(data, 'label')
  if (!labelR.ok || labelR.value === undefined) { return err<StarMapEdge>(`StarMapEdge.${labelR.error ?? 'decode failed'}`) }
  const payloadR: DecodeResult<string | null> = nullStr(data, 'payload')
  if (!payloadR.ok || payloadR.value === undefined) { return err<StarMapEdge>(`StarMapEdge.${payloadR.error ?? 'decode failed'}`) }
  const value: StarMapEdge = {
    id: id,
    from: fromR.value,
    to: toR.value,
    kind: kind as EdgeKind,
    label: labelR.value,
    payload: payloadR.value,
    fromTarget: fromTarget.value ?? null,
    toTarget: toTarget.value ?? null,
    fromEndpoint: fromEndpoint.value ?? null,
    toEndpoint: toEndpoint.value ?? null,
    fromEndpointPath: fromEndpointPath.value ?? null,
    toEndpointPath: toEndpointPath.value ?? null,
    createdAt: createdAt,
    updatedAt: updatedAt
  }
  return ok<StarMapEdge>(value)
}

function decodeStarMapEmbed(data: Object): DecodeResult<StarMapEmbed> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<StarMapEmbed>('StarMapEmbed: expected object')
  }
  const instanceId: string | null = reqStr(data, 'instanceId')
  if (instanceId === null) { return err<StarMapEmbed>('instanceId missing') }
  const targetStarmapId: string | null = reqStr(data, 'targetStarmapId')
  if (targetStarmapId === null) { return err<StarMapEmbed>('targetStarmapId missing') }
  const openBehavior: string | null = reqStr(data, 'openBehavior')
  if (openBehavior === null) { return err<StarMapEmbed>('openBehavior missing') }
  const createdAt: number | null = reqNum(data, 'createdAt')
  if (createdAt === null) { return err<StarMapEmbed>('createdAt missing') }
  const updatedAt: number | null = reqNum(data, 'updatedAt')
  if (updatedAt === null) { return err<StarMapEmbed>('updatedAt missing') }
  const placementObj: Object | null = reqObj(data, 'placement')
  if (placementObj === null) { return err<StarMapEmbed>('placement missing or not object') }
  const targetViewportObj: Object | null = reqObj(data, 'targetViewport')
  if (targetViewportObj === null) { return err<StarMapEmbed>('targetViewport missing or not object') }
  const displayPolicyObj: Object | null = reqObj(data, 'displayPolicy')
  if (displayPolicyObj === null) { return err<StarMapEmbed>('displayPolicy missing or not object') }
  const displayPolicyR = decodeStarMapDisplayPolicy(displayPolicyObj)
  if (!displayPolicyR.ok || displayPolicyR.value === undefined) { return err<StarMapEmbed>(`displayPolicy: ${displayPolicyR.error}`) }
  const provenanceObj: Object | null = reqObj(data, 'provenance')
  if (provenanceObj === null) { return err<StarMapEmbed>('provenance missing or not object') }
  const provenanceR = decodeStarMapProvenance(provenanceObj)
  if (!provenanceR.ok || provenanceR.value === undefined) { return err<StarMapEmbed>(`provenance: ${provenanceR.error}`) }
  const hostEndpoint = decodeOptional(data, 'hostEndpoint', decodeStarMapEndpoint)
  if (!hostEndpoint.ok || hostEndpoint.value === undefined) { return err<StarMapEmbed>(`hostEndpoint: ${hostEndpoint.error ?? 'decode failed'}`) }
  const labelR: DecodeResult<string | null> = nullStr(data, 'label')
  if (!labelR.ok || labelR.value === undefined) { return err<StarMapEmbed>(`StarMapEmbed.${labelR.error ?? 'decode failed'}`) }
  const sourceNodeIdR: DecodeResult<string | null> = nullStr(data, 'sourceNodeId')
  if (!sourceNodeIdR.ok || sourceNodeIdR.value === undefined) { return err<StarMapEmbed>(`StarMapEmbed.${sourceNodeIdR.error ?? 'decode failed'}`) }
  const pX: number | null = reqNum(placementObj, 'x')
  if (pX === null) { return err<StarMapEmbed>('StarMapEmbed.placement.x missing or not number') }
  const pY: number | null = reqNum(placementObj, 'y')
  if (pY === null) { return err<StarMapEmbed>('StarMapEmbed.placement.y missing or not number') }
  const pWidth: number | null = reqNum(placementObj, 'width')
  if (pWidth === null) { return err<StarMapEmbed>('StarMapEmbed.placement.width missing or not number') }
  const pHeight: number | null = reqNum(placementObj, 'height')
  if (pHeight === null) { return err<StarMapEmbed>('StarMapEmbed.placement.height missing or not number') }
  const pScale: number | null = reqNum(placementObj, 'scale')
  if (pScale === null) { return err<StarMapEmbed>('StarMapEmbed.placement.scale missing or not number') }
  const pZIndex: number | null = reqNum(placementObj, 'zIndex')
  if (pZIndex === null) { return err<StarMapEmbed>('StarMapEmbed.placement.zIndex missing or not number') }
  const pCollapsed: boolean | null = reqBool(placementObj, 'collapsed')
  if (pCollapsed === null) { return err<StarMapEmbed>('StarMapEmbed.placement.collapsed missing or not boolean') }
  const tvScale: number | null = reqNum(targetViewportObj, 'scale')
  if (tvScale === null) { return err<StarMapEmbed>('StarMapEmbed.targetViewport.scale missing or not number') }
  const tvOffsetX: number | null = reqNum(targetViewportObj, 'offsetX')
  if (tvOffsetX === null) { return err<StarMapEmbed>('StarMapEmbed.targetViewport.offsetX missing or not number') }
  const tvOffsetY: number | null = reqNum(targetViewportObj, 'offsetY')
  if (tvOffsetY === null) { return err<StarMapEmbed>('StarMapEmbed.targetViewport.offsetY missing or not number') }
  const placement: import('../dto/StarMapDtos').StarMapEmbedPlacement = {
    x: pX,
    y: pY,
    width: pWidth,
    height: pHeight,
    scale: pScale,
    zIndex: pZIndex,
    collapsed: pCollapsed
  }
  const targetViewport: import('../dto/StarMapDtos').StarMapEmbedViewport = {
    scale: tvScale,
    offsetX: tvOffsetX,
    offsetY: tvOffsetY
  }
  const value: StarMapEmbed = {
    instanceId: instanceId,
    targetStarmapId: targetStarmapId,
    label: labelR.value,
    displayPolicy: displayPolicyR.value,
    openBehavior: openBehavior as import('../dto/StarMapDtos').OpenBehavior,
    placement: placement,
    targetViewport: targetViewport,
    sourceNodeId: sourceNodeIdR.value,
    hostEndpoint: hostEndpoint.value ?? null,
    provenance: provenanceR.value,
    createdAt: createdAt,
    updatedAt: updatedAt
  }
  return ok<StarMapEmbed>(value)
}

function decodeStarMapEndpoint(data: Object): DecodeResult<import('../dto/StarMapDtos').StarMapEndpoint> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err('StarMapEndpoint: expected object')
  }
  const type: string | null = reqStr(data, 'type')
  if (type === null) { return err('type missing') }
  const nodeIdR = nullStr(data, 'nodeId')
  if (!nodeIdR.ok || nodeIdR.value === undefined) { return err(`StarMapEndpoint.${nodeIdR.error ?? 'decode failed'}`) }
  const anchorIdR = nullStr(data, 'anchorId')
  if (!anchorIdR.ok || anchorIdR.value === undefined) { return err(`StarMapEndpoint.${anchorIdR.error ?? 'decode failed'}`) }
  const value: import('../dto/StarMapDtos').StarMapEndpoint = {
    type: type,
    nodeId: nodeIdR.value,
    anchorId: anchorIdR.value
  }; return ok(value)
}

function decodeStarMapEdgeEndpoint(data: Object): DecodeResult<import('../dto/StarMapDtos').StarMapEdgeEndpoint> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err('StarMapEdgeEndpoint: expected object')
  }
  const type: string | null = reqStr(data, 'type')
  if (type === null) { return err('type missing') }
  const target = decodeOptional(data, 'target', decodeStarMapDeepTarget)
  if (!target.ok || target.value === undefined) { return err(`target: ${target.error ?? 'decode failed'}`) }
  const nodeIdR = nullStr(data, 'nodeId')
  if (!nodeIdR.ok || nodeIdR.value === undefined) { return err(`StarMapEdgeEndpoint.${nodeIdR.error ?? 'decode failed'}`) }
  const anchorIdR = nullStr(data, 'anchorId')
  if (!anchorIdR.ok || anchorIdR.value === undefined) { return err(`StarMapEdgeEndpoint.${anchorIdR.error ?? 'decode failed'}`) }
  const value: import('../dto/StarMapDtos').StarMapEdgeEndpoint = {
    type: type,
    nodeId: nodeIdR.value,
    anchorId: anchorIdR.value,
    target: target.value ?? null
  }; return ok(value)
}

function decodeStarMapEndpointPathSegment(data: Object): DecodeResult<import('../dto/StarMapDtos').StarMapEndpointPathSegment> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err('StarMapEndpointPathSegment: expected object')
  }
  const type: string | null = reqStr(data, 'type')
  if (type === null) { return err('type missing') }
  const starmapIdR = nullStr(data, 'starmapId')
  if (!starmapIdR.ok || starmapIdR.value === undefined) { return err(`StarMapEndpointPathSegment.${starmapIdR.error ?? 'decode failed'}`) }
  const value: import('../dto/StarMapDtos').StarMapEndpointPathSegment = {
    type: type,
    starmapId: starmapIdR.value
  }; return ok(value)
}

function decodeStarMapEndpointPath(data: Object): DecodeResult<import('../dto/StarMapDtos').StarMapEndpointPath> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err('StarMapEndpointPath: expected object')
  }
  const segmentsArr: Array<Object> | null = reqArray(data, 'segments')
  if (segmentsArr === null) { return err('segments missing or not array') }
  const segmentsR: DecodeResult<import('../dto/StarMapDtos').StarMapEndpointPathSegment[]> =
    decodeArray<import('../dto/StarMapDtos').StarMapEndpointPathSegment>(segmentsArr as Object, decodeStarMapEndpointPathSegment, 'segments')
  if (!segmentsR.ok || segmentsR.value === undefined) { return err(`segments: ${segmentsR.error}`) }
  const endpointObj: Object | null = reqObj(data, 'endpoint')
  if (endpointObj === null) { return err('endpoint missing or not object') }
  const endpointR = decodeStarMapEdgeEndpoint(endpointObj)
  if (!endpointR.ok || endpointR.value === undefined) { return err(`endpoint: ${endpointR.error}`) }
  const value: import('../dto/StarMapDtos').StarMapEndpointPath = {
    segments: segmentsR.value,
    endpoint: endpointR.value
  }; return ok(value)
}

function decodeStarMapPathSegment(data: Object): DecodeResult<import('../dto/StarMapDtos').StarMapPathSegment> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err('StarMapPathSegment: expected object')
  }
  const type: string | null = reqStr(data, 'type')
  if (type === null) { return err('type missing') }
  const starmapIdR = nullStr(data, 'starmapId')
  if (!starmapIdR.ok || starmapIdR.value === undefined) { return err(`StarMapPathSegment.${starmapIdR.error ?? 'decode failed'}`) }
  const value: import('../dto/StarMapDtos').StarMapPathSegment = {
    type: type,
    starmapId: starmapIdR.value
  }; return ok(value)
}

function decodeStarMapTargetDetail(data: Object): DecodeResult<import('../dto/StarMapDtos').StarMapTargetDetail> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err('StarMapTargetDetail: expected object')
  }
  const type: string | null = reqStr(data, 'type')
  if (type === null) { return err('type missing') }
  const nodeIdR = nullStr(data, 'nodeId')
  if (!nodeIdR.ok || nodeIdR.value === undefined) { return err(`StarMapTargetDetail.${nodeIdR.error ?? 'decode failed'}`) }
  const anchorIdR = nullStr(data, 'anchorId')
  if (!anchorIdR.ok || anchorIdR.value === undefined) { return err(`StarMapTargetDetail.${anchorIdR.error ?? 'decode failed'}`) }
  const projectIdR = nullStr(data, 'projectId')
  if (!projectIdR.ok || projectIdR.value === undefined) { return err(`StarMapTargetDetail.${projectIdR.error ?? 'decode failed'}`) }
  const volumeIdR = nullStr(data, 'volumeId')
  if (!volumeIdR.ok || volumeIdR.value === undefined) { return err(`StarMapTargetDetail.${volumeIdR.error ?? 'decode failed'}`) }
  const chapterIdR = nullStr(data, 'chapterId')
  if (!chapterIdR.ok || chapterIdR.value === undefined) { return err(`StarMapTargetDetail.${chapterIdR.error ?? 'decode failed'}`) }
  const rangeStartR = nullNum(data, 'rangeStart')
  if (!rangeStartR.ok || rangeStartR.value === undefined) { return err(`StarMapTargetDetail.${rangeStartR.error ?? 'decode failed'}`) }
  const rangeEndR = nullNum(data, 'rangeEnd')
  if (!rangeEndR.ok || rangeEndR.value === undefined) { return err(`StarMapTargetDetail.${rangeEndR.error ?? 'decode failed'}`) }
  const entityTypeR = nullStr(data, 'entityType')
  if (!entityTypeR.ok || entityTypeR.value === undefined) { return err(`StarMapTargetDetail.${entityTypeR.error ?? 'decode failed'}`) }
  const entityIdR = nullStr(data, 'entityId')
  if (!entityIdR.ok || entityIdR.value === undefined) { return err(`StarMapTargetDetail.${entityIdR.error ?? 'decode failed'}`) }
  const uriR = nullStr(data, 'uri')
  if (!uriR.ok || uriR.value === undefined) { return err(`StarMapTargetDetail.${uriR.error ?? 'decode failed'}`) }
  const value: import('../dto/StarMapDtos').StarMapTargetDetail = {
    type: type,
    nodeId: nodeIdR.value,
    anchorId: anchorIdR.value,
    projectId: projectIdR.value,
    volumeId: volumeIdR.value,
    chapterId: chapterIdR.value,
    rangeStart: rangeStartR.value,
    rangeEnd: rangeEndR.value,
    entityType: entityTypeR.value,
    entityId: entityIdR.value,
    uri: uriR.value
  }; return ok(value)
}

function decodeStarMapDeepTarget(data: Object): DecodeResult<import('../dto/StarMapDtos').StarMapDeepTarget> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err('StarMapDeepTarget: expected object')
  }
  const starmapId: string | null = reqStr(data, 'starmapId')
  if (starmapId === null) { return err('starmapId missing') }
  const pathArr: Array<Object> | null = reqArray(data, 'path')
  if (pathArr === null) { return err('path missing or not array') }
  const path: import('../dto/StarMapDtos').StarMapPathSegment[] = []
  for (let i = 0; i < pathArr.length; i++) {
    const pR = decodeStarMapPathSegment(pathArr[i])
    if (!pR.ok || pR.value === undefined) { return err(`path[${i}]: ${pR.error}`) }
    path.push(pR.value)
  }
  const targetObj: Object | null = reqObj(data, 'target')
  if (targetObj === null) { return err('target missing or not object') }
  const targetR = decodeStarMapTargetDetail(targetObj)
  if (!targetR.ok || targetR.value === undefined) { return err(`target: ${targetR.error}`) }
  const value: import('../dto/StarMapDtos').StarMapDeepTarget = {
    starmapId: starmapId,
    path: path,
    target: targetR.value
  }; return ok(value)
}

function decodeStarMapLink(data: Object): DecodeResult<StarMapLink> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<StarMapLink>('StarMapLink: expected object')
  }
  const linkId: string | null = reqStr(data, 'linkId')
  if (linkId === null) { return err<StarMapLink>('linkId missing') }
  const sourceObj: Object | null = reqObj(data, 'source')
  if (sourceObj === null) { return err<StarMapLink>('source missing or not object') }
  const sourceR = decodeStarMapEndpoint(sourceObj)
  if (!sourceR.ok || sourceR.value === undefined) { return err<StarMapLink>(`source: ${sourceR.error}`) }
  const targetObj: Object | null = reqObj(data, 'target')
  if (targetObj === null) { return err<StarMapLink>('target missing or not object') }
  const targetR = decodeStarMapDeepTarget(targetObj)
  if (!targetR.ok || targetR.value === undefined) { return err<StarMapLink>(`target: ${targetR.error}`) }
  const createdAt: number | null = reqNum(data, 'createdAt')
  if (createdAt === null) { return err<StarMapLink>('createdAt missing') }
  const updatedAt: number | null = reqNum(data, 'updatedAt')
  if (updatedAt === null) { return err<StarMapLink>('updatedAt missing') }
  const labelR: DecodeResult<string | null> = nullStr(data, 'label')
  if (!labelR.ok || labelR.value === undefined) { return err<StarMapLink>(`StarMapLink.${labelR.error ?? 'decode failed'}`) }
  const value: StarMapLink = {
    linkId: linkId,
    source: sourceR.value,
    target: targetR.value,
    label: labelR.value,
    createdAt: createdAt,
    updatedAt: updatedAt
  }
  return ok<StarMapLink>(value)
}

function decodeStarMapHyperlink(data: Object): DecodeResult<StarMapHyperlink> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<StarMapHyperlink>('StarMapHyperlink: expected object')
  }
  const hyperlinkId: string | null = reqStr(data, 'hyperlinkId')
  if (hyperlinkId === null) { return err<StarMapHyperlink>('hyperlinkId missing') }
  const sourceObj: Object | null = reqObj(data, 'source')
  if (sourceObj === null) { return err<StarMapHyperlink>('source missing or not object') }
  const sourceR = decodeStarMapEndpointPath(sourceObj)
  if (!sourceR.ok || sourceR.value === undefined) { return err<StarMapHyperlink>(`source: ${sourceR.error}`) }
  const targetUri: string | null = reqStr(data, 'targetUri')
  if (targetUri === null) { return err<StarMapHyperlink>('targetUri missing') }
  const createdAt: number | null = reqNum(data, 'createdAt')
  if (createdAt === null) { return err<StarMapHyperlink>('createdAt missing') }
  const updatedAt: number | null = reqNum(data, 'updatedAt')
  if (updatedAt === null) { return err<StarMapHyperlink>('updatedAt missing') }
  const labelR: DecodeResult<string | null> = nullStr(data, 'label')
  if (!labelR.ok || labelR.value === undefined) { return err<StarMapHyperlink>(`StarMapHyperlink.${labelR.error ?? 'decode failed'}`) }
  const targetStarmapIdR: DecodeResult<string | null> = nullStr(data, 'targetStarmapId')
  if (!targetStarmapIdR.ok || targetStarmapIdR.value === undefined) { return err<StarMapHyperlink>(`StarMapHyperlink.${targetStarmapIdR.error ?? 'decode failed'}`) }
  const value: StarMapHyperlink = {
    hyperlinkId: hyperlinkId,
    source: sourceR.value,
    targetUri: targetUri,
    label: labelR.value,
    targetStarmapId: targetStarmapIdR.value,
    createdAt: createdAt,
    updatedAt: updatedAt
  }
  return ok<StarMapHyperlink>(value)
}

function decodeStarMapLayoutNode(data: Object): DecodeResult<StarMapLayoutNode> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<StarMapLayoutNode>('StarMapLayoutNode: expected object')
  }
  const nodeId: string | null = reqStr(data, 'nodeId')
  if (nodeId === null) { return err<StarMapLayoutNode>('nodeId missing') }
  const orbitGroupR: DecodeResult<string | null> = nullStr(data, 'orbitGroup')
  if (!orbitGroupR.ok || orbitGroupR.value === undefined) { return err<StarMapLayoutNode>(`StarMapLayoutNode.${orbitGroupR.error ?? 'decode failed'}`) }
  const x: number | null = reqNum(data, 'x')
  if (x === null) { return err<StarMapLayoutNode>('StarMapLayoutNode.x missing or not number') }
  const y: number | null = reqNum(data, 'y')
  if (y === null) { return err<StarMapLayoutNode>('StarMapLayoutNode.y missing or not number') }
  const width: number | null = reqNum(data, 'width')
  if (width === null) { return err<StarMapLayoutNode>('StarMapLayoutNode.width missing or not number') }
  const height: number | null = reqNum(data, 'height')
  if (height === null) { return err<StarMapLayoutNode>('StarMapLayoutNode.height missing or not number') }
  const radius: number | null = reqNum(data, 'radius')
  if (radius === null) { return err<StarMapLayoutNode>('StarMapLayoutNode.radius missing or not number') }
  const collapsed: boolean | null = reqBool(data, 'collapsed')
  if (collapsed === null) { return err<StarMapLayoutNode>('StarMapLayoutNode.collapsed missing or not boolean') }
  const zIndex: number | null = reqNum(data, 'zIndex')
  if (zIndex === null) { return err<StarMapLayoutNode>('StarMapLayoutNode.zIndex missing or not number') }
  const scale: number | null = reqNum(data, 'scale')
  if (scale === null) { return err<StarMapLayoutNode>('StarMapLayoutNode.scale missing or not number') }
  const depth: number | null = reqNum(data, 'depth')
  if (depth === null) { return err<StarMapLayoutNode>('StarMapLayoutNode.depth missing or not number') }
  const focusWeight: number | null = reqNum(data, 'focusWeight')
  if (focusWeight === null) { return err<StarMapLayoutNode>('StarMapLayoutNode.focusWeight missing or not number') }
  const value: StarMapLayoutNode = {
    nodeId: nodeId,
    x: x,
    y: y,
    width: width,
    height: height,
    radius: radius,
    collapsed: collapsed,
    zIndex: zIndex,
    scale: scale,
    depth: depth,
    focusWeight: focusWeight,
    orbitGroup: orbitGroupR.value
  }
  return ok<StarMapLayoutNode>(value)
}

export function decodeStarMapLayout(data: Object): DecodeResult<StarMapLayout> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<StarMapLayout>('StarMapLayout: expected object')
  }
  const kind: string | null = reqStr(data, 'kind')
  if (kind === null) { return err<StarMapLayout>('kind missing or not string') }
  const nodesArr: Array<Object> | null = reqArray(data, 'nodes')
  if (nodesArr === null) { return err<StarMapLayout>('nodes missing or not array') }
  const nodesR: DecodeResult<StarMapLayoutNode[]> = decodeArray<StarMapLayoutNode>(nodesArr as Object, decodeStarMapLayoutNode, 'nodes')
  if (!nodesR.ok || nodesR.value === undefined) { return err<StarMapLayout>(nodesR.error ?? 'decode failed') }
  const value: StarMapLayout = {
    kind: kind as 'Freeform' | 'AutoRadial' | 'Custom',
    nodes: nodesR.value
  }
  return ok<StarMapLayout>(value)
}

export function decodeStarMapGraph(data: Object): DecodeResult<StarMapGraph> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<StarMapGraph>('StarMapGraph: expected object')
  }
  const schemaVersion: number | null = reqNum(data, 'schemaVersion')
  if (schemaVersion === null) { return err<StarMapGraph>('schemaVersion missing') }
  const id: string | null = reqStr(data, 'id')
  if (id === null) { return err<StarMapGraph>('id missing') }
  const starmapId: string | null = reqStr(data, 'starmapId')
  if (starmapId === null) { return err<StarMapGraph>('starmapId missing') }
  const title: string | null = reqStr(data, 'title')
  if (title === null) { return err<StarMapGraph>('title missing') }
  const nodesArr: Array<Object> | null = reqArray(data, 'nodes')
  if (nodesArr === null) { return err<StarMapGraph>('nodes missing or not array') }
  const nodesR: DecodeResult<StarMapNode[]> = decodeArray<StarMapNode>(nodesArr as Object, decodeStarMapNode, 'nodes')
  if (!nodesR.ok || nodesR.value === undefined) { return err<StarMapGraph>(nodesR.error ?? 'decode failed') }
  const edgesArr: Array<Object> | null = reqArray(data, 'edges')
  if (edgesArr === null) { return err<StarMapGraph>('edges missing or not array') }
  const edgesR: DecodeResult<StarMapEdge[]> = decodeArray<StarMapEdge>(edgesArr as Object, decodeStarMapEdge, 'edges')
  if (!edgesR.ok || edgesR.value === undefined) { return err<StarMapGraph>(edgesR.error ?? 'decode failed') }
  const embedsArr: Array<Object> | null = reqArray(data, 'embeds')
  if (embedsArr === null) { return err<StarMapGraph>('embeds missing or not array') }
  const embedsR: DecodeResult<StarMapEmbed[]> = decodeArray<StarMapEmbed>(embedsArr as Object, decodeStarMapEmbed, 'embeds')
  if (!embedsR.ok || embedsR.value === undefined) { return err<StarMapGraph>(embedsR.error ?? 'decode failed') }
  const linksArr: Array<Object> | null = reqArray(data, 'links')
  if (linksArr === null) { return err<StarMapGraph>('links missing or not array') }
  const linksR: DecodeResult<StarMapLink[]> = decodeArray<StarMapLink>(linksArr as Object, decodeStarMapLink, 'links')
  if (!linksR.ok || linksR.value === undefined) { return err<StarMapGraph>(linksR.error ?? 'decode failed') }
  const hyperlinksArr: Array<Object> | null = reqArray(data, 'hyperlinks')
  if (hyperlinksArr === null) { return err<StarMapGraph>('hyperlinks missing or not array') }
  const hyperlinksR: DecodeResult<StarMapHyperlink[]> = decodeArray<StarMapHyperlink>(hyperlinksArr as Object, decodeStarMapHyperlink, 'hyperlinks')
  if (!hyperlinksR.ok || hyperlinksR.value === undefined) { return err<StarMapGraph>(hyperlinksR.error ?? 'decode failed') }
  const createdAt: number | null = reqNum(data, 'createdAt')
  if (createdAt === null) { return err<StarMapGraph>('createdAt missing') }
  const updatedAt: number | null = reqNum(data, 'updatedAt')
  if (updatedAt === null) { return err<StarMapGraph>('updatedAt missing') }
  const value: StarMapGraph = {
    schemaVersion: schemaVersion,
    id: id,
    starmapId: starmapId,
    title: title,
    nodes: nodesR.value,
    edges: edgesR.value,
    embeds: embedsR.value,
    links: linksR.value,
    hyperlinks: hyperlinksR.value,
    createdAt: createdAt,
    updatedAt: updatedAt
  }
  return ok<StarMapGraph>(value)
}

export function decodeStarMapEdgeRender(data: Object): DecodeResult<StarMapEdgeRender> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<StarMapEdgeRender>('StarMapEdgeRender: expected object')
  }
  const edgeId: string | null = reqStr(data, 'edgeId')
  if (edgeId === null) { return err<StarMapEdgeRender>('edgeId missing') }
  const labelR: DecodeResult<string | null> = nullStr(data, 'label')
  if (!labelR.ok || labelR.value === undefined) { return err<StarMapEdgeRender>(`StarMapEdgeRender.${labelR.error ?? 'decode failed'}`) }
  const fromCx: number | null = reqNum(data, 'fromCx')
  if (fromCx === null) { return err<StarMapEdgeRender>('StarMapEdgeRender.fromCx missing or not number') }
  const fromCy: number | null = reqNum(data, 'fromCy')
  if (fromCy === null) { return err<StarMapEdgeRender>('StarMapEdgeRender.fromCy missing or not number') }
  const toCx: number | null = reqNum(data, 'toCx')
  if (toCx === null) { return err<StarMapEdgeRender>('StarMapEdgeRender.toCx missing or not number') }
  const toCy: number | null = reqNum(data, 'toCy')
  if (toCy === null) { return err<StarMapEdgeRender>('StarMapEdgeRender.toCy missing or not number') }
  const startX: number | null = reqNum(data, 'startX')
  if (startX === null) { return err<StarMapEdgeRender>('StarMapEdgeRender.startX missing or not number') }
  const startY: number | null = reqNum(data, 'startY')
  if (startY === null) { return err<StarMapEdgeRender>('StarMapEdgeRender.startY missing or not number') }
  const endX: number | null = reqNum(data, 'endX')
  if (endX === null) { return err<StarMapEdgeRender>('StarMapEdgeRender.endX missing or not number') }
  const endY: number | null = reqNum(data, 'endY')
  if (endY === null) { return err<StarMapEdgeRender>('StarMapEdgeRender.endY missing or not number') }
  const offsetX: number | null = reqNum(data, 'offsetX')
  if (offsetX === null) { return err<StarMapEdgeRender>('StarMapEdgeRender.offsetX missing or not number') }
  const offsetY: number | null = reqNum(data, 'offsetY')
  if (offsetY === null) { return err<StarMapEdgeRender>('StarMapEdgeRender.offsetY missing or not number') }
  const arrowTipX: number | null = reqNum(data, 'arrowTipX')
  if (arrowTipX === null) { return err<StarMapEdgeRender>('StarMapEdgeRender.arrowTipX missing or not number') }
  const arrowTipY: number | null = reqNum(data, 'arrowTipY')
  if (arrowTipY === null) { return err<StarMapEdgeRender>('StarMapEdgeRender.arrowTipY missing or not number') }
  const arrowLeftX: number | null = reqNum(data, 'arrowLeftX')
  if (arrowLeftX === null) { return err<StarMapEdgeRender>('StarMapEdgeRender.arrowLeftX missing or not number') }
  const arrowLeftY: number | null = reqNum(data, 'arrowLeftY')
  if (arrowLeftY === null) { return err<StarMapEdgeRender>('StarMapEdgeRender.arrowLeftY missing or not number') }
  const arrowRightX: number | null = reqNum(data, 'arrowRightX')
  if (arrowRightX === null) { return err<StarMapEdgeRender>('StarMapEdgeRender.arrowRightX missing or not number') }
  const arrowRightY: number | null = reqNum(data, 'arrowRightY')
  if (arrowRightY === null) { return err<StarMapEdgeRender>('StarMapEdgeRender.arrowRightY missing or not number') }
  const labelX: number | null = reqNum(data, 'labelX')
  if (labelX === null) { return err<StarMapEdgeRender>('StarMapEdgeRender.labelX missing or not number') }
  const labelY: number | null = reqNum(data, 'labelY')
  if (labelY === null) { return err<StarMapEdgeRender>('StarMapEdgeRender.labelY missing or not number') }
  const hasBidirectional: boolean | null = reqBool(data, 'hasBidirectional')
  if (hasBidirectional === null) { return err<StarMapEdgeRender>('StarMapEdgeRender.hasBidirectional missing or not boolean') }
  const value: StarMapEdgeRender = {
    edgeId: edgeId,
    fromCx: fromCx,
    fromCy: fromCy,
    toCx: toCx,
    toCy: toCy,
    startX: startX,
    startY: startY,
    endX: endX,
    endY: endY,
    offsetX: offsetX,
    offsetY: offsetY,
    arrowTipX: arrowTipX,
    arrowTipY: arrowTipY,
    arrowLeftX: arrowLeftX,
    arrowLeftY: arrowLeftY,
    arrowRightX: arrowRightX,
    arrowRightY: arrowRightY,
    labelX: labelX,
    labelY: labelY,
    label: labelR.value,
    hasBidirectional: hasBidirectional
  }
  return ok<StarMapEdgeRender>(value)
}

export function decodeStarMapMotionPolicy(data: Object): DecodeResult<StarMapMotionPolicy> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<StarMapMotionPolicy>('StarMapMotionPolicy: expected object')
  }
  const enabled: boolean | null = reqBool(data, 'enabled')
  if (enabled === null) { return err<StarMapMotionPolicy>('StarMapMotionPolicy.enabled missing or not boolean') }
  const idleWobbleEnabled: boolean | null = reqBool(data, 'idleWobbleEnabled')
  if (idleWobbleEnabled === null) { return err<StarMapMotionPolicy>('StarMapMotionPolicy.idleWobbleEnabled missing or not boolean') }
  const idleAmplitudeVp: number | null = reqNum(data, 'idleAmplitudeVp')
  if (idleAmplitudeVp === null) { return err<StarMapMotionPolicy>('StarMapMotionPolicy.idleAmplitudeVp missing or not number') }
  const idlePeriodMs: number | null = reqNum(data, 'idlePeriodMs')
  if (idlePeriodMs === null) { return err<StarMapMotionPolicy>('StarMapMotionPolicy.idlePeriodMs missing or not number') }
  const dragLiftScale: number | null = reqNum(data, 'dragLiftScale')
  if (dragLiftScale === null) { return err<StarMapMotionPolicy>('StarMapMotionPolicy.dragLiftScale missing or not number') }
  const dragShadowBoost: number | null = reqNum(data, 'dragShadowBoost')
  if (dragShadowBoost === null) { return err<StarMapMotionPolicy>('StarMapMotionPolicy.dragShadowBoost missing or not number') }
  const settleDurationMs: number | null = reqNum(data, 'settleDurationMs')
  if (settleDurationMs === null) { return err<StarMapMotionPolicy>('StarMapMotionPolicy.settleDurationMs missing or not number') }
  const reduceMotion: boolean | null = reqBool(data, 'reduceMotion')
  if (reduceMotion === null) { return err<StarMapMotionPolicy>('StarMapMotionPolicy.reduceMotion missing or not boolean') }
  const value: StarMapMotionPolicy = {
    enabled: enabled,
    idleWobbleEnabled: idleWobbleEnabled,
    idleAmplitudeVp: idleAmplitudeVp,
    idlePeriodMs: idlePeriodMs,
    dragLiftScale: dragLiftScale,
    dragShadowBoost: dragShadowBoost,
    settleDurationMs: settleDurationMs,
    reduceMotion: reduceMotion
  }
  return ok<StarMapMotionPolicy>(value)
}

// ─── Sync Decoders ───

function decodeSyncConflict(data: Object): DecodeResult<SyncConflict> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<SyncConflict>('SyncConflict: expected object')
  }
  const localPath: string | null = reqStr(data, 'localPath')
  if (localPath === null) { return err<SyncConflict>('localPath missing') }
  const remotePath: string | null = reqStr(data, 'remotePath')
  if (remotePath === null) { return err<SyncConflict>('remotePath missing') }
  const localHash: string | null = reqStr(data, 'localHash')
  if (localHash === null) { return err<SyncConflict>('localHash missing') }
  const remoteHash: string | null = reqStr(data, 'remoteHash')
  if (remoteHash === null) { return err<SyncConflict>('remoteHash missing') }
  const baseHash: string | null = reqStr(data, 'baseHash')
  if (baseHash === null) { return err<SyncConflict>('baseHash missing') }
  const createdAt: number | null = reqNum(data, 'createdAt')
  if (createdAt === null) { return err<SyncConflict>('createdAt missing') }
  const description: string | null = reqStr(data, 'description')
  if (description === null) { return err<SyncConflict>('description missing') }
  // kind: Core 侧 #[serde(default = "both_changed")]，老数据可能缺失 → 默认 "both_changed"
  const kindRaw: string | null = reqStr(data, 'kind')
  const kind: string = kindRaw !== null ? kindRaw : 'both_changed'
  // remoteSnapshotPath: Core 侧 Option<String>，恒发该键（string 或 null）
  const remoteSnapshotPathR: DecodeResult<string | null> = nullStr(data, 'remoteSnapshotPath')
  if (!remoteSnapshotPathR.ok || remoteSnapshotPathR.value === undefined) { return err<SyncConflict>(`SyncConflict.${remoteSnapshotPathR.error ?? 'decode failed'}`) }
  return ok<SyncConflict>({
    localPath: localPath, remotePath: remotePath, localHash: localHash, remoteHash: remoteHash,
    baseHash: baseHash, createdAt: createdAt, description: description,
    kind: kind, remoteSnapshotPath: remoteSnapshotPathR.value
  })
}

export function decodeProviderConfig(data: Object): DecodeResult<ProviderConfig> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<ProviderConfig>('ProviderConfig: expected object')
  }
  const type: string | null = reqStr(data, 'type')
  if (type === null) { return err<ProviderConfig>('ProviderConfig.type missing or not string') }
  const remoteUrl: string | null = reqStr(data, 'remote_url')
  if (remoteUrl === null) { return err<ProviderConfig>('ProviderConfig.remote_url missing or not string') }
  const branch: string | null = reqStr(data, 'branch')
  if (branch === null) { return err<ProviderConfig>('ProviderConfig.branch missing or not string') }
  const username: string | null = reqStr(data, 'username')
  if (username === null) { return err<ProviderConfig>('ProviderConfig.username missing or not string') }
  const transport: string | null = reqStr(data, 'transport')
  if (transport === null) { return err<ProviderConfig>('ProviderConfig.transport missing or not string') }
  return ok<ProviderConfig>({ type: type, remote_url: remoteUrl, branch: branch, username: username, transport: transport })
}

export function decodeSyncConfig(data: Object): DecodeResult<SyncConfig> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<SyncConfig>('SyncConfig: expected object')
  }
  const enabled: boolean | null = reqBool(data, 'enabled')
  if (enabled === null) { return err<SyncConfig>('SyncConfig.enabled missing or not boolean') }
  const activeProvider: string | null = reqStr(data, 'activeProvider')
  if (activeProvider === null) { return err<SyncConfig>('SyncConfig.activeProvider missing or not string') }
  const autoSync: boolean | null = reqBool(data, 'autoSync')
  if (autoSync === null) { return err<SyncConfig>('SyncConfig.autoSync missing or not boolean') }
  const syncIntervalSeconds: number | null = reqNum(data, 'syncIntervalSeconds')
  if (syncIntervalSeconds === null) { return err<SyncConfig>('SyncConfig.syncIntervalSeconds missing or not number') }
  const hasNetworkPermission: boolean | null = reqBool(data, 'hasNetworkPermission')
  if (hasNetworkPermission === null) { return err<SyncConfig>('SyncConfig.hasNetworkPermission missing or not boolean') }
  const hasNetworkStatePermission: boolean | null = reqBool(data, 'hasNetworkStatePermission')
  if (hasNetworkStatePermission === null) { return err<SyncConfig>('SyncConfig.hasNetworkStatePermission missing or not boolean') }

  const providerConfig = decodeOptional(data, 'providerConfig', decodeProviderConfig)
  if (!providerConfig.ok || providerConfig.value === undefined) { return err<SyncConfig>(`providerConfig: ${providerConfig.error ?? 'decode failed'}`) }

  const value: SyncConfig = {
    enabled: enabled,
    activeProvider: activeProvider,
    providerConfig: providerConfig.value ?? null,
    autoSync: autoSync,
    syncIntervalSeconds: syncIntervalSeconds,
    hasNetworkPermission: hasNetworkPermission,
    hasNetworkStatePermission: hasNetworkStatePermission
  }
  return ok<SyncConfig>(value)
}

export function decodeSyncState(data: Object): DecodeResult<SyncState> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<SyncState>('SyncState: expected object')
  }
  const status: string | null = reqStr(data, 'status')
  if (status === null) { return err<SyncState>('status missing') }
  const conflictsArrR: DecodeResult<Array<Object> | null> = nullArray(data, 'conflicts')
  if (!conflictsArrR.ok || conflictsArrR.value === undefined) { return err<SyncState>(`SyncState.${conflictsArrR.error ?? 'decode failed'}`) }
  let conflicts: SyncConflict[] | null = null
  if (conflictsArrR.value !== null) {
    const cR: DecodeResult<SyncConflict[]> = decodeArray<SyncConflict>(conflictsArrR.value as Object, decodeSyncConflict, 'conflicts')
    if (!cR.ok || cR.value === undefined) { return err<SyncState>(cR.error ?? 'decode failed') }
    conflicts = cR.value
  }
  const lastSyncTimeR: DecodeResult<number | null> = nullNum(data, 'lastSyncTime')
  if (!lastSyncTimeR.ok || lastSyncTimeR.value === undefined) { return err<SyncState>(`SyncState.${lastSyncTimeR.error ?? 'decode failed'}`) }
  const lastErrorR: DecodeResult<string | null> = nullStr(data, 'lastError')
  if (!lastErrorR.ok || lastErrorR.value === undefined) { return err<SyncState>(`SyncState.${lastErrorR.error ?? 'decode failed'}`) }
  const value: SyncState = {
    status: status,
    lastSyncTime: lastSyncTimeR.value,
    lastError: lastErrorR.value,
    conflicts: conflicts
  }
  return ok<SyncState>(value)
}

function decodeSyncResult(data: Object): DecodeResult<SyncResult> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<SyncResult>('SyncResult: expected object')
  }
  const status: string | null = reqStr(data, 'status')
  if (status === null) { return err<SyncResult>('status missing') }
  const conflictsArr: Array<Object> | null = reqArray(data, 'conflicts')
  if (conflictsArr === null) { return err<SyncResult>('conflicts missing or not array') }
  const conflictsR: DecodeResult<SyncConflict[]> = decodeArray<SyncConflict>(conflictsArr as Object, decodeSyncConflict, 'conflicts')
  if (!conflictsR.ok || conflictsR.value === undefined) { return err<SyncResult>(conflictsR.error ?? 'decode failed') }
  const uploadedFiles: string[] | null = reqStrArray(data, 'uploadedFiles')
  if (uploadedFiles === null) { return err<SyncResult>('uploadedFiles missing or not string[]') }
  const downloadedFiles: string[] | null = reqStrArray(data, 'downloadedFiles')
  if (downloadedFiles === null) { return err<SyncResult>('downloadedFiles missing or not string[]') }
  const localDeletes: string[] | null = reqStrArray(data, 'localDeletes')
  if (localDeletes === null) { return err<SyncResult>('localDeletes missing or not string[]') }
  const remoteDeletes: string[] | null = reqStrArray(data, 'remoteDeletes')
  if (remoteDeletes === null) { return err<SyncResult>('remoteDeletes missing or not string[]') }
  const overwrittenFiles: string[] | null = reqStrArray(data, 'overwrittenFiles')
  if (overwrittenFiles === null) { return err<SyncResult>('overwrittenFiles missing or not string[]') }
  const ignoredFiles: string[] | null = reqStrArray(data, 'ignoredFiles')
  if (ignoredFiles === null) { return err<SyncResult>('ignoredFiles missing or not string[]') }
  const errorR: DecodeResult<string | null> = nullStr(data, 'error')
  if (!errorR.ok || errorR.value === undefined) { return err<SyncResult>(`SyncResult.${errorR.error ?? 'decode failed'}`) }
  const errorCategoryR: DecodeResult<string | null> = nullStr(data, 'errorCategory')
  if (!errorCategoryR.ok || errorCategoryR.value === undefined) { return err<SyncResult>(`SyncResult.${errorCategoryR.error ?? 'decode failed'}`) }
  const messageKeyR: DecodeResult<string | null> = nullStr(data, 'messageKey')
  if (!messageKeyR.ok || messageKeyR.value === undefined) { return err<SyncResult>(`SyncResult.${messageKeyR.error ?? 'decode failed'}`) }
  const searchIndexRebuildErrorR: DecodeResult<string | null> = nullStr(data, 'searchIndexRebuildError')
  if (!searchIndexRebuildErrorR.ok || searchIndexRebuildErrorR.value === undefined) { return err<SyncResult>(`SyncResult.${searchIndexRebuildErrorR.error ?? 'decode failed'}`) }
  const value: SyncResult = {
    status: status,
    uploadedFiles: uploadedFiles,
    downloadedFiles: downloadedFiles,
    localDeletes: localDeletes,
    remoteDeletes: remoteDeletes,
    overwrittenFiles: overwrittenFiles,
    ignoredFiles: ignoredFiles,
    conflicts: conflictsR.value,
    error: errorR.value,
    errorCategory: errorCategoryR.value,
    messageKey: messageKeyR.value,
    searchIndexRebuildError: searchIndexRebuildErrorR.value
  }
  return ok<SyncResult>(value)
}

function decodeSyncPlan(data: Object): DecodeResult<SyncPlan> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<SyncPlan>('SyncPlan: expected object')
  }
  const filesToUpload: string[] | null = reqStrArray(data, 'filesToUpload')
  if (filesToUpload === null) { return err<SyncPlan>('filesToUpload missing or not string[]') }
  const filesToDownload: string[] | null = reqStrArray(data, 'filesToDownload')
  if (filesToDownload === null) { return err<SyncPlan>('filesToDownload missing or not string[]') }
  const filesToDeleteLocal: string[] | null = reqStrArray(data, 'filesToDeleteLocal')
  if (filesToDeleteLocal === null) { return err<SyncPlan>('filesToDeleteLocal missing or not string[]') }
  const filesToDeleteRemote: string[] | null = reqStrArray(data, 'filesToDeleteRemote')
  if (filesToDeleteRemote === null) { return err<SyncPlan>('filesToDeleteRemote missing or not string[]') }
  const ignoredFiles: string[] | null = reqStrArray(data, 'ignoredFiles')
  if (ignoredFiles === null) { return err<SyncPlan>('ignoredFiles missing or not string[]') }
  const conflicts: string[] | null = reqStrArray(data, 'conflicts')
  if (conflicts === null) { return err<SyncPlan>('conflicts missing or not string[]') }
  const value: SyncPlan = {
    filesToUpload: filesToUpload,
    filesToDownload: filesToDownload,
    filesToDeleteLocal: filesToDeleteLocal,
    filesToDeleteRemote: filesToDeleteRemote,
    ignoredFiles: ignoredFiles,
    conflicts: conflicts
  }
  return ok<SyncPlan>(value)
}

function decodeFullSyncTargetResult(data: Object): DecodeResult<FullSyncTargetResult> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<FullSyncTargetResult>('FullSyncTargetResult: expected object')
  }
  const targetKind: string | null = reqStr(data, 'targetKind')
  if (targetKind === null) { return err<FullSyncTargetResult>('targetKind missing') }
  const remotePrefix: string | null = reqStr(data, 'remotePrefix')
  if (remotePrefix === null) { return err<FullSyncTargetResult>('remotePrefix missing') }
  const resultObj: Object | null = reqObj(data, 'result')
  if (resultObj === null) { return err<FullSyncTargetResult>('result missing or not object') }
  const resultR: DecodeResult<SyncResult> = decodeSyncResult(resultObj)
  if (!resultR.ok || resultR.value === undefined) { return err<FullSyncTargetResult>(`result: ${resultR.error}`) }
  const projectIdR: DecodeResult<string | null> = nullStr(data, 'projectId')
  if (!projectIdR.ok || projectIdR.value === undefined) { return err<FullSyncTargetResult>(`FullSyncTargetResult.${projectIdR.error ?? 'decode failed'}`) }
  const value: FullSyncTargetResult = {
    targetKind: targetKind,
    projectId: projectIdR.value,
    remotePrefix: remotePrefix,
    result: resultR.value
  }
  return ok<FullSyncTargetResult>(value)
}

function decodeFullSyncTargetPlan(data: Object): DecodeResult<FullSyncTargetPlan> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<FullSyncTargetPlan>('FullSyncTargetPlan: expected object')
  }
  const targetKind: string | null = reqStr(data, 'targetKind')
  if (targetKind === null) { return err<FullSyncTargetPlan>('targetKind missing') }
  const remotePrefix: string | null = reqStr(data, 'remotePrefix')
  if (remotePrefix === null) { return err<FullSyncTargetPlan>('remotePrefix missing') }
  const planObj: Object | null = reqObj(data, 'plan')
  if (planObj === null) { return err<FullSyncTargetPlan>('plan missing or not object') }
  const planR: DecodeResult<SyncPlan> = decodeSyncPlan(planObj)
  if (!planR.ok || planR.value === undefined) { return err<FullSyncTargetPlan>(`plan: ${planR.error}`) }
  const projectIdR: DecodeResult<string | null> = nullStr(data, 'projectId')
  if (!projectIdR.ok || projectIdR.value === undefined) { return err<FullSyncTargetPlan>(`FullSyncTargetPlan.${projectIdR.error ?? 'decode failed'}`) }
  const value: FullSyncTargetPlan = {
    targetKind: targetKind,
    projectId: projectIdR.value,
    remotePrefix: remotePrefix,
    plan: planR.value
  }
  return ok<FullSyncTargetPlan>(value)
}

export function decodeFullSyncResult(data: Object): DecodeResult<FullSyncResult> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<FullSyncResult>('FullSyncResult: expected object')
  }
  const overallStatus: string | null = reqStr(data, 'overallStatus')
  if (overallStatus === null) { return err<FullSyncResult>('overallStatus missing') }
  const targetsArr: Array<Object> | null = reqArray(data, 'targets')
  if (targetsArr === null) { return err<FullSyncResult>('targets missing or not array') }
  const targetsR: DecodeResult<FullSyncTargetResult[]> = decodeArray<FullSyncTargetResult>(targetsArr as Object, decodeFullSyncTargetResult, 'targets')
  if (!targetsR.ok || targetsR.value === undefined) { return err<FullSyncResult>(targetsR.error ?? 'decode failed') }
  const errorR: DecodeResult<string | null> = nullStr(data, 'error')
  if (!errorR.ok || errorR.value === undefined) { return err<FullSyncResult>(`FullSyncResult.${errorR.error ?? 'decode failed'}`) }
  const errorCategoryR: DecodeResult<string | null> = nullStr(data, 'errorCategory')
  if (!errorCategoryR.ok || errorCategoryR.value === undefined) { return err<FullSyncResult>(`FullSyncResult.${errorCategoryR.error ?? 'decode failed'}`) }
  const messageKeyR: DecodeResult<string | null> = nullStr(data, 'messageKey')
  if (!messageKeyR.ok || messageKeyR.value === undefined) { return err<FullSyncResult>(`FullSyncResult.${messageKeyR.error ?? 'decode failed'}`) }
  const totalUploaded: number | null = reqNum(data, 'totalUploaded')
  if (totalUploaded === null) { return err<FullSyncResult>('FullSyncResult.totalUploaded missing or not number') }
  const totalDownloaded: number | null = reqNum(data, 'totalDownloaded')
  if (totalDownloaded === null) { return err<FullSyncResult>('FullSyncResult.totalDownloaded missing or not number') }
  const totalLocalDeletes: number | null = reqNum(data, 'totalLocalDeletes')
  if (totalLocalDeletes === null) { return err<FullSyncResult>('FullSyncResult.totalLocalDeletes missing or not number') }
  const totalRemoteDeletes: number | null = reqNum(data, 'totalRemoteDeletes')
  if (totalRemoteDeletes === null) { return err<FullSyncResult>('FullSyncResult.totalRemoteDeletes missing or not number') }
  const totalOverwritten: number | null = reqNum(data, 'totalOverwritten')
  if (totalOverwritten === null) { return err<FullSyncResult>('FullSyncResult.totalOverwritten missing or not number') }
  const totalIgnored: number | null = reqNum(data, 'totalIgnored')
  if (totalIgnored === null) { return err<FullSyncResult>('FullSyncResult.totalIgnored missing or not number') }
  const totalConflicts: number | null = reqNum(data, 'totalConflicts')
  if (totalConflicts === null) { return err<FullSyncResult>('FullSyncResult.totalConflicts missing or not number') }
  const value: FullSyncResult = {
    overallStatus: overallStatus,
    targets: targetsR.value,
    totalUploaded: totalUploaded,
    totalDownloaded: totalDownloaded,
    totalLocalDeletes: totalLocalDeletes,
    totalRemoteDeletes: totalRemoteDeletes,
    totalOverwritten: totalOverwritten,
    totalIgnored: totalIgnored,
    totalConflicts: totalConflicts,
    error: errorR.value,
    errorCategory: errorCategoryR.value,
    messageKey: messageKeyR.value
  }
  return ok<FullSyncResult>(value)
}

export function decodeFullSyncDryRunResult(data: Object): DecodeResult<FullSyncDryRunResult> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<FullSyncDryRunResult>('FullSyncDryRunResult: expected object')
  }
  const targetsArr: Array<Object> | null = reqArray(data, 'targets')
  if (targetsArr === null) { return err<FullSyncDryRunResult>('targets missing or not array') }
  const targetsR: DecodeResult<FullSyncTargetPlan[]> = decodeArray<FullSyncTargetPlan>(targetsArr as Object, decodeFullSyncTargetPlan, 'targets')
  if (!targetsR.ok || targetsR.value === undefined) { return err<FullSyncDryRunResult>(targetsR.error ?? 'decode failed') }
  const totalToUpload: number | null = reqNum(data, 'totalToUpload')
  if (totalToUpload === null) { return err<FullSyncDryRunResult>('FullSyncDryRunResult.totalToUpload missing or not number') }
  const totalToDownload: number | null = reqNum(data, 'totalToDownload')
  if (totalToDownload === null) { return err<FullSyncDryRunResult>('FullSyncDryRunResult.totalToDownload missing or not number') }
  const totalToDeleteLocal: number | null = reqNum(data, 'totalToDeleteLocal')
  if (totalToDeleteLocal === null) { return err<FullSyncDryRunResult>('FullSyncDryRunResult.totalToDeleteLocal missing or not number') }
  const totalToDeleteRemote: number | null = reqNum(data, 'totalToDeleteRemote')
  if (totalToDeleteRemote === null) { return err<FullSyncDryRunResult>('FullSyncDryRunResult.totalToDeleteRemote missing or not number') }
  const totalIgnored: number | null = reqNum(data, 'totalIgnored')
  if (totalIgnored === null) { return err<FullSyncDryRunResult>('FullSyncDryRunResult.totalIgnored missing or not number') }
  const totalConflicts: number | null = reqNum(data, 'totalConflicts')
  if (totalConflicts === null) { return err<FullSyncDryRunResult>('FullSyncDryRunResult.totalConflicts missing or not number') }
  const value: FullSyncDryRunResult = {
    targets: targetsR.value,
    totalToUpload: totalToUpload,
    totalToDownload: totalToDownload,
    totalToDeleteLocal: totalToDeleteLocal,
    totalToDeleteRemote: totalToDeleteRemote,
    totalIgnored: totalIgnored,
    totalConflicts: totalConflicts
  }
  return ok<FullSyncDryRunResult>(value)
}

function decodeSyncDiagnosticsResult(data: Object): DecodeResult<SyncDiagnosticsResult> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<SyncDiagnosticsResult>('SyncDiagnosticsResult: expected object')
  }
  const success: boolean | null = reqBool(data, 'success')
  if (success === null) { return err<SyncDiagnosticsResult>('success missing or not boolean') }
  const providerType: string | null = reqStr(data, 'providerType')
  if (providerType === null) { return err<SyncDiagnosticsResult>('providerType missing or not string') }
  const hasNetworkPermission: boolean | null = reqBool(data, 'hasNetworkPermission')
  if (hasNetworkPermission === null) { return err<SyncDiagnosticsResult>('hasNetworkPermission missing or not boolean') }
  const hasNetworkStatePermission: boolean | null = reqBool(data, 'hasNetworkStatePermission')
  if (hasNetworkStatePermission === null) { return err<SyncDiagnosticsResult>('hasNetworkStatePermission missing or not boolean') }
  const networkState: string | null = reqStr(data, 'networkState')
  if (networkState === null) { return err<SyncDiagnosticsResult>('networkState missing or not string') }
  const networkOk: boolean | null = reqBool(data, 'networkOk')
  if (networkOk === null) { return err<SyncDiagnosticsResult>('networkOk missing or not boolean') }
  const authOk: boolean | null = reqBool(data, 'authOk')
  if (authOk === null) { return err<SyncDiagnosticsResult>('authOk missing or not boolean') }
  const remoteOk: boolean | null = reqBool(data, 'remoteOk')
  if (remoteOk === null) { return err<SyncDiagnosticsResult>('remoteOk missing or not boolean') }
  const networkStatus: string | null = reqStr(data, 'networkStatus')
  if (networkStatus === null) { return err<SyncDiagnosticsResult>('networkStatus missing or not string') }
  const authStatus: string | null = reqStr(data, 'authStatus')
  if (authStatus === null) { return err<SyncDiagnosticsResult>('authStatus missing or not string') }
  const errorCategory: string | null = reqStr(data, 'errorCategory')
  if (errorCategory === null) { return err<SyncDiagnosticsResult>('errorCategory missing or not string') }
  const rawErrorR: DecodeResult<string | null> = nullStr(data, 'rawError')
  if (!rawErrorR.ok || rawErrorR.value === undefined) { return err<SyncDiagnosticsResult>(`SyncDiagnosticsResult.${rawErrorR.error ?? 'decode failed'}`) }
  const providerDetailsR: DecodeResult<string | null> = nullStr(data, 'providerDetails')
  if (!providerDetailsR.ok || providerDetailsR.value === undefined) { return err<SyncDiagnosticsResult>(`SyncDiagnosticsResult.${providerDetailsR.error ?? 'decode failed'}`) }
  const value: SyncDiagnosticsResult = {
    success: success,
    providerType: providerType,
    hasNetworkPermission: hasNetworkPermission,
    hasNetworkStatePermission: hasNetworkStatePermission,
    networkState: networkState,
    networkOk: networkOk,
    authOk: authOk,
    remoteOk: remoteOk,
    networkStatus: networkStatus,
    authStatus: authStatus,
    errorCategory: errorCategory,
    rawError: rawErrorR.value,
    providerDetails: providerDetailsR.value
  }
  return ok<SyncDiagnosticsResult>(value)
}

export function decodeFullSyncDiagnosticsResult(data: Object): DecodeResult<FullSyncDiagnosticsResult> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<FullSyncDiagnosticsResult>('FullSyncDiagnosticsResult: expected object')
  }
  const diagObj: Object | null = reqObj(data, 'diagnostics')
  if (diagObj === null) { return err<FullSyncDiagnosticsResult>('diagnostics missing or not object') }
  const diagR: DecodeResult<SyncDiagnosticsResult> = decodeSyncDiagnosticsResult(diagObj)
  if (!diagR.ok || diagR.value === undefined) { return err<FullSyncDiagnosticsResult>(`diagnostics: ${diagR.error}`) }
  return ok<FullSyncDiagnosticsResult>({ diagnostics: diagR.value })
}

// ─── App State / Recent Edits Decoders ───

export function decodeRecentEdit(data: Object): DecodeResult<RecentEdit> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<RecentEdit>('RecentEdit: expected object')
  }
  const projectId: string | null = reqStr(data, 'projectId')
  if (projectId === null) { return err<RecentEdit>('projectId missing') }
  const volumeId: string | null = reqStr(data, 'volumeId')
  if (volumeId === null) { return err<RecentEdit>('volumeId missing') }
  const chapterId: string | null = reqStr(data, 'chapterId')
  if (chapterId === null) { return err<RecentEdit>('chapterId missing') }
  const timestamp: string | null = reqStr(data, 'timestamp')
  if (timestamp === null) { return err<RecentEdit>('timestamp missing') }
  return ok<RecentEdit>({ projectId: projectId, volumeId: volumeId, chapterId: chapterId, timestamp: timestamp })
}

export function decodeAppSummary(data: Object): DecodeResult<AppSummary> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<AppSummary>('AppSummary: expected object')
  }
  const projectsArr: Array<Object> | null = reqArray(data, 'projects')
  if (projectsArr === null) { return err<AppSummary>('projects missing or not array') }
  const projectsR: DecodeResult<ProjectSummary[]> = decodeArray<ProjectSummary>(projectsArr as Object, decodeProjectSummary, 'projects')
  if (!projectsR.ok || projectsR.value === undefined) { return err<AppSummary>(projectsR.error ?? 'decode failed') }
  const recentEditObjR: DecodeResult<Object | null> = nullObj(data, 'recentEdit')
  if (!recentEditObjR.ok || recentEditObjR.value === undefined) { return err<AppSummary>(`AppSummary.${recentEditObjR.error ?? 'decode failed'}`) }
  if (recentEditObjR.value === null) {
    return ok<AppSummary>({ projects: projectsR.value, recentEdit: null })
  }
  const reR: DecodeResult<RecentEdit> = decodeRecentEdit(recentEditObjR.value)
  if (!reR.ok || reR.value === undefined) { return err<AppSummary>(`recentEdit: ${reR.error}`) }
  return ok<AppSummary>({ projects: projectsR.value, recentEdit: reR.value })
}

// ─── Editor Session Decoders ───
// Core api/types/editor.rs 的编辑 DTO（EditorEditResultDto / EditorSessionSnapshotDto）。
// skip_serializing_if 的可选子结构（offsetMap / compositionSession / composition）
// 用 opt* 取值：Core 省略该键时 ArkTS 侧保持 undefined，与 interface 的 `?` 一致。

function decodeEditorContentDelta(data: Object): DecodeResult<import('../dto/EditorDtos').EditorContentDelta> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err('EditorContentDelta: expected object')
  }
  const insertedChars: number | null = reqNum(data, 'insertedChars')
  if (insertedChars === null) { return err('EditorContentDelta.insertedChars missing') }
  const deletedChars: number | null = reqNum(data, 'deletedChars')
  if (deletedChars === null) { return err('EditorContentDelta.deletedChars missing') }
  const insertedNonWhitespaceChars: number | null = reqNum(data, 'insertedNonWhitespaceChars')
  if (insertedNonWhitespaceChars === null) { return err('EditorContentDelta.insertedNonWhitespaceChars missing') }
  const deletedNonWhitespaceChars: number | null = reqNum(data, 'deletedNonWhitespaceChars')
  if (deletedNonWhitespaceChars === null) { return err('EditorContentDelta.deletedNonWhitespaceChars missing') }
  const value: import('../dto/EditorDtos').EditorContentDelta = {
    insertedChars: insertedChars,
    deletedChars: deletedChars,
    insertedNonWhitespaceChars: insertedNonWhitespaceChars,
    deletedNonWhitespaceChars: deletedNonWhitespaceChars
  }; return ok(value)
}

function decodeDisplayPatch(data: Object): DecodeResult<import('../dto/EditorDtos').DisplayPatch> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err('DisplayPatch: expected object')
  }
  const baseRevision: number | null = reqNum(data, 'baseRevision')
  if (baseRevision === null) { return err('DisplayPatch.baseRevision missing') }
  const newRevision: number | null = reqNum(data, 'newRevision')
  if (newRevision === null) { return err('DisplayPatch.newRevision missing') }
  const replaceByteStart: number | null = reqNum(data, 'replaceByteStart')
  if (replaceByteStart === null) { return err('DisplayPatch.replaceByteStart missing') }
  const replaceByteEndExclusive: number | null = reqNum(data, 'replaceByteEndExclusive')
  if (replaceByteEndExclusive === null) { return err('DisplayPatch.replaceByteEndExclusive missing') }
  const insertedText: string | null = reqStr(data, 'insertedText')
  if (insertedText === null) { return err('DisplayPatch.insertedText missing') }
  const resultingSelectionStart: number | null = reqNum(data, 'resultingSelectionStart')
  if (resultingSelectionStart === null) { return err('DisplayPatch.resultingSelectionStart missing') }
  const resultingSelectionEnd: number | null = reqNum(data, 'resultingSelectionEnd')
  if (resultingSelectionEnd === null) { return err('DisplayPatch.resultingSelectionEnd missing') }
  const value: import('../dto/EditorDtos').DisplayPatch = {
    baseRevision: baseRevision,
    newRevision: newRevision,
    replaceByteStart: replaceByteStart,
    replaceByteEndExclusive: replaceByteEndExclusive,
    insertedText: insertedText,
    resultingSelectionStart: resultingSelectionStart,
    resultingSelectionEnd: resultingSelectionEnd
  }; return ok(value)
}

function decodeCompositionSession(data: Object): DecodeResult<import('../dto/EditorDtos').CompositionSession> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err('CompositionSession: expected object')
  }
  const sessionId: number | null = reqNum(data, 'sessionId')
  if (sessionId === null) { return err('CompositionSession.sessionId missing') }
  const baseRevision: number | null = reqNum(data, 'baseRevision')
  if (baseRevision === null) { return err('CompositionSession.baseRevision missing') }
  const generation: number | null = reqNum(data, 'generation')
  if (generation === null) { return err('CompositionSession.generation missing') }
  const value: import('../dto/EditorDtos').CompositionSession = { sessionId: sessionId, baseRevision: baseRevision, generation: generation }
  return ok(value)
}

function decodeEditorCompositionState(data: Object): DecodeResult<import('../dto/EditorDtos').EditorCompositionState> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err('EditorCompositionState: expected object')
  }
  const sessionId: number | null = reqNum(data, 'sessionId')
  if (sessionId === null) { return err('EditorCompositionState.sessionId missing') }
  const baseRevision: number | null = reqNum(data, 'baseRevision')
  if (baseRevision === null) { return err('EditorCompositionState.baseRevision missing') }
  const generation: number | null = reqNum(data, 'generation')
  if (generation === null) { return err('EditorCompositionState.generation missing') }
  const replaceByteStart: number | null = reqNum(data, 'replaceByteStart')
  if (replaceByteStart === null) { return err('EditorCompositionState.replaceByteStart missing') }
  const replaceByteEndExclusive: number | null = reqNum(data, 'replaceByteEndExclusive')
  if (replaceByteEndExclusive === null) { return err('EditorCompositionState.replaceByteEndExclusive missing') }
  const preeditText: string | null = reqStr(data, 'preeditText')
  if (preeditText === null) { return err('EditorCompositionState.preeditText missing') }
  const preeditCursorUtf16: number | null = reqNum(data, 'preeditCursorUtf16')
  if (preeditCursorUtf16 === null) { return err('EditorCompositionState.preeditCursorUtf16 missing') }
  const value: import('../dto/EditorDtos').EditorCompositionState = {
    sessionId: sessionId,
    baseRevision: baseRevision,
    generation: generation,
    replaceByteStart: replaceByteStart,
    replaceByteEndExclusive: replaceByteEndExclusive,
    preeditText: preeditText,
    preeditCursorUtf16: preeditCursorUtf16
  }; return ok(value)
}

function decodeOffsetMapEntry(data: Object): DecodeResult<import('../dto/EditorDtos').OffsetMapEntry> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err('OffsetMapEntry: expected object')
  }
  const oldByteOffset: number | null = reqNum(data, 'oldByteOffset')
  if (oldByteOffset === null) { return err('OffsetMapEntry.oldByteOffset missing') }
  const newByteOffset: number | null = reqNum(data, 'newByteOffset')
  if (newByteOffset === null) { return err('OffsetMapEntry.newByteOffset missing') }
  const length: number | null = reqNum(data, 'length')
  if (length === null) { return err('OffsetMapEntry.length missing') }
  const kind: string | null = reqStr(data, 'kind')
  if (kind === null) { return err('OffsetMapEntry.kind missing') }
  const value: import('../dto/EditorDtos').OffsetMapEntry = {
    oldByteOffset: oldByteOffset,
    newByteOffset: newByteOffset,
    length: length,
    kind: kind as import('../dto/EditorDtos').OffsetMapKind
  }; return ok(value)
}

function decodeOffsetMap(data: Object): DecodeResult<import('../dto/EditorDtos').OffsetMap> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err('OffsetMap: expected object')
  }
  const entriesArr: Array<Object> | null = reqArray(data, 'entries')
  if (entriesArr === null) { return err('OffsetMap.entries missing or not array') }
  const entriesR = decodeArray<import('../dto/EditorDtos').OffsetMapEntry>(entriesArr as Object, decodeOffsetMapEntry, 'entries')
  if (!entriesR.ok || entriesR.value === undefined) { return err(`entries: ${entriesR.error}`) }
  const value: import('../dto/EditorDtos').OffsetMap = { entries: entriesR.value }
  return ok(value)
}

export function decodeEditorSessionSnapshot(data: Object): DecodeResult<import('../dto/EditorDtos').EditorSessionSnapshot> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err('EditorSessionSnapshot: expected object')
  }
  const text: string | null = reqStr(data, 'text')
  if (text === null) { return err('text missing') }
  const revision: number | null = reqNum(data, 'revision')
  if (revision === null) { return err('revision missing') }
  const cursor: number | null = reqNum(data, 'cursor')
  if (cursor === null) { return err('cursor missing') }
  const selectionAnchor: number | null = reqNum(data, 'selectionAnchor')
  if (selectionAnchor === null) { return err('selectionAnchor missing') }
  const generation: number | null = reqNum(data, 'generation')
  if (generation === null) { return err('generation missing') }
  const chapterId: string | null = reqStr(data, 'chapterId')
  if (chapterId === null) { return err('chapterId missing') }
  let composition: import('../dto/EditorDtos').EditorCompositionState | undefined = undefined
  const compositionObjR: DecodeResult<Object | undefined> = optObj(data, 'composition')
  if (!compositionObjR.ok) { return err(`composition: ${compositionObjR.error ?? 'decode failed'}`) }
  if (compositionObjR.value !== undefined) {
    const compositionR = decodeEditorCompositionState(compositionObjR.value)
    if (!compositionR.ok || compositionR.value === undefined) { return err(`composition: ${compositionR.error}`) }
    composition = compositionR.value
  }
  const value: import('../dto/EditorDtos').EditorSessionSnapshot = {
    text: text,
    revision: revision,
    cursor: cursor,
    selectionAnchor: selectionAnchor,
    generation: generation,
    chapterId: chapterId,
    composition: composition
  }; return ok(value)
}

export function decodeEditorEditResult(data: Object): DecodeResult<import('../dto/EditorDtos').EditorEditResult> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err('EditorEditResult: expected object')
  }
  const outcome: string | null = reqStr(data, 'outcome')
  if (outcome === null) { return err('outcome missing') }
  const transactionId: number | null = reqNum(data, 'transactionId')
  if (transactionId === null) { return err('transactionId missing') }
  const baseRevision: number | null = reqNum(data, 'baseRevision')
  if (baseRevision === null) { return err('baseRevision missing') }
  const newRevision: number | null = reqNum(data, 'newRevision')
  if (newRevision === null) { return err('newRevision missing') }
  const patchesArr: Array<Object> | null = reqArray(data, 'displayPatches')
  if (patchesArr === null) { return err('displayPatches missing or not array') }
  const patchesR = decodeArray<import('../dto/EditorDtos').DisplayPatch>(patchesArr as Object, decodeDisplayPatch, 'displayPatches')
  if (!patchesR.ok || patchesR.value === undefined) { return err(patchesR.error ?? 'decode failed') }
  const oldSelectionAnchor: number | null = reqNum(data, 'oldSelectionAnchor')
  if (oldSelectionAnchor === null) { return err('oldSelectionAnchor missing') }
  const oldSelectionHead: number | null = reqNum(data, 'oldSelectionHead')
  if (oldSelectionHead === null) { return err('oldSelectionHead missing') }
  const newSelectionAnchor: number | null = reqNum(data, 'newSelectionAnchor')
  if (newSelectionAnchor === null) { return err('newSelectionAnchor missing') }
  const newSelectionHead: number | null = reqNum(data, 'newSelectionHead')
  if (newSelectionHead === null) { return err('newSelectionHead missing') }
  const cause: string | null = reqStr(data, 'cause')
  if (cause === null) { return err('cause missing') }
  const operationKind: string | null = reqStr(data, 'operationKind')
  if (operationKind === null) { return err('operationKind missing') }
  const contentDeltaObj: Object | null = reqObj(data, 'contentDelta')
  if (contentDeltaObj === null) { return err('contentDelta missing or not object') }
  const contentDeltaR = decodeEditorContentDelta(contentDeltaObj)
  if (!contentDeltaR.ok || contentDeltaR.value === undefined) { return err(`contentDelta: ${contentDeltaR.error}`) }

  let offsetMap: import('../dto/EditorDtos').OffsetMap | undefined = undefined
  const offsetMapObjR: DecodeResult<Object | undefined> = optObj(data, 'offsetMap')
  if (!offsetMapObjR.ok) { return err(`offsetMap: ${offsetMapObjR.error ?? 'decode failed'}`) }
  if (offsetMapObjR.value !== undefined) {
    const offsetMapR = decodeOffsetMap(offsetMapObjR.value)
    if (!offsetMapR.ok || offsetMapR.value === undefined) { return err(`offsetMap: ${offsetMapR.error}`) }
    offsetMap = offsetMapR.value
  }
  let compositionSession: import('../dto/EditorDtos').CompositionSession | undefined = undefined
  const compositionSessionObjR: DecodeResult<Object | undefined> = optObj(data, 'compositionSession')
  if (!compositionSessionObjR.ok) { return err(`compositionSession: ${compositionSessionObjR.error ?? 'decode failed'}`) }
  if (compositionSessionObjR.value !== undefined) {
    const compositionSessionR = decodeCompositionSession(compositionSessionObjR.value)
    if (!compositionSessionR.ok || compositionSessionR.value === undefined) { return err(`compositionSession: ${compositionSessionR.error}`) }
    compositionSession = compositionSessionR.value
  }
  let composition: import('../dto/EditorDtos').EditorCompositionState | undefined = undefined
  const compositionObjR: DecodeResult<Object | undefined> = optObj(data, 'composition')
  if (!compositionObjR.ok) { return err(`composition: ${compositionObjR.error ?? 'decode failed'}`) }
  if (compositionObjR.value !== undefined) {
    const compositionR = decodeEditorCompositionState(compositionObjR.value)
    if (!compositionR.ok || compositionR.value === undefined) { return err(`composition: ${compositionR.error}`) }
    composition = compositionR.value
  }

  const value: import('../dto/EditorDtos').EditorEditResult = {
    outcome: outcome as import('../dto/EditorDtos').EditorEditOutcome,
    transactionId: transactionId,
    baseRevision: baseRevision,
    newRevision: newRevision,
    displayPatches: patchesR.value,
    oldSelectionAnchor: oldSelectionAnchor,
    oldSelectionHead: oldSelectionHead,
    newSelectionAnchor: newSelectionAnchor,
    newSelectionHead: newSelectionHead,
    cause: cause as import('../dto/EditorDtos').EditorTransactionCause,
    operationKind: operationKind as import('../dto/EditorDtos').EditorOperationKind,
    offsetMap: offsetMap,
    compositionSession: compositionSession,
    contentDelta: contentDeltaR.value,
    composition: composition
  }; return ok(value)
}

// ─── Layout / Screen Policy Decoders ───

function decodeLayoutMetrics(data: Object): DecodeResult<LayoutMetrics> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<LayoutMetrics>('LayoutMetrics: expected object')
  }
  const listPaneWidthDp: number | null = reqNum(data, 'listPaneWidthDp')
  if (listPaneWidthDp === null) { return err<LayoutMetrics>('listPaneWidthDp missing') }
  const projectCardMinWidthDp: number | null = reqNum(data, 'projectCardMinWidthDp')
  if (projectCardMinWidthDp === null) { return err<LayoutMetrics>('projectCardMinWidthDp missing') }
  const toolPaneWidthDp: number | null = reqNum(data, 'toolPaneWidthDp')
  if (toolPaneWidthDp === null) { return err<LayoutMetrics>('toolPaneWidthDp missing') }
  const toolRailWidthDp: number | null = reqNum(data, 'toolRailWidthDp')
  if (toolRailWidthDp === null) { return err<LayoutMetrics>('toolRailWidthDp missing') }
  const editorMinWidthDp: number | null = reqNum(data, 'editorMinWidthDp')
  if (editorMinWidthDp === null) { return err<LayoutMetrics>('editorMinWidthDp missing') }
  const toolbarHeightDp: number | null = reqNum(data, 'toolbarHeightDp')
  if (toolbarHeightDp === null) { return err<LayoutMetrics>('toolbarHeightDp missing') }
  const toolbarLeadingWidthDp: number | null = reqNum(data, 'toolbarLeadingWidthDp')
  if (toolbarLeadingWidthDp === null) { return err<LayoutMetrics>('toolbarLeadingWidthDp missing') }
  const toolbarTrailingWidthDp: number | null = reqNum(data, 'toolbarTrailingWidthDp')
  if (toolbarTrailingWidthDp === null) { return err<LayoutMetrics>('toolbarTrailingWidthDp missing') }
  const listPaneMinWidthDp: number | null = reqNum(data, 'listPaneMinWidthDp')
  if (listPaneMinWidthDp === null) { return err<LayoutMetrics>('listPaneMinWidthDp missing') }
  const toolPaneMinWidthDp: number | null = reqNum(data, 'toolPaneMinWidthDp')
  if (toolPaneMinWidthDp === null) { return err<LayoutMetrics>('toolPaneMinWidthDp missing') }
  return ok<LayoutMetrics>({
    listPaneWidthDp: listPaneWidthDp,
    projectCardMinWidthDp: projectCardMinWidthDp,
    toolPaneWidthDp: toolPaneWidthDp,
    toolRailWidthDp: toolRailWidthDp,
    editorMinWidthDp: editorMinWidthDp,
    toolbarHeightDp: toolbarHeightDp,
    toolbarLeadingWidthDp: toolbarLeadingWidthDp,
    toolbarTrailingWidthDp: toolbarTrailingWidthDp,
    listPaneMinWidthDp: listPaneMinWidthDp,
    toolPaneMinWidthDp: toolPaneMinWidthDp
  })
}

export function decodeLayoutContract(data: Object): DecodeResult<LayoutContract> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<LayoutContract>('LayoutContract: expected object')
  }
  const shellMode: string | null = reqStr(data, 'shellMode')
  if (shellMode === null) { return err<LayoutContract>('shellMode missing') }
  const workspaceLayoutMode: string | null = reqStr(data, 'workspaceLayoutMode')
  if (workspaceLayoutMode === null) { return err<LayoutContract>('workspaceLayoutMode missing') }
  const primaryNavigationPlacement: string | null = reqStr(data, 'primaryNavigationPlacement')
  if (primaryNavigationPlacement === null) { return err<LayoutContract>('primaryNavigationPlacement missing') }
  const metricsObj: Object | null = reqObj(data, 'metrics')
  if (metricsObj === null) { return err<LayoutContract>('metrics missing or not object') }
  const metricsR: DecodeResult<LayoutMetrics> = decodeLayoutMetrics(metricsObj)
  if (!metricsR.ok || metricsR.value === undefined) { return err<LayoutContract>(`metrics: ${metricsR.error}`) }
  const value: LayoutContract = {
    shellMode: shellMode as ShellMode,
    workspaceLayoutMode: workspaceLayoutMode as WorkspaceLayoutMode,
    primaryNavigationPlacement: primaryNavigationPlacement as PrimaryNavigationPlacement,
    metrics: metricsR.value
  }
  return ok<LayoutContract>(value)
}

function decodeActionSlot(data: Object): DecodeResult<ActionSlot> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<ActionSlot>('ActionSlot: expected object')
  }
  const role: string | null = reqStr(data, 'role')
  if (role === null) { return err<ActionSlot>('role missing') }
  const target: string | null = reqStr(data, 'target')
  if (target === null) { return err<ActionSlot>('target missing') }
  const region: string | null = reqStr(data, 'region')
  if (region === null) { return err<ActionSlot>('region missing') }
  const order: number | null = reqNum(data, 'order')
  if (order === null) { return err<ActionSlot>('order missing') }
  const requiresConfirmation: boolean | null = reqBool(data, 'requiresConfirmation')
  if (requiresConfirmation === null) { return err<ActionSlot>('requiresConfirmation missing') }
  const value: ActionSlot = {
    role: role as ActionRole,
    target: target as ActionTarget,
    region: region as ActionRegion,
    order: order,
    requiresConfirmation: requiresConfirmation
  }
  return ok<ActionSlot>(value)
}

export function decodeScreenPolicy(data: Object): DecodeResult<ScreenPolicy> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<ScreenPolicy>('ScreenPolicy: expected object')
  }
  const screenRole: string | null = reqStr(data, 'screenRole')
  if (screenRole === null) { return err<ScreenPolicy>('screenRole missing') }
  const showPrimaryNavigation: boolean | null = reqBool(data, 'showPrimaryNavigation')
  if (showPrimaryNavigation === null) { return err<ScreenPolicy>('showPrimaryNavigation missing or not boolean') }
  const slotsArr: Array<Object> | null = reqArray(data, 'actionSlots')
  if (slotsArr === null) { return err<ScreenPolicy>('actionSlots missing or not array') }
  const slotsR: DecodeResult<ActionSlot[]> = decodeArray<ActionSlot>(slotsArr as Object, decodeActionSlot, 'actionSlots')
  if (!slotsR.ok || slotsR.value === undefined) { return err<ScreenPolicy>(slotsR.error ?? 'decode failed') }
  const value: ScreenPolicy = {
    screenRole: screenRole as ScreenRole,
    showPrimaryNavigation: showPrimaryNavigation,
    actionSlots: slotsR.value
  }
  return ok<ScreenPolicy>(value)
}
