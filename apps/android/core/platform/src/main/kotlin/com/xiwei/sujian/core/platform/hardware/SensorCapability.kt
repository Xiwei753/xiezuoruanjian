package com.xiwei.sujian.core.platform.hardware

import android.content.Context
import android.hardware.Sensor
import android.hardware.SensorManager

data class SensorCapability(
    val hasGyroscope: Boolean = false,
    val hasAccelerometer: Boolean = false,
    val hasMagnetometer: Boolean = false,
    val hasProximity: Boolean = false,
    val hasLight: Boolean = false,
)

fun detectSensorCapabilities(context: Context): SensorCapability {
    val sensorManager = context.getSystemService(Context.SENSOR_SERVICE) as? SensorManager
    return SensorCapability(
        hasGyroscope = sensorManager?.getDefaultSensor(Sensor.TYPE_GYROSCOPE) != null,
        hasAccelerometer = sensorManager?.getDefaultSensor(Sensor.TYPE_ACCELEROMETER) != null,
        hasMagnetometer = sensorManager?.getDefaultSensor(Sensor.TYPE_MAGNETIC_FIELD) != null,
        hasProximity = sensorManager?.getDefaultSensor(Sensor.TYPE_PROXIMITY) != null,
        hasLight = sensorManager?.getDefaultSensor(Sensor.TYPE_LIGHT) != null,
    )
}
