#!/usr/bin/env python3
"""检查 codelinter 的 JSON 报告，发现 error 级缺陷时以非零退出码失败。

codelinter 的启动脚本只把退出码 1/127/255 透传为失败，规则命中的缺陷不会让
命令本身返回非零，所以静态门禁必须自己读报告。
"""

from __future__ import annotations

import json
import sys
from pathlib import Path


def load_report(path: Path) -> list[object] | None:
    if not path.is_file():
        print(f"错误：codelinter 报告不存在：{path}", file=sys.stderr)
        return None
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        print(f"错误：无法解析 codelinter 报告 {path}：{exc}", file=sys.stderr)
        return None
    if not isinstance(data, list):
        print(f"错误：codelinter 报告格式不正确（期望 JSON 数组）：{path}", file=sys.stderr)
        return None
    return data


def collect_errors(report: list[object]) -> list[str]:
    found: list[str] = []
    for entry in report:
        if not isinstance(entry, dict):
            continue
        messages = entry.get("messages")
        if not isinstance(messages, list):
            continue
        for message in messages:
            if not isinstance(message, dict):
                continue
            if str(message.get("severity", "")).lower() != "error":
                continue
            found.append(
                f"{entry.get('filePath')}:{message.get('line')}:{message.get('column')} "
                f"{message.get('rule')} {message.get('message')}"
            )
    return found


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print(f"用法：{argv[0]} <codelinter-json-report>", file=sys.stderr)
        return 2

    report = load_report(Path(argv[1]))
    if report is None:
        return 1

    errors = collect_errors(report)
    if errors:
        print(f"CodeLinter 报出 {len(errors)} 条 error 级缺陷：", file=sys.stderr)
        for line in errors:
            print(f"  {line}", file=sys.stderr)
        return 1

    print("CodeLinter 报告无 error 级缺陷。")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
