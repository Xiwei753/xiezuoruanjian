package com.xiwei.sujian.ui

import androidx.activity.ComponentActivity
import android.content.pm.ActivityInfo

class OrientationTestActivity : ComponentActivity() {
    var lastConfigurationChangeOrientation: Int = -1
        private set

    override fun onConfigurationChanged(newConfig: android.content.res.Configuration) {
        super.onConfigurationChanged(newConfig)
        lastConfigurationChangeOrientation = newConfig.orientation
    }
}
