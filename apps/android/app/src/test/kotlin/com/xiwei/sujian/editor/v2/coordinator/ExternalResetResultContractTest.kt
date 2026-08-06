package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #595 五：外部正文 reset 可提交事务结果契约测试。
 *
 * 旧实现 resetPersistentSession 返回 Unit，Core reset 失败时 WritingPane 仍无条件
 * 推进 applyExternalContentFact + applyExternalContentToUi，导致 Rust session（旧正文）/
 * SessionStore（新版本）/ViewModel（新正文+hash）三份状态分裂。
 *
 * 修复：resetPersistentSession 返回 [ExternalResetResult] — Success 携带真实 snapshot，
 * Failed 表示 reset 未执行或 Core 失败；调用方仅 Success 时才推进会话事实与 UI。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ExternalResetResultContractTest {

    private fun createCoordinator(): EditorSessionCoordinator {
        return EditorSessionCoordinator(com.xiwei.sujian.data.AppServiceBridge(
            com.xiwei.sujian.data.WriterAppServiceHolder("/tmp/sujian_test_workspace_595_reset")
        ))
    }

    @Test
    fun resetPersistentSession_returnsExternalResetResultType() {
        val method = EditorSessionCoordinator::class.java.methods.firstOrNull {
            it.name == "resetPersistentSession" &&
            it.returnType == com.xiwei.sujian.editor.v2.coordinator.ExternalResetResult::class.java
        }
        assertTrue(
            "resetPersistentSession must return ExternalResetResult — Unit 让调用方无法检测 reset 失败 (#595 五)",
            method != null,
        )
    }

    @Test
    fun resetPersistentSession_localContentChanged_returnsFailed() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("t1", TextEditorProfile.DocumentBody, persistent = true)
        val result = coordinator.resetPersistentSession("t1", "text", 0, SessionResetSource.LOCAL_CONTENT_CHANGED)
        assertEquals(
            "LOCAL_CONTENT_CHANGED 不得执行 reset — 返回 Failed",
            ExternalResetResult.Failed,
            result,
        )
    }

    @Test
    fun resetPersistentSession_nonPersistentTarget_returnsFailed() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("t1", TextEditorProfile.DocumentBody, persistent = false)
        val result = coordinator.resetPersistentSession("t1", "text", 0, SessionResetSource.EXTERNAL)
        assertEquals(
            "非持久 target 不得 reset — 返回 Failed",
            ExternalResetResult.Failed,
            result,
        )
    }

    @Test
    fun resetPersistentSession_unregisteredTarget_returnsFailed() {
        val coordinator = createCoordinator()
        val result = coordinator.resetPersistentSession("unknown", "text", 0, SessionResetSource.EXTERNAL)
        assertEquals(
            "未注册 target 不得 reset — 返回 Failed",
            ExternalResetResult.Failed,
            result,
        )
    }

    @Test
    fun resetPersistentSession_persistentWithoutNativeSession_returnsFailed() {
        // 持久 target 但无 native（测试环境 createSession 返回 null）→ Failed，
        // 不推进任何状态。旧实现返回 Unit，调用方无法区分成功/失败。
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("t1", TextEditorProfile.DocumentBody, persistent = true)
        val result = coordinator.resetPersistentSession("t1", "text", 0, SessionResetSource.EXTERNAL)
        assertEquals(
            "无 native 时 createSession 失败必须返回 Failed",
            ExternalResetResult.Failed,
            result,
        )
    }

    @Test
    fun externalResetResult_sealedHierarchyHasSuccessAndFailed() {
        val success = ExternalResetResult.Success(TargetSnapshot("t", 0, 0L, 0, 0))
        val failed = ExternalResetResult.Failed
        assertTrue(success is ExternalResetResult.Success)
        assertTrue(failed is ExternalResetResult.Failed)
        assertEquals("t", success.snapshot.text)
    }
}
