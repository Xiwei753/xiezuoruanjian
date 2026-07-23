package com.xiwei.sujian.platform.aosp

import android.content.Context
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.callbackFlow

class AospSensorAdapter(private val context: Context) {

    private val sensorManager by lazy {
        context.getSystemService(Context.SENSOR_SERVICE) as? SensorManager
    }

    fun hasSensor(type: Int): Boolean {
        return sensorManager?.getDefaultSensor(type) != null
    }

    fun sensorEvents(sensorType: Int) = callbackFlow {
        val manager = sensorManager ?: run { close(); return@callbackFlow }
        val sensor = manager.getDefaultSensor(sensorType) ?: run { close(); return@callbackFlow }

        val listener = object : SensorEventListener {
            override fun onSensorChanged(event: SensorEvent) {
                trySend(event.values.toList())
            }
            override fun onAccuracyChanged(sensor: Sensor?, accuracy: Int) {}
        }

        manager.registerListener(listener, sensor, SensorManager.SENSOR_DELAY_NORMAL)
        awaitClose { manager.unregisterListener(listener) }
    }
}
