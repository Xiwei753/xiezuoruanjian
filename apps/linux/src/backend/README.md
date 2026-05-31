# Linux Rust Backend (桥接层)

本目录作为 QML 请求到 Core API 调用的桥接翻译层。
必须不能包含 UI 状态机或平铺复杂异步结果。向前端返回的异步操作或状态应该是标准结构化 JSON 或 QObject 属性集合，让前端自己决定如何展示。
