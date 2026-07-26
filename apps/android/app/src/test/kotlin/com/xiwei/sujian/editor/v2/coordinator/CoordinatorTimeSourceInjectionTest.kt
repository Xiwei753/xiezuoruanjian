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
        val isLastParams = timeSourceParamIndex >= ctorWithTimeSource.parameterTypes.size - 2
        assertTrue(
            "AnimationTimeSource should be one of the last parameters (with default)",
            isLastParams
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
}
