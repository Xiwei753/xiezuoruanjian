#!/usr/bin/env python3
"""QML DesignTokens (dt) 传递检查脚本。

扫描所有 QML 文件，检查 AppText/AppButton/HubPageHeader 是否正确传递了 dt 属性。
"""

import re
import sys
from pathlib import Path

def check_qml_file(filepath):
    """检查单个 QML 文件中 AppText/AppButton/HubPageHeader 的 dt 传递情况。"""
    errors = []
    content = filepath.read_text(encoding='utf-8')
    lines = content.split('\n')
    
    # 检查 AppText { ... } 块是否有 dt:
    # 简单检查：如果行包含 "AppText {" 且后续行没有 "dt:" 则报错
    for i, line in enumerate(lines):
        stripped = line.strip()
        if 'AppText {' in stripped or 'AppText{' in stripped:
            # 检查后续 10 行内是否有 dt: 属性
            found_dt = False
            for j in range(i, min(i + 10, len(lines))):
                if 'dt:' in lines[j] or 'dt :' in lines[j]:
                    found_dt = True
                    break
                if '}' in lines[j] and j > i:
                    break
            if not found_dt:
                errors.append(f"Line {i+1}: AppText without dt: property")
        
        if 'AppButton {' in stripped or 'AppButton{' in stripped:
            found_dt = False
            for j in range(i, min(i + 10, len(lines))):
                if 'dt:' in lines[j] or 'dt :' in lines[j]:
                    found_dt = True
                    break
                if '}' in lines[j] and j > i:
                    break
            if not found_dt:
                errors.append(f"Line {i+1}: AppButton without dt: property")
        
        if 'HubPageHeader {' in stripped or 'HubPageHeader{' in stripped:
            found_dt = False
            for j in range(i, min(i + 10, len(lines))):
                if 'dt:' in lines[j] or 'dt :' in lines[j]:
                    found_dt = True
                    break
                if '}' in lines[j] and j > i:
                    break
            if not found_dt:
                errors.append(f"Line {i+1}: HubPageHeader without dt: property")
    
    return errors

def main():
    qml_dir = Path(__file__).parent.parent / "apps" / "desktop" / "qml"
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
        print("✅ All AppText/AppButton/HubPageHeader have dt: property")
        sys.exit(0)

if __name__ == "__main__":
    main()
