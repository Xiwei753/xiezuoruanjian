#!/usr/bin/env python3
"""Harmony DTO 契约检查（Issue #753 评论 5809590165）。

Rust Core 的 serde DTO 是唯一数据结构，Harmony FFI 只负责搬运。本工具把
"ArkTS 端字段名/层级/可空性必须跟 Core DTO 对齐" 变成可执行检查，避免再次
出现第二套手写字段表：

1. dto-field-drift        ArkTS interface 字段集合必须与对应 Core DTO 的
                          serde 线格式字段集合完全一致（多一个、少一个都报）。
2. decoder-alias          CoreWireDecoders 的某个 decoder 读了 Core DTO（含其
                          嵌套 DTO 闭包）里不存在的键，即页面私有别名。
3. decoder-missing-field  Core DTO 的字段没有任何 decoder 读取，等于 Core 发出
                          的数据被静默丢弃。
4. decoder-nullability    decoder 取值用的 req*/opt*/null* 家族必须与该字段在
                          ArkTS interface 上的可空性一致：
                          - interface `T | null` → 必须 null*（Core 恒发该键，值是 null）
                          - interface 可选 `?: T` → 必须 opt*
                          - interface 非空 `T`   → 必须 req*

对应关系写在 INTERFACE_MAP / DECODER_MAP，新增 DTO 时必须同时登记，否则
`unmapped-*` 规则会报出来。
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

# ---------------------------------------------------------------------------
# 路径与对应关系
# ---------------------------------------------------------------------------

RUST_DTO_SOURCES: tuple[str, ...] = (
    "core/writer_core/src/api/types",
    "core/writer_core/src/ffi",
)

ARKTS_CONTRACT_DIR = "apps/harmony/entry/src/main/ets/corebridge"
ARKTS_DTO_DIR = "apps/harmony/entry/src/main/ets/corebridge/dto"
ARKTS_DECODER_FILE = "apps/harmony/entry/src/main/ets/corebridge/codec/CoreWireDecoders.ets"

# ArkTS interface -> Core Rust DTO
INTERFACE_MAP: dict[str, tuple[str, str]] = {
    # (arkts 文件名, interface 名) -> Rust DTO 名
    "SettingsDtos.ets:LocalSettings": "LocalSettingsDto",
    "SettingsDtos.ets:SyncableSettings": "SyncableSettingsDto",
    "SettingsDtos.ets:ThemeColorSchemeDto": "ThemeColorSchemeDto",
    "SettingsDtos.ets:ThemePaletteRecordDto": "ThemePaletteRecordDto",
    "SettingsDtos.ets:BuiltinThemeDto": "BuiltinThemeDto",
    "ProjectDtos.ets:Project": "ProjectDto",
    "ProjectDtos.ets:ProjectSummary": "ProjectSummaryDto",
    "ProjectDtos.ets:ProjectStats": "ProjectStatsDto",
    "ProjectDtos.ets:Volume": "VolumeDto",
    "ProjectDtos.ets:ChapterMeta": "ChapterMetaDto",
    "ProjectDtos.ets:ChapterContent": "ChapterContentDto",
    "ProjectDtos.ets:ChapterSaveReceipt": "ChapterSaveReceiptDto",
    "ProjectDtos.ets:ProjectTree": "ProjectWorkspaceSnapshotDto",
    "ProjectDtos.ets:VolumeTree": "VolumeWithChaptersDto",
    "ProjectDtos.ets:DateRange": "DateRangeDto",
    "ProjectDtos.ets:WritingStats": "WritingStatsSummaryDto",
    "ResultEnvelope.ets:RecentEdit": "RecentEditDto",
    "ResultEnvelope.ets:AppSummary": "AppStateSummaryDto",
    "PlatformDtos.ets:LayoutMetrics": "LayoutMetricsDto",
    "PlatformDtos.ets:LayoutContract": "LayoutContractDto",
    "PlatformDtos.ets:WindowOcclusion": "WindowOcclusionDto",
    "PlatformDtos.ets:WindowViewport": "WindowViewportDto",
    "PlatformDtos.ets:ActionSlot": "ActionSlotDto",
    "PlatformDtos.ets:ScreenPolicy": "ScreenPolicyDto",
    "SyncDtos.ets:ProviderConfig": "ProviderConfigDto",
    "SyncDtos.ets:SyncConfig": "SyncConfigDto",
    "SyncDtos.ets:SyncState": "SyncStateDto",
    "SyncDtos.ets:SyncConflict": "SyncConflictDto",
    "SyncDtos.ets:SyncResult": "SyncResultDto",
    "SyncDtos.ets:SyncDiagnosticsResult": "SyncDiagnosticsResultDto",
    "SyncDtos.ets:SyncPlan": "SyncPlanDto",
    "SyncDtos.ets:FullSyncResult": "FullSyncResultDto",
    "SyncDtos.ets:FullSyncTargetResult": "TargetSyncResultDto",
    "SyncDtos.ets:FullSyncTargetPlan": "TargetSyncPlanDto",
    "SyncDtos.ets:FullSyncDryRunResult": "FullSyncDryRunResultDto",
    "SyncDtos.ets:FullSyncDiagnosticsResult": "FullSyncDiagnosticsResultDto",
    "StarMapDtos.ets:StarMapNodeContent": "StarMapNodeContentDto",
    "StarMapDtos.ets:StarMapAnchorTarget": "StarMapAnchorTargetDto",
    "StarMapDtos.ets:StarMapAnchor": "StarMapAnchorDto",
    "StarMapDtos.ets:StarMapPathSegment": "StarMapPathSegmentDto",
    "StarMapDtos.ets:StarMapTargetDetail": "StarMapTargetDetailDto",
    "StarMapDtos.ets:StarMapDeepTarget": "StarMapDeepTargetDto",
    "StarMapDtos.ets:StarMapPortal": "StarMapPortalDto",
    "StarMapDtos.ets:StarMapDisplayPolicy": "StarMapDisplayPolicyDto",
    "StarMapDtos.ets:StarMapProvenance": "StarMapProvenanceDto",
    "StarMapDtos.ets:StarMapNode": "StarMapNodeDto",
    "StarMapDtos.ets:StarMapEdgeEndpoint": "StarMapEdgeEndpointDto",
    "StarMapDtos.ets:StarMapEndpointPathSegment": "StarMapEndpointPathSegmentDto",
    "StarMapDtos.ets:StarMapEndpointPath": "StarMapEndpointPathDto",
    "StarMapDtos.ets:StarMapEdge": "StarMapEdgeDto",
    "StarMapDtos.ets:StarMapEmbedPlacement": "StarMapEmbedPlacementDto",
    "StarMapDtos.ets:StarMapEmbedViewport": "StarMapEmbedViewportDto",
    "StarMapDtos.ets:StarMapEndpoint": "StarMapEndpointDto",
    "StarMapDtos.ets:StarMapEmbed": "StarMapEmbedDto",
    "StarMapDtos.ets:StarMapLink": "StarMapLinkDto",
    "StarMapDtos.ets:StarMapHyperlink": "StarMapHyperlinkDto",
    "StarMapDtos.ets:StarMapGraph": "StarMapGraphDto",
    "StarMapDtos.ets:StarMapMeta": "StarMapMetaDto",
    "StarMapDtos.ets:StarMapViewport": "StarMapViewportDto",
    "StarMapDtos.ets:StarMapLayout": "StarMapLayoutDto",
    "StarMapDtos.ets:StarMapLayoutNode": "StarMapLayoutNodeDto",
    "StarMapDtos.ets:StarMapEdgeRender": "StarMapEdgeRenderDto",
    "StarMapDtos.ets:StarMapMotionPolicy": "StarMapMotionPolicyDto",
    "EditorDtos.ets:OffsetMapEntry": "OffsetMapEntryDto",
    "EditorDtos.ets:OffsetMap": "OffsetMapDto",
    "EditorDtos.ets:DisplayPatch": "DisplayPatchDto",
    "EditorDtos.ets:CompositionSession": "CompositionSessionDto",
    "EditorDtos.ets:EditorCompositionState": "EditorCompositionStateDto",
    "EditorDtos.ets:EditorContentDelta": "EditorContentDeltaDto",
    "EditorDtos.ets:EditorEditResult": "EditorEditResultDto",
    "EditorDtos.ets:EditorSessionSnapshot": "EditorSessionSnapshotDto",
}

# CoreWireDecoders 里的 decoder 函数 -> Core Rust DTO
DECODER_MAP: dict[str, str] = {
    "decodeLocalSettings": "LocalSettingsDto",
    "decodeSyncableSettings": "SyncableSettingsDto",
    "decodeThemeColorScheme": "ThemeColorSchemeDto",
    "decodeThemePaletteRecord": "ThemePaletteRecordDto",
    "decodeBuiltinTheme": "BuiltinThemeDto",
    "decodeProject": "ProjectDto",
    "decodeProjectSummary": "ProjectSummaryDto",
    "decodeProjectStats": "ProjectStatsDto",
    "decodeVolume": "VolumeDto",
    "decodeChapterMeta": "ChapterMetaDto",
    "decodeChapterContent": "ChapterContentDto",
    "decodeChapterSaveReceipt": "ChapterSaveReceiptDto",
    "decodeProjectTree": "ProjectWorkspaceSnapshotDto",
    "decodeWritingStats": "WritingStatsSummaryDto",
    "decodeRecentEdit": "RecentEditDto",
    "decodeAppSummary": "AppStateSummaryDto",
    "decodeStarMapNodeContent": "StarMapNodeContentDto",
    "decodeStarMapAnchorTarget": "StarMapAnchorTargetDto",
    "decodeStarMapAnchor": "StarMapAnchorDto",
    "decodeStarMapPortal": "StarMapPortalDto",
    "decodeStarMapDisplayPolicy": "StarMapDisplayPolicyDto",
    "decodeStarMapProvenance": "StarMapProvenanceDto",
    "decodeStarMapNode": "StarMapNodeDto",
    "decodeStarMapEdge": "StarMapEdgeDto",
    "decodeStarMapEmbed": "StarMapEmbedDto",
    "decodeStarMapEndpoint": "StarMapEndpointDto",
    "decodeStarMapPathSegment": "StarMapPathSegmentDto",
    "decodeStarMapTargetDetail": "StarMapTargetDetailDto",
    "decodeStarMapDeepTarget": "StarMapDeepTargetDto",
    "decodeStarMapEdgeEndpoint": "StarMapEdgeEndpointDto",
    "decodeStarMapEndpointPathSegment": "StarMapEndpointPathSegmentDto",
    "decodeStarMapEndpointPath": "StarMapEndpointPathDto",
    "decodeStarMapLink": "StarMapLinkDto",
    "decodeStarMapHyperlink": "StarMapHyperlinkDto",
    "decodeStarMapLayoutNode": "StarMapLayoutNodeDto",
    "decodeStarMapLayout": "StarMapLayoutDto",
    "decodeStarMapGraph": "StarMapGraphDto",
    "decodeStarMapEdgeRender": "StarMapEdgeRenderDto",
    "decodeStarMapMotionPolicy": "StarMapMotionPolicyDto",
    "decodeStarMapMeta": "StarMapMetaDto",
    "decodeProviderConfig": "ProviderConfigDto",
    "decodeSyncConfig": "SyncConfigDto",
    "decodeSyncState": "SyncStateDto",
    "decodeSyncConflict": "SyncConflictDto",
    "decodeSyncResult": "SyncResultDto",
    "decodeSyncPlan": "SyncPlanDto",
    "decodeFullSyncTargetResult": "TargetSyncResultDto",
    "decodeFullSyncTargetPlan": "TargetSyncPlanDto",
    "decodeFullSyncResult": "FullSyncResultDto",
    "decodeFullSyncDryRunResult": "FullSyncDryRunResultDto",
    "decodeSyncDiagnosticsResult": "SyncDiagnosticsResultDto",
    "decodeFullSyncDiagnosticsResult": "FullSyncDiagnosticsResultDto",
    "decodeLayoutMetrics": "LayoutMetricsDto",
    "decodeLayoutContract": "LayoutContractDto",
    "decodeActionSlot": "ActionSlotDto",
    "decodeScreenPolicy": "ScreenPolicyDto",
    "decodeEditorContentDelta": "EditorContentDeltaDto",
    "decodeDisplayPatch": "DisplayPatchDto",
    "decodeCompositionSession": "CompositionSessionDto",
    "decodeEditorCompositionState": "EditorCompositionStateDto",
    "decodeOffsetMapEntry": "OffsetMapEntryDto",
    "decodeOffsetMap": "OffsetMapDto",
    "decodeEditorSessionSnapshot": "EditorSessionSnapshotDto",
    "decodeEditorEditResult": "EditorEditResultDto",
}


@dataclass(frozen=True)
class Finding:
    rule: str
    where: str
    message: str


@dataclass
class RustField:
    wire_name: str
    rust_type: str
    nullable: bool
    may_be_omitted: bool


@dataclass
class RustDto:
    name: str
    fields: dict[str, RustField] = field(default_factory=dict)


@dataclass
class ArktsField:
    name: str
    type_text: str
    optional: bool

    @property
    def nullable(self) -> bool:
        return "| null" in self.type_text or self.type_text.strip() == "null"


@dataclass
class Decoder:
    name: str
    keys: dict[str, str] = field(default_factory=dict)


# ---------------------------------------------------------------------------
# Camel case helper（与 serde rename_all = "camelCase" 一致）
# ---------------------------------------------------------------------------


def camel_case(name: str) -> str:
    head, *tail = name.split("_")
    return head + "".join(part[:1].upper() + part[1:] for part in tail)


def snake_case(name: str) -> str:
    out: list[str] = []
    for ch in name:
        if ch.isupper() and out:
            out.append("_")
        out.append(ch.lower())
    return "".join(out)


# ---------------------------------------------------------------------------
# Rust DTO 解析
# ---------------------------------------------------------------------------

_STRUCT_RE = re.compile(r"^(?:pub\s+)?(?:struct|enum)\s+(\w+)\s*\{")
_ATTR_RE = re.compile(r"^\s*#\[serde\((?P<body>.*)\)\]\s*$")
_FIELD_RE = re.compile(r"^\s*(?:pub\s+)?(?P<name>\w+)\s*:\s*(?P<ty>.+?),?\s*$")
_VARIANT_FIELDS_RE = re.compile(r"^\s*(?P<name>\w+)\s*\{")
_RENAME_ALL_RE = re.compile(r'rename_all\s*=\s*"(?P<case>\w+)"')
_RENAME_RE = re.compile(r'rename\s*=\s*"(?P<name>[^"]+)"')
_TAG_RE = re.compile(r'tag\s*=\s*"(?P<tag>[^"]+)"')


def _apply_case(name: str, case: str | None) -> str:
    if case == "camelCase":
        return camel_case(name)
    if case == "snake_case":
        return snake_case(name)
    if case == "PascalCase":
        return name
    return name


def parse_rust_dtos_from_text(path: Path, text: str) -> dict[str, RustDto]:
    """从一份 Rust 源码文本里抽取带 serde 属性的 DTO/枚举线格式字段。"""
    dtos: dict[str, RustDto] = {}
    lines = text.splitlines()
    idx = 0
    while idx < len(lines):
        stripped = lines[idx].strip()
        if stripped.startswith("#["):
            attrs: list[str] = []
            while idx < len(lines) and lines[idx].strip().startswith("#["):
                attrs.append(lines[idx].strip())
                idx += 1
            if idx >= len(lines):
                break
            match = _STRUCT_RE.match(lines[idx])
            if not match:
                idx += 1
                continue
            name = match.group(1)
            is_enum = lines[idx].lstrip().startswith("pub enum")
            attr_text = " ".join(attrs)
            case = _RENAME_ALL_RE.search(attr_text)
            rename_all = case.group("case") if case else None
            tag = _TAG_RE.search(attr_text)
            dto = RustDto(name=name)
            if tag:
                dto.fields[tag.group("tag")] = RustField(
                    wire_name=tag.group("tag"),
                    rust_type="string",
                    nullable=False,
                    may_be_omitted=False,
                )
            idx += 1
            field_attrs: list[str] = []
            variant_attrs: list[str] = []
            while idx < len(lines) and lines[idx].strip() != "}":
                line = lines[idx]
                if line.strip().startswith("#["):
                    if is_enum:
                        variant_attrs.append(line.strip())
                    else:
                        field_attrs.append(line.strip())
                    idx += 1
                    continue
                if is_enum:
                    variant = _VARIANT_FIELDS_RE.match(line)
                    if variant:
                        idx += 1
                        while idx < len(lines) and lines[idx].strip() != "},":
                            field_match = _FIELD_RE.match(lines[idx])
                            if field_match:
                                fname = field_match.group("name")
                                dto.fields[fname] = RustField(
                                    wire_name=fname,
                                    rust_type=field_match.group("ty").rstrip(","),
                                    nullable="Option<" in field_match.group("ty"),
                                    may_be_omitted=not field_match.group("ty")
                                    .strip()
                                    .startswith("Vec<"),
                                )
                            idx += 1
                        variant_attrs = []
                    idx += 1
                    continue
                field_match = _FIELD_RE.match(line)
                if field_match:
                    fname = field_match.group("name")
                    ty = field_match.group("ty").rstrip(",")
                    attr_text_field = " ".join(field_attrs)
                    explicit = _RENAME_RE.search(attr_text_field)
                    wire = explicit.group("name") if explicit else _apply_case(fname, rename_all)
                    dto.fields[wire] = RustField(
                        wire_name=wire,
                        rust_type=ty,
                        nullable=ty.strip().startswith("Option<"),
                        may_be_omitted="skip_serializing_if" in attr_text_field,
                    )
                    field_attrs = []
                idx += 1
            dtos[name] = dto
            continue
        idx += 1
    return dtos


def parse_rust_dtos(root: Path) -> dict[str, RustDto]:
    dtos: dict[str, RustDto] = {}
    for rel in RUST_DTO_SOURCES:
        base = root / rel
        if not base.exists():
            continue
        for path in sorted(base.rglob("*.rs")):
            if path.name.endswith("_tests.rs") or path.name == "mod.rs":
                continue
            text = path.read_text(encoding="utf-8")
            if "#[serde" not in text:
                continue
            for name, dto in parse_rust_dtos_from_text(path, text).items():
                dtos.setdefault(name, dto)
    return dtos


# ---------------------------------------------------------------------------
# ArkTS DTO / decoder 解析
# ---------------------------------------------------------------------------

_INTERFACE_RE = re.compile(r"^export\s+interface\s+(\w+)\s*\{")
_IFACE_FIELD_RE = re.compile(r"^\s{2}(?P<name>\w+)(?P<optional>\?)?:\s*(?P<ty>.+?)\s*$")
_FUNC_RE = re.compile(r"^(?:export\s+)?function\s+(\w+)\s*\(")
_HELPER_RE = re.compile(
    r"\b(req|opt|null)(Str|Num|Bool|Obj|Array)\w*\s*\(\s*\w+\s*,\s*'(?P<key>[^']+)'"
)
# 可空嵌套 DTO：decodeOptional(data, 'key', decodeXxx) —— 键一定出现，值为对象或 null。
_OPTIONAL_HELPER_RE = re.compile(
    r"\bdecodeOptional\s*<\s*[^>]+>\s*\(\s*\w+\s*,\s*'(?P<key>[^']+)'|\bdecodeOptional\s*\(\s*\w+\s*,\s*'(?P<key2>[^']+)'"
)
# 变量键调用：reqStr(data, key) —— 键来自函数内的 string[] 字面量列表。
_DYNAMIC_HELPER_RE = re.compile(r"\b(req|opt|null)(?:Str|Num|Bool|Obj|Array)\w*\s*\(\s*\w+\s*,\s*\w+\s*\)")
# 键列表：const required: string[] = ['a', 'b', ...]
_KEY_LIST_RE = re.compile(r"const\s+\w+\s*:\s*string\[\]\s*=\s*\[(?P<body>[^\]]*)\]", re.S)
_STRING_LITERAL_RE = re.compile(r"'([^']+)'")


def parse_arkts_interfaces_from_text(text: str) -> dict[str, list[ArktsField]]:
    interfaces: dict[str, list[ArktsField]] = {}
    lines = text.splitlines()
    idx = 0
    while idx < len(lines):
        match = _INTERFACE_RE.match(lines[idx])
        if not match:
            idx += 1
            continue
        name = match.group(1)
        fields: list[ArktsField] = []
        idx += 1
        while idx < len(lines) and lines[idx].strip() != "}":
            field_match = _IFACE_FIELD_RE.match(lines[idx])
            if field_match:
                fields.append(
                    ArktsField(
                        name=field_match.group("name"),
                        type_text=field_match.group("ty"),
                        optional=field_match.group("optional") is not None,
                    )
                )
            idx += 1
        interfaces[name] = fields
    return interfaces


def parse_arkts_interfaces(root: Path) -> dict[str, list[ArktsField]]:
    interfaces: dict[str, list[ArktsField]] = {}
    base = root / ARKTS_DTO_DIR
    if not base.exists():
        return interfaces
    for path in sorted(base.glob("*.ets")):
        for name, fields in parse_arkts_interfaces_from_text(
            path.read_text(encoding="utf-8")
        ).items():
            interfaces[f"{path.name}:{name}"] = fields
    return interfaces


def parse_decoders_from_text(text: str) -> dict[str, Decoder]:
    decoders: dict[str, Decoder] = {}
    lines = text.splitlines()
    idx = 0
    while idx < len(lines):
        match = _FUNC_RE.match(lines[idx])
        if not match:
            idx += 1
            continue
        name = match.group(1)
        decoder = Decoder(name=name)
        body: list[str] = []
        depth = 0
        started = False
        while idx < len(lines):
            body.append(lines[idx])
            for helper, kind, key in _HELPER_RE.findall(lines[idx]):
                decoder.keys[key] = helper
            for match_optional in _OPTIONAL_HELPER_RE.finditer(lines[idx]):
                key = match_optional.group("key") or match_optional.group("key2")
                decoder.keys[key] = "null"
            depth += lines[idx].count("{") - lines[idx].count("}")
            if "{" in lines[idx]:
                started = True
            if started and depth <= 0:
                break
            idx += 1
        decoder.keys.update(_list_driven_keys("\n".join(body)))
        decoders[name] = decoder
        idx += 1
    return decoders


def _list_driven_keys(body: str) -> dict[str, str]:
    """`for (const key of required) reqStr(data, key)` 形式的键来自 string[] 字面量。"""
    dynamic = _DYNAMIC_HELPER_RE.search(body)
    if not dynamic:
        return {}
    keys: dict[str, str] = {}
    for key_list in _KEY_LIST_RE.findall(body):
        for key in _STRING_LITERAL_RE.findall(key_list):
            keys[key] = dynamic.group(1)
    return keys


def parse_decoders(root: Path) -> dict[str, Decoder]:
    path = root / ARKTS_DECODER_FILE
    if not path.exists():
        return {}
    return parse_decoders_from_text(path.read_text(encoding="utf-8"))


# ---------------------------------------------------------------------------
# 检查
# ---------------------------------------------------------------------------


def _dto_closure(
    dto_name: str, dtos: dict[str, RustDto], seen: set[str] | None = None
) -> set[str]:
    """DTO 及其嵌套 DTO 的线格式字段名闭包。"""
    if seen is None:
        seen = set()
    if dto_name in seen or dto_name not in dtos:
        return set()
    seen.add(dto_name)
    names: set[str] = set()
    for rust_field in dtos[dto_name].fields.values():
        names.add(rust_field.wire_name)
        for token in re.findall(r"[A-Za-z_]\w*", rust_field.rust_type):
            if token in dtos:
                names |= _dto_closure(token, dtos, seen)
    return names


def check_sources(
    dtos: dict[str, RustDto],
    interfaces: dict[str, list[ArktsField]],
    decoders: dict[str, Decoder],
) -> list[Finding]:
    findings: list[Finding] = []

    # 规则 1：ArkTS interface 字段集合 == Core DTO 线格式字段集合。
    for key, dto_name in sorted(INTERFACE_MAP.items()):
        fields = interfaces.get(key)
        if fields is None:
            findings.append(
                Finding("dto-not-found", key, f"ArkTS interface 不存在（映射到 {dto_name}）")
            )
            continue
        dto = dtos.get(dto_name)
        if dto is None:
            findings.append(
                Finding("dto-not-found", dto_name, "Core Rust DTO 不存在（检查 serde 属性）")
            )
            continue
        arkts_names = {f.name for f in fields}
        core_names = set(dto.fields)
        for name in sorted(arkts_names - core_names):
            findings.append(
                Finding(
                    "dto-field-drift",
                    key,
                    f"ArkTS 多出字段 '{name}'：Core {dto_name} 不发送该键",
                )
            )
        for name in sorted(core_names - arkts_names):
            findings.append(
                Finding(
                    "dto-field-drift",
                    key,
                    f"ArkTS 缺少字段 '{name}'：Core {dto_name} 会发送该键",
                )
            )

    # 规则 2/3/4：decoder 键集合与 Core DTO 对齐。
    for decoder_name, dto_name in sorted(DECODER_MAP.items()):
        decoder = decoders.get(decoder_name)
        if decoder is None:
            findings.append(
                Finding(
                    "decoder-not-found",
                    decoder_name,
                    f"CoreWireDecoders 不存在该 decoder（映射到 {dto_name}）",
                )
            )
            continue
        dto = dtos.get(dto_name)
        if dto is None:
            findings.append(
                Finding("dto-not-found", dto_name, "Core Rust DTO 不存在（检查 serde 属性）")
            )
            continue
        closure = _dto_closure(dto_name, dtos)
        for key in sorted(set(decoder.keys) - closure):
            findings.append(
                Finding(
                    "decoder-alias",
                    f"{decoder_name}",
                    f"读取了 '{key}'，但 Core {dto_name} 闭包里没有该键（页面私有别名）",
                )
            )
        for name in sorted(set(dto.fields) - set(decoder.keys)):
            findings.append(
                Finding(
                    "decoder-missing-field",
                    decoder_name,
                    f"未读取 Core {dto_name} 的字段 '{name}'，该键会被静默丢弃",
                )
            )

        # 规则 4 需要 interface 侧的可空性声明。
        iface_key = _interface_key_for_dto(dto_name)
        if iface_key is None:
            continue
        iface_fields = {
            f.name: f
            for f in interfaces.get(iface_key, [])
        }
        for key in sorted(set(decoder.keys) & set(dto.fields)):
            iface_field = iface_fields.get(key)
            if iface_field is None:
                continue
            helper = decoder.keys[key]
            if iface_field.nullable and iface_field.optional:
                findings.append(
                    Finding(
                        "decoder-nullability",
                        decoder_name,
                        f"'{key}' 同时声明 `?` 与 `| null`，无法判定 req*/opt*/null*",
                    )
                )
                continue
            if iface_field.nullable and helper != "null":
                findings.append(
                    Finding(
                        "decoder-nullability",
                        decoder_name,
                        f"'{key}' 在 interface 上是 `T | null`（Core 恒发该键），必须用 null* 取值",
                    )
                )
            elif iface_field.optional and helper != "opt":
                findings.append(
                    Finding(
                        "decoder-nullability",
                        decoder_name,
                        f"'{key}' 在 interface 上是可选属性（Core 可能省略该键），必须用 opt*",
                    )
                )
            elif not iface_field.nullable and not iface_field.optional and helper != "req":
                findings.append(
                    Finding(
                        "decoder-nullability",
                        decoder_name,
                        f"'{key}' 在 interface 上非空必填（Core 恒发该键），必须用 req*",
                    )
                )

    # 规则 5：登记完整性——每个 decoder 必须映射到 Core DTO。
    for decoder_name in sorted(decoders):
        if decoder_name in DECODER_MAP:
            continue
        if decoder_name.startswith("decode") and decoder_name not in (
            "decodeArray",
            "decodeThemeColorScheme",
        ):
            findings.append(
                Finding(
                    "unmapped-decoder",
                    decoder_name,
                    "CoreWireDecoders 里的 decoder 未在 DECODER_MAP 登记，契约无从校验",
                )
            )
    return findings


_INTERFACE_BY_DTO: dict[str, str] = {
    dto_name: key for key, dto_name in INTERFACE_MAP.items()
}


def _interface_key_for_dto(dto_name: str) -> str | None:
    return _INTERFACE_BY_DTO.get(dto_name)


# ---------------------------------------------------------------------------
# ArkTS 契约层 import 解析（corebridge 内部不再引用不存在的类型/值）
# ---------------------------------------------------------------------------

_NAMED_IMPORT_RE = re.compile(r"^\s*import\s*\{\s*(?P<names>[^}]*)\}\s*from\s*'(?P<target>[^']+)'")
_EXPORT_DECL_RE = re.compile(
    r"^\s*export\s+(?:default\s+)?(?:abstract\s+)?"
    r"(?:interface|type|enum|class|struct|function|const|let|var)\s+(?P<name>\w+)"
)
_EXPORT_LIST_RE = re.compile(r"^\s*export\s*\{\s*(?P<names>[^}]*)\}")


def parse_exported_names(text: str) -> set[str]:
    names: set[str] = set()
    for line in text.splitlines():
        decl = _EXPORT_DECL_RE.match(line)
        if decl:
            names.add(decl.group("name"))
        listed = _EXPORT_LIST_RE.match(line)
        if listed:
            for part in listed.group("names").split(","):
                part = part.strip()
                if part:
                    names.add(part.split(" as ")[-1].strip())
    return names


def check_contract_imports(root: Path) -> list[Finding]:
    """corebridge 内所有命名 import 必须在目标 .ets 中存在导出。"""
    findings: list[Finding] = []
    base = root / ARKTS_CONTRACT_DIR
    if not base.exists():
        return findings
    exports_cache: dict[Path, set[str]] = {}
    for path in sorted(base.rglob("*.ets")):
        for line in path.read_text(encoding="utf-8").splitlines():
            match = _NAMED_IMPORT_RE.match(line)
            if not match:
                continue
            target = match.group("target")
            if not target.startswith("."):
                continue
            resolved = (path.parent / target).resolve()
            candidates = [Path(str(resolved) + ".ets"), resolved / "index.ets"]
            target_file = next((c for c in candidates if c.exists()), None)
            rel = path.relative_to(root)
            if target_file is None:
                findings.append(
                    Finding("contract-import-missing", str(rel), f"import '{target}' 无法解析到 .ets 文件")
                )
                continue
            if target_file not in exports_cache:
                exports_cache[target_file] = parse_exported_names(
                    target_file.read_text(encoding="utf-8")
                )
            available = exports_cache[target_file]
            for name in match.group("names").split(","):
                name = name.strip()
                if not name:
                    continue
                if name not in available:
                    findings.append(
                        Finding(
                            "contract-import-missing",
                            str(rel),
                            f"import 的 '{name}' 在 {target_file.name} 中没有导出（悬空类型/值）",
                        )
                    )
    return findings


def scan(root: Path) -> list[Finding]:
    return check_sources(
        parse_rust_dtos(root),
        parse_arkts_interfaces(root),
        parse_decoders(root),
    ) + check_contract_imports(root)


def main() -> int:
    parser = argparse.ArgumentParser(description="Harmony DTO 契约检查（Issue #753）")
    parser.add_argument("root", nargs="?", default=".", help="仓库根目录")
    args = parser.parse_args()
    root = Path(args.root).resolve()

    findings = scan(root)
    print("=" * 60)
    print("Harmony DTO 契约检查（Core DTO -> ArkTS DTO/decoder）")
    print("=" * 60)
    if not findings:
        print("ALL PASS")
        return 0
    for finding in findings:
        print(f"[FAIL] {finding.rule}: {finding.where}")
        print(f"       {finding.message}")
    print()
    print(f"共 {len(findings)} 处契约漂移")
    return 1


if __name__ == "__main__":
    sys.exit(main())
