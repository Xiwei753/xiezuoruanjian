package com.xiwei.sujian.editor.v2.projection

import org.junit.Test

class TargetReadonlyProjectionContractTest {

    @Test
    fun targetReadonlyProjectionClassExists() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.projection.TargetReadonlyProjection")
        assert(clazz.declaredMethods.any { it.name == "updateFromSnapshot" })
        assert(clazz.declaredMethods.any { it.name == "setSecretMasked" })
        assert(clazz.declaredMethods.any { it.name == "setSearchHighlights" })
        assert(clazz.declaredMethods.any { it.name == "setSelection" })
        assert(clazz.declaredMethods.any { it.name == "getSearchHighlightsUtf16" })
        assert(clazz.declaredMethods.any { it.name == "getSelectionStartUtf16" })
        assert(clazz.declaredMethods.any { it.name == "getSelectionEndUtf16" })
        assert(clazz.declaredMethods.any { it.name == "getProjection" })
    }
}
