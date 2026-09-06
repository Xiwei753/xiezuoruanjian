package com.xiwei.sujian.app.di

import android.content.Context
import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.core.platform.app.AndroidAppVersionProvider
import com.xiwei.sujian.core.platform.device.AndroidDeviceIdentity
import com.xiwei.sujian.core.platform.network.AndroidNetworkMonitor
import com.xiwei.sujian.core.platform.storage.documents.DocumentTreeReader
import android.net.Uri
import com.xiwei.sujian.core.platform.storage.downloads.MediaStoreDownloads
import com.xiwei.sujian.storage.mirror.CoreMirrorSnapshotSource
import com.xiwei.sujian.storage.mirror.DefaultMirrorChangeSink
import com.xiwei.sujian.storage.mirror.DocumentTreeMirrorStorage
import com.xiwei.sujian.storage.mirror.MediaStoreMirrorStorage
import com.xiwei.sujian.storage.mirror.MirrorStorageRouter
import com.xiwei.sujian.storage.mirror.ReadableMirrorPublisher
import com.xiwei.sujian.storage.mirror.ReadableMirrorStateStore
import java.util.Locale
import java.util.TimeZone

/**
 * AppServiceProvider — 应用级服务桥接提供者。
 *
 * 只保留 AppServiceBridge 唯一获取入口。
 * 统计桥接不再保留第二获取入口（#618 六）：应用容器里的唯一
 * WritingStatsRepository 直接使用 AppServiceBridge.statsBridge，
 * UI 不再绕回此提供者取 Bridge。
 * 平台相关逻辑已下沉到 :core:platform：
 * - 网络状态检测与回调注册 → AndroidNetworkMonitor
 * - 设备 ID 持久化 → AndroidDeviceIdentity
 * - 应用版本读取 → AndroidAppVersionProvider
 *
 * #649 评论 5560971132 修复 1/6：在此组装镜像发布链：
 * `CoreMirrorSnapshotSource → ReadableMirrorPublisher → DefaultMirrorChangeSink → AppServiceBridge`。
 * Publisher 只依赖 SnapshotSource（只读），不持有 AppServiceBridge，切断循环依赖。
 *
 * #649 评论 5561465552 第 3 点：Publisher 改用 [com.xiwei.sujian.storage.mirror.ReadableMirrorStorage]
 * 接口，由 [selectMirrorStorage] 根据 stateStore.backend 选择 MediaStore 或 SAF 后端。
 *
 * UI 层不应直接引用此类（架构分层规则 #597），应通过各 Repository/容器间接访问。
 */
object AppServiceProvider {
    private const val TAG = "AppServiceProvider"

    @Volatile
    private var appServiceInstance: AppServiceBridge? = null

    fun getAppServiceBridge(context: Context): AppServiceBridge {
        val testProvider = SujianAppDependencies.getTestProvider()
        if (testProvider != null) {
            return testProvider(context).appServiceBridge
        }
        return appServiceInstance ?: synchronized(this) {
            appServiceInstance ?: run {
                createAppServiceBridge(context.applicationContext).also { appServiceInstance = it }
            }
        }
    }

    private fun createAppServiceBridge(appContext: Context): AppServiceBridge {
        val (isConnected, isMetered) = AndroidNetworkMonitor.detectNetworkState(appContext)
        val holder =
            WriterAppServiceHolder.createFromContext(
                context = appContext,
                cacheDir = appContext.cacheDir.absolutePath,
                noBackupDir = appContext.noBackupFilesDir.absolutePath,
                deviceId = AndroidDeviceIdentity.getOrCreateDeviceId(appContext),
                appVersion = AndroidAppVersionProvider.getAppVersion(appContext),
                locale = Locale.getDefault().toLanguageTag(),
                timezone = TimeZone.getDefault().id,
                isConnected = isConnected,
                isMetered = isMetered,
            )
        // 组装镜像发布链（#649 修复 1/6）：
        // SnapshotSource 只读 Core 快照；Publisher 不持有 AppServiceBridge，切断循环。
        val snapshotSource = CoreMirrorSnapshotSource(holder)
        val stateStore = ReadableMirrorStateStore(appContext)
        // #649 评论 5561974464 问题 1：SAF 恢复后 Publisher 不会立即切换到 DocumentTree 后端。
        // 旧实现 selectMirrorStorage 在启动时一次性固化 storage，Publisher 握着固定实例。
        // 新实现：创建 MirrorStorageRouter，Publisher 每次事务时从 router.current() 获取。
        val documentTreeReader = DocumentTreeReader(appContext.contentResolver)
        val mediaStoreStorage = MediaStoreMirrorStorage(MediaStoreDownloads(appContext.contentResolver))
        val documentTreeFactory: (Uri) -> DocumentTreeMirrorStorage = { treeUri ->
            DocumentTreeMirrorStorage(treeUri, appContext.contentResolver, documentTreeReader)
        }
        val router = MirrorStorageRouter(stateStore, mediaStoreStorage, documentTreeFactory)
        val publisher = ReadableMirrorPublisher(snapshotSource, router, stateStore)
        val mirrorChangeSink = DefaultMirrorChangeSink(publisher)
        val bridge = AppServiceBridge(holder, mirrorChangeSink)
        AndroidNetworkMonitor.registerNetworkCallback(appContext) {
            refreshNetworkState(appContext, bridge)
        }
        return bridge
    }

    private fun refreshNetworkState(
        context: Context,
        bridge: AppServiceBridge,
    ) {
        val (isConnected, isMetered) = AndroidNetworkMonitor.detectNetworkState(context)
        try {
            bridge.holder.service.updateNetworkState(isConnected, isMetered, null, null)
            DiagnosticsLogger.d(TAG, "Network state updated: connected=$isConnected, metered=$isMetered")
        } catch (e: UnsatisfiedLinkError) {
            DiagnosticsLogger.e(TAG, "Native library not loaded, skipping network state update", e)
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Failed to update network state", e)
        }
    }
}
