package com.xiwei.sujian.ui.compose

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import com.xiwei.sujian.ui.compose.navigation.SujianNavigationSuite

@Composable
fun SujianApp() {
    Scaffold(modifier = Modifier.fillMaxSize()) { innerPadding ->
        SujianNavigationSuite(
            modifier = Modifier.padding(innerPadding)
        )
    }
}
