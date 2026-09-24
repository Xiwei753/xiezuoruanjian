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
import { Project, Volume, ChapterMeta, ChapterContent, ChapterSaveReceipt, ProjectTree, VolumeTree, ProjectStats, WritingStats } from '../dto/ProjectDtos'
import { StarMapMeta, StarMapGraph, StarMapNode, StarMapEdge, StarMapEmbed, StarMapLink, StarMapLayout, StarMapLayoutNode, StarMapEdgeRender, StarMapMotionPolicy, NodeType, EdgeKind } from '../dto/StarMapDtos'
import { SyncConfig, SyncState, SyncConflict, SyncResult, SyncDiagnosticsResult, FullSyncResult, FullSyncTargetResult, FullSyncDryRunResult, FullSyncTargetPlan, SyncPlan, FullSyncDiagnosticsResult } from '../dto/SyncDtos'
import { RecentEdit, AppSummary } from '../dto/ResultEnvelope'
import { LayoutContract, ScreenPolicy, ShellMode, WorkspacePaneMode, ScreenRole, ActionSlot, ActionRole, ActionTarget, ActionRegion } from '../dto/PlatformDtos'

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

function optStr(obj: Object, key: string): string | undefined {
  const rec = obj as Record<string, Object>
  const v: Object | undefined = rec[key]
  if (v === undefined || v === null) { return undefined }
  if (typeof v === 'string') { return v }
  return undefined
}

function optNum(obj: Object, key: string): number | undefined {
  const rec = obj as Record<string, Object>
  const v: Object | undefined = rec[key]
  if (v === undefined || v === null) { return undefined }
  if (typeof v === 'number') { return v }
  return undefined
}

function optBool(obj: Object, key: string): boolean | undefined {
  const rec = obj as Record<string, Object>
  const v: Object | undefined = rec[key]
  if (v === undefined || v === null) { return undefined }
  if (typeof v === 'boolean') { return v }
  return undefined
}

function optObj(obj: Object, key: string): Object | undefined {
  const rec = obj as Record<string, Object>
  const v: Object | undefined = rec[key]
  if (v === undefined || v === null) { return undefined }
  if (typeof v === 'object' && !Array.isArray(v)) { return v }
  return undefined
}

function reqArray(obj: Object, key: string): Array<Object> | null {
  const rec = obj as Record<string, Object>
  const v: Object | undefined = rec[key]
  if (Array.isArray(v)) { return v as Array<Object> }
  return null
}

function optArray(obj: Object, key: string): Array<Object> | undefined {
  const rec = obj as Record<string, Object>
  const v: Object | undefined = rec[key]
  if (v === undefined || v === null) { return undefined }
  if (Array.isArray(v)) { return v as Array<Object> }
  return undefined
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

  const value: LocalSettings = {
    themeMode: optStr(data, 'themeMode'),
    appearanceMode: appearanceMode,
    colorSource: colorSource,
    dynamicColorEnabled: dynamicColorEnabled,
    selectedBuiltinThemeId: selectedBuiltinThemeId,
    selectedPaletteId: selectedPaletteId,
    locale: optStr(data, 'locale'),
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
    statsDeviceId: optStr(data, 'statsDeviceId'),
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
    'outline', 'outline_variant', 'scrim'
  ]
  const value: ThemeColorSchemeDto = {} as ThemeColorSchemeDto
  for (let i = 0; i < required.length; i++) {
    const key: string = required[i]
    const v: string | null = reqStr(data, key)
    if (v === null) { return err<ThemeColorSchemeDto>(`ThemeColorScheme.${key} missing or not string`) }
    (value as Record<string, string>)[key] = v
  }
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
  const value: ChapterMeta = {
    id: id,
    title: title,
    createdAt: createdAt,
    updatedAt: updatedAt,
    order: order,
    wordCount: wordCount,
    hash: hash,
    note: optStr(data, 'note')
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
  const totalChars: number | null = reqNum(data, 'totalChars')
  if (totalChars === null) { return err<WritingStats>('totalChars missing or not number') }
  const totalWords: number | null = reqNum(data, 'totalWords')
  if (totalWords === null) { return err<WritingStats>('totalWords missing or not number') }
  const totalChapters: number | null = reqNum(data, 'totalChapters')
  if (totalChapters === null) { return err<WritingStats>('totalChapters missing or not number') }
  const todayChars: number | null = reqNum(data, 'todayChars')
  if (todayChars === null) { return err<WritingStats>('todayChars missing or not number') }
  const todayDuration: number | null = reqNum(data, 'todayDuration')
  if (todayDuration === null) { return err<WritingStats>('todayDuration missing or not number') }
  const averageSpeed: number | null = reqNum(data, 'averageSpeed')
  if (averageSpeed === null) { return err<WritingStats>('averageSpeed missing or not number') }
  const value: WritingStats = {
    totalChars: totalChars,
    totalWords: totalWords,
    totalChapters: totalChapters,
    todayChars: todayChars,
    todayDuration: todayDuration,
    averageSpeed: averageSpeed
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
    if (!chR.ok || chR.value === undefined) { return err<ProjectTree>(chR.error) }
    volumes.push({ volume: volR.value, chapters: chR.value })
  }
  const statsObj: Object | undefined = optObj(data, 'stats')
  let stats: ProjectStats | undefined = undefined
  if (statsObj !== undefined) {
    const statsR: DecodeResult<ProjectStats> = decodeProjectStats(statsObj)
    if (!statsR.ok || statsR.value === undefined) { return err<ProjectTree>(`stats: ${statsR.error}`) }
    stats = statsR.value
  }
  const value: ProjectTree = {
    project: projectR.value,
    volumes: volumes,
    stats: stats
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
  const projectId: string | null = optStr(data, 'projectId')
  const parentStarmapId: string | null = optStr(data, 'parentStarmapId')
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
    projectId: projectId ?? undefined,
    parentStarmapId: parentStarmapId ?? undefined,
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
  const value: StarMapNode = {
    id: id,
    title: title,
    kind: kind as NodeType,
    payload: optObj(data, 'payload') as Record<string, Object> | undefined,
    tags: optArray(data, 'tags') as string[] | undefined,
    description: optStr(data, 'description'),
    color: optStr(data, 'color'),
    icon: optStr(data, 'icon'),
    metadata: optObj(data, 'metadata') as Record<string, string> | undefined,
    parentId: optStr(data, 'parentId'),
    childIds: optArray(data, 'childIds') as string[] | undefined
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
  const value: StarMapEdge = {
    id: id,
    from: optStr(data, 'from'),
    to: optStr(data, 'to'),
    kind: kind as EdgeKind,
    label: optStr(data, 'label'),
    payload: optObj(data, 'payload') as Record<string, Object> | undefined,
    createdAt: createdAt,
    updatedAt: updatedAt
  }
  return ok<StarMapEdge>(value)
}

function decodeStarMapEmbed(data: Object): DecodeResult<StarMapEmbed> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<StarMapEmbed>('StarMapEmbed: expected object')
  }
  const id: string | null = reqStr(data, 'id')
  if (id === null) { return err<StarMapEmbed>('id missing') }
  const nodeId: string | null = reqStr(data, 'nodeId')
  if (nodeId === null) { return err<StarMapEmbed>('nodeId missing') }
  const chapterId: string | null = reqStr(data, 'chapterId')
  if (chapterId === null) { return err<StarMapEmbed>('chapterId missing') }
  const offset: number | null = reqNum(data, 'offset')
  if (offset === null) { return err<StarMapEmbed>('offset missing') }
  const length: number | null = reqNum(data, 'length')
  if (length === null) { return err<StarMapEmbed>('length missing') }
  const text: string | null = reqStr(data, 'text')
  if (text === null) { return err<StarMapEmbed>('text missing') }
  const value: StarMapEmbed = {
    id: id, nodeId: nodeId, chapterId: chapterId, offset: offset, length: length, text: text,
    context: optStr(data, 'context')
  }
  return ok<StarMapEmbed>(value)
}

function decodeStarMapLink(data: Object): DecodeResult<StarMapLink> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<StarMapLink>('StarMapLink: expected object')
  }
  const id: string | null = reqStr(data, 'id')
  if (id === null) { return err<StarMapLink>('id missing') }
  const sourceNodeId: string | null = reqStr(data, 'sourceNodeId')
  if (sourceNodeId === null) { return err<StarMapLink>('sourceNodeId missing') }
  const targetStarmapId: string | null = reqStr(data, 'targetStarmapId')
  if (targetStarmapId === null) { return err<StarMapLink>('targetStarmapId missing') }
  const value: StarMapLink = {
    id: id, sourceNodeId: sourceNodeId, targetStarmapId: targetStarmapId,
    label: optStr(data, 'label'), description: optStr(data, 'description')
  }
  return ok<StarMapLink>(value)
}

function decodeStarMapLayoutNode(data: Object): DecodeResult<StarMapLayoutNode> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<StarMapLayoutNode>('StarMapLayoutNode: expected object')
  }
  const nodeId: string | null = reqStr(data, 'nodeId')
  if (nodeId === null) { return err<StarMapLayoutNode>('nodeId missing') }
  const value: StarMapLayoutNode = {
    nodeId: nodeId,
    x: reqNum(data, 'x') ?? 0,
    y: reqNum(data, 'y') ?? 0,
    width: reqNum(data, 'width') ?? 0,
    height: reqNum(data, 'height') ?? 0,
    radius: reqNum(data, 'radius') ?? 0,
    collapsed: reqBool(data, 'collapsed') ?? false,
    zIndex: reqNum(data, 'zIndex') ?? 0,
    scale: reqNum(data, 'scale') ?? 1,
    depth: reqNum(data, 'depth') ?? 0,
    focusWeight: reqNum(data, 'focusWeight') ?? 0,
    orbitGroup: optStr(data, 'orbitGroup')
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
  if (!nodesR.ok || nodesR.value === undefined) { return err<StarMapLayout>(nodesR.error) }
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
  if (!nodesR.ok || nodesR.value === undefined) { return err<StarMapGraph>(nodesR.error) }
  const edgesArr: Array<Object> | null = reqArray(data, 'edges')
  if (edgesArr === null) { return err<StarMapGraph>('edges missing or not array') }
  const edgesR: DecodeResult<StarMapEdge[]> = decodeArray<StarMapEdge>(edgesArr as Object, decodeStarMapEdge, 'edges')
  if (!edgesR.ok || edgesR.value === undefined) { return err<StarMapGraph>(edgesR.error) }
  const embedsArr: Array<Object> | null = reqArray(data, 'embeds')
  if (embedsArr === null) { return err<StarMapGraph>('embeds missing or not array') }
  const embedsR: DecodeResult<StarMapEmbed[]> = decodeArray<StarMapEmbed>(embedsArr as Object, decodeStarMapEmbed, 'embeds')
  if (!embedsR.ok || embedsR.value === undefined) { return err<StarMapGraph>(embedsR.error) }
  const linksArr: Array<Object> | null = reqArray(data, 'links')
  if (linksArr === null) { return err<StarMapGraph>('links missing or not array') }
  const linksR: DecodeResult<StarMapLink[]> = decodeArray<StarMapLink>(linksArr as Object, decodeStarMapLink, 'links')
  if (!linksR.ok || linksR.value === undefined) { return err<StarMapGraph>(linksR.error) }
  const createdAt: number | null = reqNum(data, 'createdAt')
  if (createdAt === null) { return err<StarMapGraph>('createdAt missing') }
  const updatedAt: number | null = reqNum(data, 'updatedAt')
  if (updatedAt === null) { return err<StarMapGraph>('updatedAt missing') }
  const layoutObj: Object | undefined = optObj(data, 'layout')
  let layout: StarMapLayout | undefined = undefined
  if (layoutObj !== undefined) {
    const layoutR: DecodeResult<StarMapLayout> = decodeStarMapLayout(layoutObj)
    if (!layoutR.ok || layoutR.value === undefined) { return err<StarMapGraph>(`layout: ${layoutR.error}`) }
    layout = layoutR.value
  }
  const value: StarMapGraph = {
    schemaVersion: schemaVersion,
    id: id,
    starmapId: starmapId,
    title: title,
    nodes: nodesR.value,
    edges: edgesR.value,
    embeds: embedsR.value,
    links: linksR.value,
    layout: layout,
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
  const value: StarMapEdgeRender = {
    edgeId: edgeId,
    startX: reqNum(data, 'startX') ?? 0,
    startY: reqNum(data, 'startY') ?? 0,
    endX: reqNum(data, 'endX') ?? 0,
    endY: reqNum(data, 'endY') ?? 0,
    arrowTipX: reqNum(data, 'arrowTipX') ?? 0,
    arrowTipY: reqNum(data, 'arrowTipY') ?? 0,
    arrowLeftX: reqNum(data, 'arrowLeftX') ?? 0,
    arrowLeftY: reqNum(data, 'arrowLeftY') ?? 0,
    arrowRightX: reqNum(data, 'arrowRightX') ?? 0,
    arrowRightY: reqNum(data, 'arrowRightY') ?? 0,
    labelX: optNum(data, 'labelX'),
    labelY: optNum(data, 'labelY')
  }
  return ok<StarMapEdgeRender>(value)
}

export function decodeStarMapMotionPolicy(data: Object): DecodeResult<StarMapMotionPolicy> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<StarMapMotionPolicy>('StarMapMotionPolicy: expected object')
  }
  const value: StarMapMotionPolicy = {
    enabled: reqBool(data, 'enabled') ?? false,
    idleWobbleEnabled: reqBool(data, 'idleWobbleEnabled') ?? false,
    idleAmplitudeVp: reqNum(data, 'idleAmplitudeVp') ?? 0,
    idlePeriodMs: reqNum(data, 'idlePeriodMs') ?? 0,
    dragLiftScale: reqNum(data, 'dragLiftScale') ?? 1,
    dragShadowBoost: reqNum(data, 'dragShadowBoost') ?? 1,
    settleDurationMs: reqNum(data, 'settleDurationMs') ?? 0,
    reduceMotion: reqBool(data, 'reduceMotion') ?? false
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
  return ok<SyncConflict>({
    localPath: localPath, remotePath: remotePath, localHash: localHash, remoteHash: remoteHash,
    baseHash: baseHash, createdAt: createdAt, description: description
  })
}

export function decodeSyncConfig(data: Object): DecodeResult<SyncConfig> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<SyncConfig>('SyncConfig: expected object')
  }
  const value: SyncConfig = {
    enabled: reqBool(data, 'enabled') ?? false,
    backendType: reqStr(data, 'backendType') ?? '',
    remoteUrl: reqStr(data, 'remoteUrl') ?? '',
    transport: reqStr(data, 'transport') ?? '',
    branch: reqStr(data, 'branch') ?? '',
    autoSync: reqBool(data, 'autoSync') ?? false,
    syncIntervalSeconds: reqNum(data, 'syncIntervalSeconds') ?? 0,
    username: reqStr(data, 'username') ?? '',
    androidHasInternetPermission: reqBool(data, 'androidHasInternetPermission') ?? false,
    androidHasAccessNetworkStatePermission: reqBool(data, 'androidHasAccessNetworkStatePermission') ?? false
  }
  return ok<SyncConfig>(value)
}

export function decodeSyncState(data: Object): DecodeResult<SyncState> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<SyncState>('SyncState: expected object')
  }
  const status: string | null = reqStr(data, 'status')
  if (status === null) { return err<SyncState>('status missing') }
  const conflictsArr: Array<Object> | undefined = optArray(data, 'conflicts')
  let conflicts: SyncConflict[] | undefined = undefined
  if (conflictsArr !== undefined) {
    const cR: DecodeResult<SyncConflict[]> = decodeArray<SyncConflict>(conflictsArr as Object, decodeSyncConflict, 'conflicts')
    if (!cR.ok || cR.value === undefined) { return err<SyncState>(cR.error) }
    conflicts = cR.value
  }
  const value: SyncState = {
    status: status,
    remoteUrl: optStr(data, 'remoteUrl'),
    backendType: optStr(data, 'backendType'),
    transport: optStr(data, 'transport'),
    lastSyncedCommit: optStr(data, 'lastSyncedCommit'),
    lastSyncTime: optNum(data, 'lastSyncTime'),
    lastError: optStr(data, 'lastError'),
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
  if (!conflictsR.ok || conflictsR.value === undefined) { return err<SyncResult>(conflictsR.error) }
  const firstSyncMode: string | null = reqStr(data, 'firstSyncMode')
  if (firstSyncMode === null) { return err<SyncResult>('firstSyncMode missing') }
  const value: SyncResult = {
    status: status,
    uploadedFiles: (optArray(data, 'uploadedFiles') ?? []) as string[],
    downloadedFiles: (optArray(data, 'downloadedFiles') ?? []) as string[],
    localDeletes: (optArray(data, 'localDeletes') ?? []) as string[],
    remoteDeletes: (optArray(data, 'remoteDeletes') ?? []) as string[],
    overwrittenFiles: (optArray(data, 'overwrittenFiles') ?? []) as string[],
    ignoredFiles: (optArray(data, 'ignoredFiles') ?? []) as string[],
    conflicts: conflictsR.value,
    commitHash: optStr(data, 'commitHash'),
    error: optStr(data, 'error'),
    errorCategory: optStr(data, 'errorCategory'),
    firstSyncMode: firstSyncMode,
    userMessage: optStr(data, 'userMessage')
  }
  return ok<SyncResult>(value)
}

function decodeSyncPlan(data: Object): DecodeResult<SyncPlan> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<SyncPlan>('SyncPlan: expected object')
  }
  const value: SyncPlan = {
    filesToUpload: (optArray(data, 'filesToUpload') ?? []) as string[],
    filesToDownload: (optArray(data, 'filesToDownload') ?? []) as string[],
    filesToDeleteLocal: (optArray(data, 'filesToDeleteLocal') ?? []) as string[],
    filesToDeleteRemote: (optArray(data, 'filesToDeleteRemote') ?? []) as string[],
    ignoredFiles: (optArray(data, 'ignoredFiles') ?? []) as string[],
    conflicts: (optArray(data, 'conflicts') ?? []) as string[]
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
  const value: FullSyncTargetResult = {
    targetKind: targetKind,
    projectId: optStr(data, 'projectId'),
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
  const value: FullSyncTargetPlan = {
    targetKind: targetKind,
    projectId: optStr(data, 'projectId'),
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
  if (!targetsR.ok || targetsR.value === undefined) { return err<FullSyncResult>(targetsR.error) }
  const value: FullSyncResult = {
    overallStatus: overallStatus,
    targets: targetsR.value,
    totalUploaded: reqNum(data, 'totalUploaded') ?? 0,
    totalDownloaded: reqNum(data, 'totalDownloaded') ?? 0,
    totalLocalDeletes: reqNum(data, 'totalLocalDeletes') ?? 0,
    totalRemoteDeletes: reqNum(data, 'totalRemoteDeletes') ?? 0,
    totalOverwritten: reqNum(data, 'totalOverwritten') ?? 0,
    totalIgnored: reqNum(data, 'totalIgnored') ?? 0,
    totalConflicts: reqNum(data, 'totalConflicts') ?? 0,
    error: optStr(data, 'error'),
    errorCategory: optStr(data, 'errorCategory'),
    messageKey: optStr(data, 'messageKey')
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
  if (!targetsR.ok || targetsR.value === undefined) { return err<FullSyncDryRunResult>(targetsR.error) }
  const value: FullSyncDryRunResult = {
    targets: targetsR.value,
    totalToUpload: reqNum(data, 'totalToUpload') ?? 0,
    totalToDownload: reqNum(data, 'totalToDownload') ?? 0,
    totalToDeleteLocal: reqNum(data, 'totalToDeleteLocal') ?? 0,
    totalToDeleteRemote: reqNum(data, 'totalToDeleteRemote') ?? 0,
    totalIgnored: reqNum(data, 'totalIgnored') ?? 0,
    totalConflicts: reqNum(data, 'totalConflicts') ?? 0
  }
  return ok<FullSyncDryRunResult>(value)
}

function decodeSyncDiagnosticsResult(data: Object): DecodeResult<SyncDiagnosticsResult> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<SyncDiagnosticsResult>('SyncDiagnosticsResult: expected object')
  }
  const value: SyncDiagnosticsResult = {
    success: reqBool(data, 'success') ?? false,
    backendType: reqStr(data, 'backendType') ?? '',
    androidHasInternetPermission: reqBool(data, 'androidHasInternetPermission') ?? false,
    androidHasAccessNetworkStatePermission: reqBool(data, 'androidHasAccessNetworkStatePermission') ?? false,
    androidNetworkState: reqStr(data, 'androidNetworkState') ?? '',
    tcpProbeOk: reqBool(data, 'tcpProbeOk') ?? false,
    tcpProbeStatus: reqStr(data, 'tcpProbeStatus') ?? '',
    httpConnectProbeOk: reqBool(data, 'httpConnectProbeOk') ?? false,
    httpConnectProbeStatus: reqStr(data, 'httpConnectProbeStatus') ?? '',
    libgit2ProbeOk: reqBool(data, 'libgit2ProbeOk') ?? false,
    libgit2ProbeStatus: reqStr(data, 'libgit2ProbeStatus') ?? '',
    networkOk: reqBool(data, 'networkOk') ?? false,
    authOk: reqBool(data, 'authOk') ?? false,
    repoOk: reqBool(data, 'repoOk') ?? false,
    branchOk: reqBool(data, 'branchOk') ?? false,
    networkStatus: reqStr(data, 'networkStatus') ?? '',
    authStatus: reqStr(data, 'authStatus') ?? '',
    repoStatus: reqStr(data, 'repoStatus') ?? '',
    branchStatus: reqStr(data, 'branchStatus') ?? '',
    remoteUrlSanitized: reqStr(data, 'remoteUrlSanitized') ?? '',
    transport: reqStr(data, 'transport') ?? '',
    errorCategory: reqStr(data, 'errorCategory') ?? '',
    userMessage: optStr(data, 'userMessage'),
    rawError: optStr(data, 'rawError'),
    chosenNetworkMode: optStr(data, 'chosenNetworkMode'),
    networkProbeSummary: optStr(data, 'networkProbeSummary')
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
  const projectsR: DecodeResult<Project[]> = decodeArray<Project>(projectsArr as Object, decodeProject, 'projects')
  if (!projectsR.ok || projectsR.value === undefined) { return err<AppSummary>(projectsR.error) }
  const recentEditObj: Object | undefined = optObj(data, 'recentEdit')
  let recentEdit: RecentEdit | null = null
  if (recentEditObj !== undefined) {
    const reR: DecodeResult<RecentEdit> = decodeRecentEdit(recentEditObj)
    if (!reR.ok || reR.value === undefined) { return err<AppSummary>(`recentEdit: ${reR.error}`) }
    recentEdit = reR.value
  }
  return ok<AppSummary>({ projects: projectsR.value, recentEdit: recentEdit })
}

// ─── Layout / Screen Policy Decoders ───

export function decodeLayoutContract(data: Object): DecodeResult<LayoutContract> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<LayoutContract>('LayoutContract: expected object')
  }
  const shellMode: string | null = reqStr(data, 'shellMode')
  if (shellMode === null) { return err<LayoutContract>('shellMode missing') }
  const workspacePaneMode: string | null = reqStr(data, 'workspacePaneMode')
  if (workspacePaneMode === null) { return err<LayoutContract>('workspacePaneMode missing') }
  const showPrimaryNavigation: boolean | null = reqBool(data, 'showPrimaryNavigation')
  if (showPrimaryNavigation === null) { return err<LayoutContract>('showPrimaryNavigation missing') }
  const value: LayoutContract = {
    shellMode: shellMode as ShellMode,
    workspacePaneMode: workspacePaneMode as WorkspacePaneMode,
    showPrimaryNavigation: showPrimaryNavigation
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
  const slotsArr: Array<Object> | null = reqArray(data, 'actionSlots')
  if (slotsArr === null) { return err<ScreenPolicy>('actionSlots missing or not array') }
  const slotsR: DecodeResult<ActionSlot[]> = decodeArray<ActionSlot>(slotsArr as Object, decodeActionSlot, 'actionSlots')
  if (!slotsR.ok || slotsR.value === undefined) { return err<ScreenPolicy>(slotsR.error) }
  const value: ScreenPolicy = {
    screenRole: screenRole as ScreenRole,
    actionSlots: slotsR.value
  }
  return ok<ScreenPolicy>(value)
}
