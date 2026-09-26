#!/usr/bin/env python3
"""HarmonyOS shareFiles（应用共享目录 / 捐献沙箱目录）配置检查。

Issue #773 评论 5846511131 第2项要求把 `scopes[].path` 改回 `/base/files`。
这条要求与 API26 的事实相反，必须用可执行检查固定下来，而不是靠文档口径：

1. vendor-schema     `scopes[].path` 必须匹配 SDK 自带
                     `<sdk>/default/openharmony/toolchains/modulecheck/shareFiles.json`
                     的 `definitions.scopeItem.properties.path.pattern`。API26（SDK 26.0.0）
                     该正则为 `^/(?:el1|el2|el3|el4|el5)/(?:base|distributedfiles|cloud)...$`，
                     `/base/files` 不在其中；DevEco 的 PreBuild schema validate 会直接 BUILD FAILED。
2. path-shape        path 以 / 开头、不以 / 结尾、层级 2~10、第一级 el1~el5、第二级
                     base/distributedfiles/cloud（官方《应用共享目录配置》路径限制说明）。
3. path-set          路径不可重复、最多 20 条、不允许同时配置父目录和子目录（同上）。
4. scope-donation    sharingOSPath 必须等于 scopes 中已配置的 path；sharingOSSubpath 长度
                     不超过 32、空串或以 / 开头的单段；sharingOSPermission 必须是该 scope
                     permission 的子集（官方文档：捐献目录是共享目录的子目录）。

不匹配的 path 在真机上不会"部分生效"：官方文档明确"当应用的任何一条路径配置不符合
路径限制时，会自动清除该应用的全部已配置路径"，系统日志关键字
"TransAndSetToMapInner failed for bundle"。所以这里按硬失败处理。
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from dataclasses import dataclass
from pathlib import Path

PROFILE_REL = "apps/harmony/entry/src/main/resources/base/profile/share_files.json"
MODULE_REL = "apps/harmony/entry/src/main/module.json5"

# 官方《应用共享目录配置》：第一级 el1~el5，第二级 base/distributedfiles/cloud，深度 2~10。
PATH_PATTERN = re.compile(
    r"^/(?:el1|el2|el3|el4|el5)/(?:base|distributedfiles|cloud)(?:/[a-zA-Z0-9_-]+){0,8}$"
)
SCOPE_PERMISSIONS = ("r", "r+w")
MAX_SCOPES = 20

SDK_SCHEMA_REL = "default/openharmony/toolchains/modulecheck/shareFiles.json"
SDK_HOME_ENV_VARS = ("DEVECO_SDK_HOME", "HARMONY_SDK_HOME")
SDK_HOME_FALLBACKS = ("/opt/devecostudio/sdk",)


@dataclass(frozen=True)
class Finding:
    rule: str
    where: str
    message: str


def find_sdk_schema() -> Path | None:
    """定位 SDK 自带的 shareFiles schema，找不到时返回 None（退回内置规则）。"""
    candidates: list[Path] = []
    for name in SDK_HOME_ENV_VARS:
        home = os.environ.get(name)
        if home:
            candidates.append(Path(home) / SDK_SCHEMA_REL)
    cli_home = os.environ.get("HARMONY_CLI_HOME")
    if cli_home:
        candidates.append(Path(cli_home) / "sdk" / SDK_SCHEMA_REL)
    for extra in SDK_HOME_FALLBACKS:
        candidates.append(Path(extra) / SDK_SCHEMA_REL)
    for candidate in candidates:
        if candidate.is_file():
            return candidate
    return None


def load_vendor_patterns(schema_path: Path) -> dict[str, re.Pattern[str]]:
    """从 SDK schema 取 path / sharingOSPath 的正则，用于和工程配置对齐。"""
    data = json.loads(schema_path.read_text(encoding="utf-8"))
    definitions = data.get("definitions", {})
    patterns: dict[str, re.Pattern[str]] = {}
    for key, definition in (
        ("path", definitions.get("scopeItem", {}).get("properties", {}).get("path", {})),
        ("sharingOSPath", definitions.get("sharingOSPath", {})),
    ):
        pattern = definition.get("pattern")
        if isinstance(pattern, str) and pattern:
            patterns[key] = re.compile(pattern)
    return patterns


def _check_scope_paths(scopes: object) -> list[Finding]:
    findings: list[Finding] = []
    if not isinstance(scopes, list) or not scopes:
        return [Finding("path-shape", "share_files.scopes", "scopes 必须是非空数组")]
    if len(scopes) > MAX_SCOPES:
        findings.append(
            Finding("path-set", "share_files.scopes", f"最多配置 {MAX_SCOPES} 条路径，当前 {len(scopes)} 条")
        )
    paths: list[str] = []
    for index, scope in enumerate(scopes):
        where = f"share_files.scopes[{index}]"
        if not isinstance(scope, dict):
            findings.append(Finding("path-shape", where, "scope 必须是对象"))
            continue
        path = scope.get("path")
        permission = scope.get("permission")
        if not isinstance(path, str) or not PATH_PATTERN.match(path):
            findings.append(
                Finding(
                    "path-shape",
                    where,
                    f"path {path!r} 不符合共享路径限制：第一级必须是 el1~el5，第二级必须是 "
                    "base/distributedfiles/cloud，深度 2~10（如 /el2/base/files）",
                )
            )
        else:
            paths.append(path)
        if permission not in SCOPE_PERMISSIONS:
            findings.append(
                Finding("path-shape", where, f"permission {permission!r} 必须是 {list(SCOPE_PERMISSIONS)} 之一")
            )
    for path in sorted(set(paths)):
        if paths.count(path) > 1:
            findings.append(Finding("path-set", "share_files.scopes", f"路径重复配置：{path}"))
    for parent in paths:
        for child in paths:
            if child != parent and child.startswith(parent + "/"):
                findings.append(
                    Finding("path-set", "share_files.scopes", f"父目录 {parent} 与子目录 {child} 不能同时配置")
                )
    return findings


def _check_donation(share_files: dict[str, object], scopes: object) -> list[Finding]:
    findings: list[Finding] = []
    sharing_os_path = share_files.get("sharingOSPath")
    if sharing_os_path is None:
        return findings
    scope_paths = [
        scope.get("path")
        for scope in scopes
        if isinstance(scope, dict) and isinstance(scope.get("path"), str)
    ] if isinstance(scopes, list) else []
    if sharing_os_path not in scope_paths:
        findings.append(
            Finding(
                "scope-donation",
                "share_files.sharingOSPath",
                f"sharingOSPath {sharing_os_path!r} 必须是 scopes 中已配置的 path 之一：{scope_paths}",
            )
        )
    subpath = share_files.get("sharingOSSubpath")
    if not isinstance(subpath, str):
        findings.append(
            Finding("scope-donation", "share_files.sharingOSSubpath", "配置 sharingOSPath 时必须同时配置 sharingOSSubpath")
        )
    else:
        if len(subpath) > 32:
            findings.append(
                Finding("scope-donation", "share_files.sharingOSSubpath", f"长度不得超过 32，当前 {len(subpath)}")
            )
        if subpath and not re.fullmatch(r"/[a-zA-Z0-9_-]+", subpath):
            findings.append(
                Finding("scope-donation", "share_files.sharingOSSubpath", f"{subpath!r} 必须为空串或以 / 开头的单段路径")
            )
    permission = share_files.get("sharingOSPermission")
    if permission not in SCOPE_PERMISSIONS:
        findings.append(
            Finding(
                "scope-donation",
                "share_files.sharingOSPermission",
                f"sharingOSPermission {permission!r} 必须是 {list(SCOPE_PERMISSIONS)} 之一",
            )
        )
    else:
        matched = next(
            (
                scope
                for scope in (scopes if isinstance(scopes, list) else [])
                if isinstance(scope, dict) and scope.get("path") == sharing_os_path
            ),
            None,
        )
        if isinstance(matched, dict) and matched.get("permission") == "r" and permission == "r+w":
            findings.append(
                Finding(
                    "scope-donation",
                    "share_files.sharingOSPermission",
                    "捐献权限必须是共享路径权限的子集：scopes 是 r 时不能捐献 r+w",
                )
            )
    return findings


def check_profile(profile: object, vendor_patterns: dict[str, re.Pattern[str]] | None = None) -> list[Finding]:
    findings: list[Finding] = []
    if not isinstance(profile, dict) or set(profile) != {"share_files"}:
        return [Finding("schema-shape", "share_files.json", '顶层必须只有 "share_files" 一个键')]
    share_files = profile.get("share_files")
    if not isinstance(share_files, dict):
        return [Finding("schema-shape", "share_files", "share_files 必须是对象")]
    allowed = {"scopes", "sharingOSPath", "sharingOSSubpath", "sharingOSPermission"}
    for key in sorted(set(share_files) - allowed):
        findings.append(
            Finding("schema-shape", f"share_files.{key}", f"未知字段，官方只支持 {sorted(allowed)}")
        )
    scopes = share_files.get("scopes")
    findings.extend(_check_scope_paths(scopes))
    findings.extend(_check_donation(share_files, scopes))

    # vendor schema 对齐：用 SDK 自带正则复核本工程真正写入的 path / sharingOSPath。
    if vendor_patterns:
        path_pattern = vendor_patterns.get("path")
        if path_pattern is not None and isinstance(scopes, list):
            for index, scope in enumerate(scopes):
                if not isinstance(scope, dict):
                    continue
                path = scope.get("path")
                if isinstance(path, str) and not path_pattern.match(path):
                    findings.append(
                        Finding(
                            "vendor-schema",
                            f"share_files.scopes[{index}].path",
                            f"{path!r} 不匹配 SDK schema {path_pattern.pattern}（PreBuild schema validate 会 BUILD FAILED）",
                        )
                    )
        sharing_pattern = vendor_patterns.get("sharingOSPath")
        sharing_value = share_files.get("sharingOSPath")
        if sharing_pattern is not None and isinstance(sharing_value, str) and not sharing_pattern.match(sharing_value):
            findings.append(
                Finding(
                    "vendor-schema",
                    "share_files.sharingOSPath",
                    f"{sharing_value!r} 不匹配 SDK schema {sharing_pattern.pattern}",
                )
            )
    return findings


def check_repository(root: Path) -> list[Finding]:
    profile_path = root / PROFILE_REL
    findings: list[Finding] = []
    if not profile_path.is_file():
        return [Finding("missing-profile", str(profile_path), "share_files.json 不存在")]
    module_path = root / MODULE_REL
    if module_path.is_file():
        module_text = module_path.read_text(encoding="utf-8")
        if '"shareFiles"' not in module_text:
            findings.append(
                Finding("module-reference", str(module_path), 'module.json5 缺少 "shareFiles": "$profile:share_files"')
            )
    try:
        profile = json.loads(profile_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        return [Finding("schema-shape", str(profile_path), f"JSON 解析失败：{exc}")]
    schema_path = find_sdk_schema()
    vendor_patterns = load_vendor_patterns(schema_path) if schema_path is not None else None
    findings.extend(check_profile(profile, vendor_patterns))
    return findings


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="HarmonyOS shareFiles 配置检查（Issue #773）")
    parser.add_argument("root", nargs="?", default=".", help="仓库根目录")
    args = parser.parse_args()
    root = Path(args.root).resolve()

    findings = check_repository(root)
    schema_path = find_sdk_schema()
    print("=" * 60)
    print("HarmonyOS shareFiles 配置检查（SDK schema + 官方路径限制）")
    print("=" * 60)
    print(f"SDK schema：{schema_path if schema_path else '未找到，仅用内置官方规则'}")
    if not findings:
        print("ALL PASS")
        return 0
    for finding in findings:
        print(f"[FAIL] {finding.rule}: {finding.where}")
        print(f"       {finding.message}")
    print()
    print(f"共 {len(findings)} 处 shareFiles 配置问题")
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
