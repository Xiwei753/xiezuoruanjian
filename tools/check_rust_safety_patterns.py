#!/usr/bin/env python3
"""扫描 Rust 源码中常见的“绕过编译器”写法。

本工具不替代 rustc/Clippy。它只检查仓库已经反复出现、且普通 lint
不一定能表达的项目级禁用模式。
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable


@dataclass(frozen=True)
class Finding:
    path: Path
    line: int
    rule: str
    message: str


@dataclass(frozen=True)
class PatternRule:
    name: str
    pattern: re.Pattern[str]
    message: str


RULES: tuple[PatternRule, ...] = (
    PatternRule(
        "unsafe-impl-send-sync",
        re.compile(r"\bunsafe\s+impl(?:\s*<[^>]*>)?\s+(?:Send|Sync)\b"),
        "禁止手写 Send/Sync 绕过自动线程安全判断",
    ),
    PatternRule(
        "intentional-resource-leak",
        re.compile(r"\b(?:Box::leak|(?:std::)?mem::forget)\s*\("),
        "禁止通过永久泄漏资源解决生命周期问题",
    ),
    PatternRule(
        "crate-wide-allow",
        re.compile(
            r"#!\s*\[\s*allow\s*\([^)]*\b"
            r"(?:warnings|deprecated|dead_code|unused(?:_[a-z_]+)?)\b"
            r"[^)]*\)\s*\]"
        ),
        "禁止 crate/module 级关闭过期、死代码或全部警告",
    ),
    PatternRule(
        "broad-dead-code-allow",
        re.compile(
            r"#(?!\!)\s*\[\s*allow\s*\([^)]*\bdead_code\b[^)]*\)\s*\]"
        ),
        "dead_code 只能精确标注到确有宏误报的最小成员",
    ),
    PatternRule(
        "app-backend-mut-pointer-cast",
        re.compile(r"\bas\s+\*mut\s+AppBackend\b"),
        "禁止把共享 AppBackend 引用强转成可变裸指针",
    ),
    PatternRule(
        "app-backend-mut-alias",
        re.compile(r"&mut\s+\*\s*app\b"),
        "禁止从共享裸指针伪造 &mut AppBackend",
    ),
    PatternRule(
        "safe-app-ptr-usage",
        re.compile(r"\bSafeAppPtr\b"),
        "SafeAppPtr 已被 AppRef (Rc<RefCell<AppBackend>>) 替代；禁止新增使用",
    ),
    PatternRule(
        "rc-cell-raw-pointer",
        re.compile(r"Rc\s*<\s*Cell\s*<\s*\*const"),
        "禁止用 Rc<Cell<*const>> 存储裸指针；应使用 Rc<RefCell<T>> 直接共享",
    ),
    PatternRule(
        "revision-zero-bypass",
        re.compile(r"\bexpected_revision\s*==\s*0\b"),
        "禁止使用 0 绕过编辑器版本校验",
    ),
    PatternRule(
        "assert-unwind-safe",
        re.compile(r"\b(?:std::panic::)?AssertUnwindSafe\b"),
        "禁止用 AssertUnwindSafe 掩盖未建立的 panic/借用边界",
    ),
    PatternRule(
        "transmute-pointer-escape",
        re.compile(r"\bstd::mem::transmute\s*[<(]"),
        "禁止用 transmute 绕过类型系统",
    ),
    PatternRule(
        "production-lock-unwrap",
        re.compile(r"\.lock\(\)\s*\.\s*unwrap\(\)"),
        "生产代码中 Mutex lock().unwrap() 可能因锁中毒 panic；应使用 lock().unwrap_or_else 或 ok()",
    ),
    PatternRule(
        "from-error-string-usage",
        re.compile(r"\bfrom_error_string\s*\("),
        "from_error_string 已废弃；应使用 from_code(error.sync_category(), msg) 做结构化分类",
    ),
    PatternRule(
        "cpp-unsafe-call",
        re.compile(r"cpp!\s*\(\s*unsafe\s*\[(?!\s*\])"),
        "cpp!(unsafe [...]) 捕获指针时必须说明来源、有效期、所属线程和 null 前提；空捕获列表 [] 不触发此规则",
    ),
)

SKIP_DIRS = {
    ".git",
    "target",
    "build",
    ".gradle",
    "generated",
    "vendor",
    "third_party",
}

_STRING_RE = re.compile(r'"(?:\\.|[^"\\])*"')
_RAW_STRING_RE = re.compile(r'r(?P<hashes>#{0,16})".*?"(?P=hashes)')


def _code_part(line: str) -> str:
    """去掉单行字符串和行注释，避免文案/注释触发规则。"""
    without_raw_strings = _RAW_STRING_RE.sub('""', line)
    without_strings = _STRING_RE.sub('""', without_raw_strings)
    return without_strings.split("//", 1)[0]


def _has_safety_comment(lines: list[str], index: int) -> bool:
    for previous in lines[max(0, index - 4) : index]:
        stripped = previous.strip()
        if stripped and "SAFETY:" in stripped:
            return True
    return False


def _has_nearby_deprecated(lines: list[str], index: int, lookback: int = 3) -> bool:
    for previous in lines[max(0, index - lookback) : index]:
        stripped = previous.strip()
        if stripped and "#[deprecated" in stripped:
            return True
    return False


_QMETA_MACRO_PATTERNS = [
    re.compile(r"\bqt_property!\s*\("),
    re.compile(r"\bqt_signal!\s*\(\s*\)"),
    re.compile(r"\bqt_method!\s*\("),
    re.compile(r"\bqt_base_class!\s*\("),
]

_QMETA_DERIVE_PATTERN = re.compile(r"#\[derive\s*\([^)]*\bQObject\b")


def _is_struct_level_allow(lines: list[str], index: int) -> bool:
    for i in range(index + 1, min(len(lines), index + 5)):
        stripped = lines[i].strip()
        if not stripped or stripped.startswith("//") or stripped.startswith("#["):
            continue
        if stripped.startswith("pub struct ") or stripped.startswith("struct "):
            return True
        if _QMETA_DERIVE_PATTERN.search(stripped):
            return True
        return False
    return False


def _is_qmetaobject_macro_field(lines: list[str], index: int) -> bool:
    if _is_struct_level_allow(lines, index):
        return False
    for i in range(index, min(len(lines), index + 3)):
        stripped = lines[i].strip()
        for pat in _QMETA_MACRO_PATTERNS:
            if pat.search(stripped):
                return True
    return False


_RULES_WITH_DEPRECATED = {
    "from-error-string-usage",
}

_RULES_PRODUCTION_ONLY = {
    "production-lock-unwrap",
    "cpp-unsafe-call",
}

_ASSERT_UNWIND_SAFE_WHITELIST_PATHS = {
    Path("apps/Linux_qt/src/backend/sync_backend.rs"),
    Path("apps/Linux_qt/src/backend/sync_operations.rs"),
    Path("apps/Linux_qt/src/editor/input/platform_ime.rs"),
    Path("core/writer_core/src/sync/github_backend.rs"),
}


def _is_in_whitelisted_path(path: Path, whitelist: set[Path]) -> bool:
    for allowed in whitelist:
        try:
            path.relative_to(allowed)
            return True
        except ValueError:
            if path == allowed:
                return True
    return False


def _is_in_test_context(lines: list[str], index: int) -> bool:
    for i in range(max(0, index - 80), index + 1):
        stripped = lines[i].strip()
        if stripped.startswith("#[test]") or stripped.startswith("#[tokio::test]"):
            return True
        if re.match(r"^\s*fn\s+test_", stripped) or re.match(r"^\s*async\s+fn\s+test_", stripped):
            return True
        if stripped == "mod tests {" or stripped.startswith("mod tests"):
            return True
    if len(lines) > 1 and lines[0].strip().startswith("#[cfg(test)]"):
        return True
    return False


def scan_text(path: Path, text: str) -> list[Finding]:
    findings: list[Finding] = []
    lines = text.splitlines()

    for index, line in enumerate(lines):
        code = _code_part(line)
        for rule in RULES:
            if rule.pattern.search(code):
                if rule.name == "assert-unwind-safe":
                    if _is_in_whitelisted_path(path, _ASSERT_UNWIND_SAFE_WHITELIST_PATHS):
                        continue
                if rule.name == "broad-dead-code-allow":
                    if _is_qmetaobject_macro_field(lines, index):
                        continue
                if rule.name in _RULES_WITH_DEPRECATED:
                    if _has_nearby_deprecated(lines, index):
                        continue
                if rule.name in _RULES_PRODUCTION_ONLY:
                    if _is_in_test_context(lines, index):
                        continue
                findings.append(Finding(path, index + 1, rule.name, rule.message))

        if re.search(r"\bunsafe\s*\{", code) and not _has_safety_comment(lines, index):
            findings.append(
                Finding(
                    path,
                    index + 1,
                    "undocumented-unsafe-block",
                    "unsafe 块前必须有 SAFETY: 注释说明生命周期、别名和线程前提",
                )
            )

    return findings


def iter_rust_files(root: Path) -> Iterable[Path]:
    for path in root.rglob("*.rs"):
        if any(part in SKIP_DIRS for part in path.parts):
            continue
        yield path


def scan_repository(root: Path) -> list[Finding]:
    findings: list[Finding] = []
    for path in iter_rust_files(root):
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            findings.append(
                Finding(path, 1, "non-utf8-rust-source", "Rust 源文件必须是 UTF-8")
            )
            continue
        findings.extend(scan_text(path.relative_to(root), text))
    return findings


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("root", nargs="?", default=".", help="仓库根目录")
    args = parser.parse_args(argv)

    root = Path(args.root).resolve()
    findings = scan_repository(root)
    for finding in findings:
        print(f"{finding.path}:{finding.line}: {finding.rule}: {finding.message}")

    if findings:
        print(f"发现 {len(findings)} 个 Rust 安全策略问题。", file=sys.stderr)
        return 1
    print("Rust 安全策略扫描通过。")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
