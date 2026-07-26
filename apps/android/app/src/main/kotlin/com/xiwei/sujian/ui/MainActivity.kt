package com.xiwei.sujian.ui

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import com.xiwei.sujian.ui.compose.SujianApp

class MainActivity : ComponentActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        enableEdgeToEdge()

        val initialDestination = intent?.getStringExtra("navigateTo")

        setContent {
            SujianApp(initialDestination = initialDestination)
        }
    }
}
