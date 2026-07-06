#!/usr/bin/env python3
"""QML DesignTokens (dt) 传递检查脚本。

扫描所有 QML 文件，检查 AppText/AppButton/HubPageHeader/EditorAnimationOverlay
是否正确传递了 dt 属性。

方案 A：所有基础组件 property var dt，调用处必须传 dt。
null guard 只能作为最后安全兜底，不能掩盖调用处没传 token 的问题。
此脚本作为静态检查保证 dt 注入正确。
"""

import re
import sys
from pathlib import Path

COMPONENTS_REQUIRING_DT = [
    "AppText",
    "AppButton",
    "HubPageHeader",
    "EditorAnimationOverlay",
]

def check_qml_file(filepath):
    """检查单个 QML 文件中关键组件的 dt 传递情况。"""
    errors = []
    content = filepath.read_text(encoding='utf-8')
    lines = content.split('\n')
    
    for component in COMPONENTS_REQUIRING_DT:
        for i, line in enumerate(lines):
            stripped = line.strip()
            if f'{component} {{' in stripped or f'{component}{{' in stripped:
                found_dt = False
                for j in range(i, min(i + 15, len(lines))):
                    if 'dt:' in lines[j] or 'dt :' in lines[j]:
                        found_dt = True
                        break
                    if '}' in lines[j] and j > i:
                        break
                if not found_dt:
                    errors.append(f"Line {i+1}: {component} without dt: property")
    
    return errors

def main():
    qml_dir = Path(__file__).parent.parent / "apps" / "Linux_qt" / "qml"
    if not qml_dir.exists():
        print(f"QML directory not found: {qml_dir}")
        sys.exit(1)
    
    total_errors = 0
    for qml_file in sorted(qml_dir.glob("*.qml")):
        errors = check_qml_file(qml_file)
        if errors:
            print(f"\n{qml_file.name}:")
            for error in errors:
                print(f"  {error}")
            total_errors += len(errors)
    
    if total_errors > 0:
        print(f"\n❌ Found {total_errors} dt property issues")
        sys.exit(1)
    else:
        component_list = ", ".join(COMPONENTS_REQUIRING_DT)
        print(f"✅ All {component_list} have dt: property")
        sys.exit(0)

if __name__ == "__main__":
    main()
