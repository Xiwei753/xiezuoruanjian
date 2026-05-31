# QML UI 视觉系统

本目录包含所有的 QML UI 组件和页面。
所有业务页面必须强制组件化，禁止直接实例化基础图形元素，必须使用白名单系统中的业务/视觉组件（如 `AppText`, `StatusPill`, `AppTextField`）。
