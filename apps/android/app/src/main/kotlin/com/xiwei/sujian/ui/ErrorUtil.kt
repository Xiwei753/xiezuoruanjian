package com.xiwei.sujian.ui

import android.content.Context
import android.util.Log
import android.widget.Toast
import com.xiwei.sujian.data.RepositoryException

/**
 * ErrorUtil — 错误处理工具类
 *
 * 提供统一的错误处理机制，将 RepositoryException 转换为用户友好的 Toast 提示。
 *
 * ## 架构定位
 * - 全局工具类，被 Activity 和 ViewModel 调用
 * - 捕获 RepositoryException 并展示错误信息
 *
 * ## 使用场景
 * - SettingsActivity 中的安全操作执行
 * - 各种 Activity 中的错误处理
 */
object ErrorUtil {
    private const val TAG = "SujianError"

    fun <T> safeRun(context: Context, fallback: T, action: () -> T): T {
        return try {
            action()
        } catch (e: RepositoryException) {
            Log.e(TAG, e.message ?: "Repository error", e)
            Toast.makeText(context, e.message, Toast.LENGTH_LONG).show()
            fallback
        } catch (e: Throwable) {
            Log.e(TAG, "Unexpected error", e)
            Toast.makeText(context, "应用内部错误，请查看日志", Toast.LENGTH_LONG).show()
            fallback
        }
    }

    suspend fun <T> safeRunSuspend(context: Context, fallback: T, action: suspend () -> T): T {
        return try {
            action()
        } catch (e: RepositoryException) {
            Log.e(TAG, e.message ?: "Repository error", e)
            Toast.makeText(context, e.message, Toast.LENGTH_LONG).show()
            fallback
        } catch (e: Throwable) {
            Log.e(TAG, "Unexpected error", e)
            Toast.makeText(context, "应用内部错误，请查看日志", Toast.LENGTH_LONG).show()
            fallback
        }
    }

    fun safeRun(context: Context, action: () -> Unit) {
        try {
            action()
        } catch (e: RepositoryException) {
            Log.e(TAG, e.message ?: "Repository error", e)
            Toast.makeText(context, e.message, Toast.LENGTH_LONG).show()
        } catch (e: Throwable) {
            Log.e(TAG, "Unexpected error", e)
            Toast.makeText(context, "应用内部错误，请查看日志", Toast.LENGTH_LONG).show()
        }
    }

    suspend fun safeRunSuspend(context: Context, action: suspend () -> Unit) {
        try {
            action()
        } catch (e: RepositoryException) {
            Log.e(TAG, e.message ?: "Repository error", e)
            Toast.makeText(context, e.message, Toast.LENGTH_LONG).show()
        } catch (e: Throwable) {
            Log.e(TAG, "Unexpected error", e)
            Toast.makeText(context, "应用内部错误，请查看日志", Toast.LENGTH_LONG).show()
        }
    }
}
