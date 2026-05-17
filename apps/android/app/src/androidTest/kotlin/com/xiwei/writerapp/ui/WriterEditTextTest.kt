package com.xiwei.writerapp.ui

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertNotNull
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class WriterEditTextTest {

    @Test
    fun testWriterEditTextInstantiation() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        // Should not crash during initialization
        val editText = WriterEditText(appContext)
        assertNotNull(editText)
    }
}
