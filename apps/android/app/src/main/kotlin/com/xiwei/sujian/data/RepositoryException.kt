package com.xiwei.sujian.data

/**
 * RepositoryException — 仓库层异常类
 *
 * 用于 WorkspaceRepository 中统一错误处理，将 BridgeResult.Error 转换为异常。
 *
 * ## 架构定位
 * - 继承 RuntimeException
 * - 由 WorkspaceRepository 抛出，由 ViewModel 捕获
 *
 * ## 使用场景
 * - 领域 Bridge 返回错误时抛出
 * - ViewModel 中通过 try-catch 捕获并展示给用户
 */
class RepositoryException(message: String) : RuntimeException(message)
