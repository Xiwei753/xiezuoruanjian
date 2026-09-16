#!/usr/bin/env python3
"""QML DesignTokens (dt) 传递检查脚本。

扫描所有 QML 文件，检查基础组件是否正确传递了 dt 属性。

Issue #701 评论 5699565102:
- 删除"基础组件必须有 fallbackDt"的检查。
- 反过来禁止基础组件内部再创建 DesignTokens。
- 继续检查调用处必须传 dt:。

方案 A：所有基础组件 property var dt，调用处必须传 dt。
漏传就是调用错误，不能掩盖。此脚本作为静态检查保证 dt 注入正确。
"""

import re
import sys
from pathlib import Path

# 基础组件：调用处必须传 dt，且组件内部禁止创建 DesignTokens。
COMPONENTS_REQUIRING_DT = [
    "AppText",
    "AppButton",
    "AppCard",
    "AppSlider",
    "AppTextField",
    "HubPageHeader",
    "SettingsRow",
    "StatusPill",
    "SyncPage",
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
                # 跳过根元素子类化（如 SectionHeader.qml 根元素是 AppText）：
                # 无缩进的声明是文件根 QML 对象，它继承基础组件的 dt 属性，
                # 不是"调用"基础组件，不需要传 dt。
                if not line.startswith(' ') and not line.startswith('\t'):
                    continue
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

def check_component_no_internal_designtokens(filepath):
    """Issue #701 评论 5699565102: 禁止基础组件内部再创建 DesignTokens。

    基础组件必须消费调用方传入的根 dt，不能偷偷生成一份独立主题。
    """
    errors = []
    content = filepath.read_text(encoding='utf-8')
    filename = filepath.name
    
    if filename in [c + ".qml" for c in COMPONENTS_REQUIRING_DT]:
        # 禁止组件内部创建 DesignTokens（fallbackDt 或任何其他 id）
        if re.search(r'DesignTokens\s*\{', content):
            errors.append(
                f"{filename}: basic component must not create DesignTokens "
                f"internally; consume root dt from caller instead"
            )
        # 必须有 resolvedDt 绑定到 dt
        if 'resolvedDt' not in content:
            errors.append(f"{filename}: missing resolvedDt property")
        # 禁止直接 dt.xxx 访问（必须走 resolvedDt）
        direct_dt_access = re.findall(r'(?<!resolved)(?<!\w)dt\.\w+', content)
        if direct_dt_access:
            for match in direct_dt_access:
                errors.append(f"{filename}: direct dt.xxx access without resolvedDt: '{match}'")
    
    return errors

def main():
    qml_dir = Path(__file__).parent.parent / "apps" / "Linux_qt" / "qml"
    if not qml_dir.exists():
        print(f"QML directory not found: {qml_dir}")
        sys.exit(1)
    
    total_errors = 0
    for qml_file in sorted(qml_dir.glob("*.qml")):
        errors = check_qml_file(qml_file)
        internal_errors = check_component_no_internal_designtokens(qml_file)
        all_errors = errors + internal_errors
        if all_errors:
            print(f"\n{qml_file.name}:")
            for error in all_errors:
                print(f"  {error}")
            total_errors += len(all_errors)
    
    if total_errors > 0:
        print(f"\n❌ Found {total_errors} dt property issues")
        sys.exit(1)
    else:
        component_list = ", ".join(COMPONENTS_REQUIRING_DT)
        print(f"✅ All {component_list} have dt: property and no internal DesignTokens")
        sys.exit(0)

if __name__ == "__main__":
    main()
