package com.xiwei.sujian.core.model

/**
 * App 层不可变标记注解 — 替代 `androidx.compose.runtime.Immutable`。
 *
 * 用于标记编译器可推断为不可变的数据类，帮助 Compose 编译器优化重组。
 * 此注解不引入对 Compose 运行时的依赖，使 motion/visual 等子模块
 * 可以在不依赖 Compose UI 框架的前提下声明不可变性。
 *
 * Compose 编译器插件在编译期识别 `androidx.compose.runtime.Immutable`，
 * 本注解需在 Compose 编译器插件配置中注册为额外不可变注解，
 * 或由使用方在 Compose 入口处通过 `@Immutable` 代理。
 */
@Target(AnnotationTarget.CLASS)
@Retention(AnnotationRetention.BINARY)
annotation class Immutable
