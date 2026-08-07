package com.xiwei.sujian.data.starmap

import com.xiwei.sujian.model.StarMapMotionPolicyData
import org.junit.Assert.*
import org.junit.Test
import uniffi.writer_core.StarMapMotionPolicyDto

class StarMapMotionPolicyMapperTest {
    @Test
    fun dto_toModel_preservesAllFields() {
        val dto =
            StarMapMotionPolicyDto(
                enabled = true,
                idleWobbleEnabled = false,
                idleAmplitudeVp = 3.5f,
                idlePeriodMs = 5000u,
                dragLiftScale = 1.1f,
                dragShadowBoost = 10.0f,
                settleDurationMs = 300u,
                reduceMotion = true,
            )
        val model = dto.toModel()
        assertEquals(StarMapMotionPolicyData::class, model::class)
        assertTrue(model.enabled)
        assertFalse(model.idleWobbleEnabled)
        assertEquals(3.5f, model.idleAmplitudeVp, 0.001f)
        assertEquals(5000, model.idlePeriodMs)
        assertEquals(1.1f, model.dragLiftScale, 0.001f)
        assertEquals(10.0f, model.dragShadowBoost, 0.001f)
        assertEquals(300, model.settleDurationMs)
        assertTrue(model.reduceMotion)
    }

    @Test
    fun dto_toModel_defaultValues() {
        val dto =
            StarMapMotionPolicyDto(
                enabled = true,
                idleWobbleEnabled = true,
                idleAmplitudeVp = 2.0f,
                idlePeriodMs = 4200u,
                dragLiftScale = 1.04f,
                dragShadowBoost = 8.0f,
                settleDurationMs = 220u,
                reduceMotion = false,
            )
        val model = dto.toModel()
        val expected = StarMapMotionPolicyData()
        assertEquals(expected.enabled, model.enabled)
        assertEquals(expected.idleWobbleEnabled, model.idleWobbleEnabled)
        assertEquals(expected.idleAmplitudeVp, model.idleAmplitudeVp, 0.001f)
        assertEquals(expected.idlePeriodMs, model.idlePeriodMs)
        assertEquals(expected.dragLiftScale, model.dragLiftScale, 0.001f)
        assertEquals(expected.dragShadowBoost, model.dragShadowBoost, 0.001f)
        assertEquals(expected.settleDurationMs, model.settleDurationMs)
        assertEquals(expected.reduceMotion, model.reduceMotion)
    }
}
