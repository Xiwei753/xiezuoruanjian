package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 二：一次正文更新只能有一次 Core 命令契约测试。
 *
 * 旧缺陷：外部正文更新先经 AppService 重置 Core session（textEditSessionReset），
 * 随后 View.loadText() 又对同一 session 调一次 Core load（revision 连续变化两次、
 * Undo/Redo 与 composition 重复清空）；新建 session 也存在
 * createSession(initialText) + bindSession→loadText 的重复。
 *
 * 修复：平台加载新正文只选择一个入口执行一次 Core 命令，View 只通过
 * attachSnapshot 把真实 snapshot 装入本地 mirror/layout。本测试验证结构契约：
 * - SujianEditorView 不再暴露会触发第二次 Core load 的 loadText(String, Int)；
 * - attachSession（attachSnapshot 路径）存在且参数完整；
 * - EditorWindowHost.resetPersistentSession 通过 attachSnapshotToView 附着，
 *   不再调用 view.loadText；
 * - prepareSessionForEdit 携带 initialText（窗口层提供正文），
 *   SessionBindInfo 携带真实 snapshot（新建/复用都走 attach）。
 */
class SingleCoreResetContractTest {

    @Test
    fun sujianEditorView_loadTextMethod_removed() {
        val method = com.xiwei.sujian.editor.v2.host.SujianEditorView::class.java.methods.firstOrNull {
            it.name == "loadText" &&
            it.parameterTypes.size == 2 &&
            it.parameterTypes[0] == String::class.java
        }
        assertTrue(
            "SujianEditorView.loadText(String, Int) must be removed — " +
            "it triggered a second Core textEditSessionLoadText after create/reset (#595 二)",
            method == null,
        )
    }

    @Test
    fun sujianEditorView_attachSession_exists() {
        val method = com.xiwei.sujian.editor.v2.host.SujianEditorView::class.java.methods.firstOrNull {
            it.name == "attachSession"
        }
        assertNotNull(
            "SujianEditorView.attachSession must exist for snapshot-only rebind",
            method,
        )
    }

    @Test
    fun sujianEditorView_attachSnapshotSameSession_exists() {
        // #595 二：同一 session 的外部内容重置走 attachSnapshotSameSession —
        // 不解除绑定/不清回调/不隐藏 IME（外部替换可能发生在输入过程中）。
        val method = com.xiwei.sujian.editor.v2.host.SujianEditorView::class.java.methods.firstOrNull {
            it.name == "attachSnapshotSameSession"
        }
        assertNotNull(
            "SujianEditorView.attachSnapshotSameSession must exist for in-session external reset",
            method,
        )
    }

    @Test
    fun editorWindowHost_resetPersistentSession_usesAttachSnapshotPath() {
        // #595 二：resetPersistentSession 在 Core reset 后只把真实 snapshot
        // 装入 View（attachSnapshotToView），不再调用 view.loadText 二次 Core 命令。
        val attachMethod = EditorWindowHost::class.java.declaredMethods.firstOrNull {
            it.name == "attachSnapshotToView"
        }
        assertNotNull(
            "EditorWindowHost must have private attachSnapshotToView for snapshot-only rebind",
            attachMethod,
        )
        val loadTextCall = EditorWindowHost::class.java.declaredMethods.none {
            it.name == "loadText"
        }
        assertTrue("EditorWindowHost must not have loadText", loadTextCall)
    }

    @Test
    fun prepareSessionForEdit_acceptsInitialTextFromWindowLayer() {
        // #595 四/二：会话层不再持有 targetTexts 正文缓存 —
        // prepareSessionForEdit 的初始正文由窗口层 target 提供。
        val method = EditorSessionCoordinator::class.java.methods.firstOrNull {
            it.name == "prepareSessionForEdit"
        }
        assertNotNull(method)
        val params = method!!.parameterTypes
        assertTrue(
            "prepareSessionForEdit must accept (targetId, initialText, initialSelection, windowId)",
            params.size == 4 &&
                params[0] == String::class.java &&
                params[1] == String::class.java &&
                params[2] == Integer::class.java &&
                params[3] == String::class.java,
        )
    }

    @Test
    fun sessionBindInfo_carriesSnapshotForBothNewAndReused() {
        val field = SessionBindInfo::class.java.declaredFields.firstOrNull { it.name == "snapshot" }
        assertNotNull(
            "SessionBindInfo.snapshot must exist — new sessions also attach from the real snapshot",
            field,
        )
    }

    @Test
    fun bindSession_returnsBooleanForFailureDetection() {
        val method = com.xiwei.sujian.editor.v2.host.SujianEditorView::class.java.methods.firstOrNull {
            it.name == "bindSession" &&
            it.returnType == Boolean::class.javaPrimitiveType
        }
        assertTrue(
            "bindSession must return Boolean so bind failure can abort window attach (#595 三)",
            method != null,
        )
    }

    @Test
    fun attachSession_returnsBooleanForFailureDetection() {
        val method = com.xiwei.sujian.editor.v2.host.SujianEditorView::class.java.methods.firstOrNull {
            it.name == "attachSession" &&
            it.returnType == Boolean::class.javaPrimitiveType
        }
        assertTrue(
            "attachSession must return Boolean so bind failure can abort window attach (#595 三)",
            method != null,
        )
    }

    @Test
    fun coordinator_targetTextsParallelCache_removed() {
        val field = EditorSessionCoordinator::class.java.declaredFields.firstOrNull {
            it.name == "targetTexts"
        }
        assertTrue(
            "targetTexts parallel text cache must be removed from the session layer",
            field == null,
        )
        assertFalse("no targetTexts", field != null)
    }
}
