package com.xiwei.sujian.editor.v2.coordinator

import com.xiwei.sujian.editor.v2.visual.AnimationTimeSource
import com.xiwei.sujian.editor.v2.visual.ChoreographerAnimationTimeSource
import com.xiwei.sujian.editor.v2.visual.ManualAnimationTimeSource
import com.xiwei.sujian.editor.v2.visual.TransactionIdSource
import org.junit.Assert.*
import org.junit.Test

class CoordinatorTimeSourceInjectionTest {

    @Test
    fun coordinatorConstructorAcceptsAnimationTimeSource() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator")
        val constructors = clazz.constructors
        val matchingConstructor = constructors.firstOrNull { ctor ->
            val paramTypes = ctor.parameterTypes.map { it.name }
            paramTypes.contains("com.xiwei.sujian.editor.v2.visual.AnimationTimeSource")
        }
        assertNotNull(
            "AnimatedTextEditorCoordinator should have a constructor accepting AnimationTimeSource",
            matchingConstructor
        )
    }

    @Test
    fun coordinatorConstructorAcceptsTransactionIdSource() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator")
        val constructors = clazz.constructors
        val matchingConstructor = constructors.firstOrNull { ctor ->
            val paramTypes = ctor.parameterTypes.map { it.name }
            paramTypes.contains("com.xiwei.sujian.editor.v2.visual.TransactionIdSource")
        }
        assertNotNull(
            "AnimatedTextEditorCoordinator should have a constructor accepting TransactionIdSource",
            matchingConstructor
        )
    }

    @Test
    fun coordinatorConstructorTimeSourceHasDefault() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator")
        val constructors = clazz.constructors
        val ctorWithTimeSource = constructors.firstOrNull { ctor ->
            val paramTypes = ctor.parameterTypes.map { it.name }
            paramTypes.contains("com.xiwei.sujian.editor.v2.visual.AnimationTimeSource")
        } ?: return
        val timeSourceParamIndex = ctorWithTimeSource.parameterTypes.map { it.name }
            .indexOf("com.xiwei.sujian.editor.v2.visual.AnimationTimeSource")
        val isLateParam = timeSourceParamIndex >= ctorWithTimeSource.parameterTypes.size - 4
        assertTrue(
            "AnimationTimeSource should be a late parameter (with default), index $timeSourceParamIndex of ${ctorWithTimeSource.parameterTypes.size}",
            isLateParam
        )
    }

    @Test
    fun manualTimeSourceImplementsAnimationTimeSource() {
        val manual = ManualAnimationTimeSource()
        assertTrue(
            "ManualAnimationTimeSource should implement AnimationTimeSource",
            manual is AnimationTimeSource
        )
    }

    @Test
    fun choreographerTimeSourceImplementsAnimationTimeSource() {
        val choreographer = ChoreographerAnimationTimeSource()
        assertTrue(
            "ChoreographerAnimationTimeSource should implement AnimationTimeSource",
            choreographer is AnimationTimeSource
        )
    }

    @Test
    fun manualTimeSourceStartsAtZero() {
        val source = ManualAnimationTimeSource()
        assertEquals(0L, source.nowNanos())
    }

    @Test
    fun transactionIdSourceProducesUniqueIds() {
        val source = TransactionIdSource()
        val ids = (1..10).map { source.nextId() }.toSet()
        assertEquals(10, ids.size)
    }

    @Test
    fun coordinatorConstructorAcceptsFrameClock() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator")
        val constructors = clazz.constructors
        val matchingConstructor = constructors.firstOrNull { ctor ->
            val paramTypes = ctor.parameterTypes.map { it.name }
            paramTypes.contains("com.xiwei.sujian.editor.v2.coordinator.WindowDisplayFrameClock")
        }
        assertNotNull(
            "AnimatedTextEditorCoordinator should have a constructor accepting WindowDisplayFrameClock",
            matchingConstructor
        )
    }

    @Test
    fun coordinatorExposesWindowFrameClock() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator")
        val getter = clazz.getDeclaredMethod("getWindowFrameClock")
        assertNotNull(
            "AnimatedTextEditorCoordinator should expose windowFrameClock getter",
            getter
        )
        assertEquals(
            "windowFrameClock getter should return WindowDisplayFrameClock",
            "com.xiwei.sujian.editor.v2.coordinator.WindowDisplayFrameClock",
            getter.returnType.name
        )
    }

    @Test
    fun coordinatorHasObtainSharedEditorViewMethod() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator")
        val method = clazz.getDeclaredMethod("obtainSharedEditorView")
        assertNotNull(
            "AnimatedTextEditorCoordinator should have obtainSharedEditorView() method",
            method
        )
        assertEquals(
            "obtainSharedEditorView should return SujianEditorView",
            "com.xiwei.sujian.editor.v2.host.SujianEditorView",
            method.returnType.name
        )
    }

    @Test
    fun manualFrameClockImplementsFrameCallbackPoster() {
        val manualClock = WindowDisplayFrameClock.ManualFrameClock()
        assertTrue(
            "ManualFrameClock should implement FrameCallbackPoster",
            manualClock is WindowDisplayFrameClock.FrameCallbackPoster
        )
    }

    @Test
    fun manualFrameClockHasDispatchFrameMethod() {
        val manualClock = WindowDisplayFrameClock.ManualFrameClock()
        val method = manualClock.javaClass.getDeclaredMethod("dispatchFrame", Long::class.javaPrimitiveType)
        assertNotNull(
            "ManualFrameClock should have dispatchFrame(Long) method",
            method
        )
    }

    @Test
    fun manualFrameClockHasHasPendingFrameMethod() {
        val manualClock = WindowDisplayFrameClock.ManualFrameClock()
        val method = manualClock.javaClass.getDeclaredMethod("hasPendingFrame")
        assertNotNull(
            "ManualFrameClock should have hasPendingFrame() method",
            method
        )
        assertFalse("No pending frame initially", manualClock.hasPendingFrame())
    }
}
