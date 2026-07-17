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
        "revision-zero-bypass",
        re.compile(r"\bexpected_revision\s*==\s*0\b"),
        "禁止使用 0 绕过编辑器版本校验",
    ),
    PatternRule(
        "assert-unwind-safe",
        re.compile(r"\b(?:std::panic::)?AssertUnwindSafe\b"),
        "禁止用 AssertUnwindSafe 掩盖未建立的 panic/借用边界",
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


def scan_text(path: Path, text: str) -> list[Finding]:
    findings: list[Finding] = []
    lines = text.splitlines()

    for index, line in enumerate(lines):
        code = _code_part(line)
        for rule in RULES:
            if rule.pattern.search(code):
                findings.append(Finding(path, index + 1, rule.name, rule.message))

        # 正常 FFI/平台边界允许 unsafe，但必须在紧邻位置写清安全前提。
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
