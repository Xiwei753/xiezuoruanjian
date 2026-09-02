#!/usr/bin/env python3
"""把生产源码内嵌 #[cfg(test)] mod tests { ... } 拆到 <stem>/tests.rs。

仅处理命令行传入的文件；对每个文件：
1. 找到形如 `#[cfg(test)]\nmod tests {` 的块。
2. 用大括号深度找到匹配的闭合 `}`（跳过字符串/注释内的括号）。
3. 把 mod tests 内部内容写到 <parent>/<stem>/tests.rs。
4. 把原文件中整个块替换为 `#[cfg(test)]\nmod tests;`。

不修改测试逻辑，只移动位置。
"""

from __future__ import annotations

import re
import sys
from pathlib import Path


_CFG_TEST_RE = re.compile(r"^\s*#\[\s*cfg\s*\(\s*test\s*\)\s*\]\s*$")
_MOD_TESTS_RE = re.compile(r"^\s*mod\s+tests\s*\{")

# 匹配 Rust 字符串字面量（含转义）、行注释、块注释、字符字面量、生命周期
# 顺序很重要：先匹配字符串和注释
_TOKEN_RE = re.compile(
    r"""
    (?P<str>"(?:\\.|[^"\\])*")          # 字符串字面量
  | (?P<line_comment>//[^\n]*)          # 行注释
  | (?P<block_comment>/\*.*?\*/)        # 块注释（DOTALL）
  | (?P<char>'(?:\\.|[^'\\])')          # 字符字面量 'x' 或 '\n'
  | (?P<lifetime>'[A-Za-z_][A-Za-z0-9_]*)  # 生命周期 'a
    """,
    re.VERBOSE | re.DOTALL,
)


def strip_strings_and_comments(text: str) -> str:
    """把字符串/注释内容替换成等长空格，保留换行符位置。"""
    def replace(m: re.Match[str]) -> str:
        s = m.group(0)
        # 保留换行符，其余替换为空格
        return "".join(c if c == "\n" else " " for c in s)

    return _TOKEN_RE.sub(replace, text)


def find_block_end(lines: list[str], open_idx: int) -> int | None:
    """从含 `{` 的行 open_idx 开始，找匹配的闭合 `}` 行号。"""
    # 把整个文件拼起来，剥离字符串/注释，再按行 split
    full = "".join(lines)
    stripped = strip_strings_and_comments(full)
    stripped_lines = stripped.splitlines(keepends=True)
    depth = 0
    for idx in range(open_idx, len(stripped_lines)):
        for ch in stripped_lines[idx]:
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth == 0:
                    return idx
    return None


def extract(file_path: Path) -> bool:
    text = file_path.read_text(encoding="utf-8")
    lines = text.splitlines(keepends=True)
    cfg_idx = None
    mod_idx = None
    for idx, line in enumerate(lines):
        if _CFG_TEST_RE.match(line):
            for look in range(idx + 1, min(len(lines), idx + 4)):
                if _MOD_TESTS_RE.match(lines[look]):
                    cfg_idx = idx
                    mod_idx = look
                    break
            if cfg_idx is not None:
                break
    if cfg_idx is None:
        print(f"[skip] {file_path}: 未找到 #[cfg(test)] mod tests")
        return False

    end_idx = find_block_end(lines, mod_idx)
    if end_idx is None:
        print(f"[skip] {file_path}: 未找到 mod tests 闭合")
        return False

    inner_lines = lines[mod_idx + 1 : end_idx]
    # 去掉首尾空行
    while inner_lines and inner_lines[0].strip() == "":
        inner_lines.pop(0)
    while inner_lines and inner_lines[-1].strip() == "":
        inner_lines.pop()

    inner_text = "".join(inner_lines)
    if not inner_text.endswith("\n"):
        inner_text += "\n"

    stem = file_path.stem
    parent = file_path.parent
    sub_dir = parent / stem
    sub_dir.mkdir(parents=True, exist_ok=True)
    tests_rs = sub_dir / "tests.rs"
    tests_rs.write_text(inner_text, encoding="utf-8")

    new_lines = lines[:cfg_idx]
    if new_lines and new_lines[-1].strip() != "":
        new_lines.append("\n")
    new_lines.append("#[cfg(test)]\n")
    new_lines.append("mod tests;\n")
    rest = lines[end_idx + 1 :]
    while rest and rest[0].strip() == "" and new_lines and new_lines[-1].strip() == "":
        rest.pop(0)
    new_lines.extend(rest)

    new_text = "".join(new_lines)
    file_path.write_text(new_text, encoding="utf-8")

    block_lines = end_idx - cfg_idx + 1
    print(
        f"[ok] {file_path}: 移出 {block_lines} 行测试到 {tests_rs} "
        f"(inner {len(inner_lines)} 行)"
    )
    return True


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: _extract_test_mods.py <file1> [file2 ...]", file=sys.stderr)
        return 2
    ok = 0
    for arg in sys.argv[1:]:
        if extract(Path(arg)):
            ok += 1
    print(f"处理完成: {ok}/{len(sys.argv) - 1}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
