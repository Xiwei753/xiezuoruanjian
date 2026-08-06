package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.lang.reflect.Method

/**
 * #595 三：AndroidView factory 与 beginEdit 的 View 绑定时序契约测试。
 *
 * 验证 beginEdit 不直接创建 View（避免与 AndroidView.factory 用不同 Context 创建
 * 两个 View 实例，导致显示的 View 没有 session 绑定）。session 绑定通过
 * pendingViewBind 延迟到 attachView（在 AndroidView.factory 内调用）时执行。
 */
class ViewBindTimingContractTest {

    @Test
    fun createWindowView_existsOnEditorWindowHost() {
        val method: Method? = EditorWindowHost::class.java.methods.firstOrNull {
            it.name == "createWindowView" &&
            it.parameterTypes.size == 1 &&
            it.parameterTypes[0] == android.content.Context::class.java
        }
        assertNotNull(
            "EditorWindowHost must have createWindowView(Context) for AndroidView.factory",
            method,
        )
    }

    @Test
    fun attachView_existsOnEditorWindowHost() {
        val method: Method? = EditorWindowHost::class.java.methods.firstOrNull {
            it.name == "attachView" &&
            it.parameterTypes.size == 3 &&
            it.parameterTypes[0] == String::class.java &&
            it.parameterTypes[1] == String::class.java &&
            it.parameterTypes[2] == com.xiwei.sujian.editor.v2.host.SujianEditorView::class.java
        }
        assertNotNull(
            "EditorWindowHost must have attachView(windowId, targetId, view) for AndroidView.factory post-creation",
            method,
        )
    }

    @Test
    fun detachView_existsOnEditorWindowHost() {
        val method: Method? = EditorWindowHost::class.java.methods.firstOrNull {
            it.name == "detachView" &&
            it.parameterTypes.size == 3
        }
        assertNotNull(
            "EditorWindowHost must have detachView(windowId, targetId, view) for AndroidView.onRelease",
            method,
        )
    }

    @Test
    fun pendingViewBind_fieldExists() {
        val field = EditorWindowHost::class.java.declaredFields.firstOrNull {
            it.name == "pendingViewBind"
        }
        assertNotNull(
            "EditorWindowHost must have pendingViewBind field to defer session binding " +
            "until AndroidView.factory creates the View with Compose Context",
            field,
        )
    }

    @Test
    fun obtainSharedEditorView_doesNotExist() {
        val method: Method? = EditorWindowHost::class.java.methods.firstOrNull {
            it.name == "obtainSharedEditorView"
        }
        assertTrue(
            "EditorWindowHost must NOT have obtainSharedEditorView — " +
            "beginEdit must not create a View with Host Context; " +
            "View creation is solely AndroidView.factory's responsibility",
            method == null,
        )
    }

    @Test
    fun getOrCreateEditorView_doesNotExistAsPublicMethod() {
        val method: Method? = EditorWindowHost::class.java.methods.firstOrNull {
            it.name == "getOrCreateEditorView"
        }
        assertTrue(
            "getOrCreateEditorView must not be accessible — " +
            "it created a View with Host Context that AndroidView.factory would replace",
            method == null,
        )
    }

    @Test
    fun performViewBind_existsAsPrivateMethod() {
        val method: Method? = EditorWindowHost::class.java.declaredMethods.firstOrNull {
            it.name == "performViewBind"
        }
        assertNotNull(
            "EditorWindowHost must have performViewBind private method " +
            "shared by beginEdit (View exists) and attachView (pending bind)",
            method,
        )
    }
}
