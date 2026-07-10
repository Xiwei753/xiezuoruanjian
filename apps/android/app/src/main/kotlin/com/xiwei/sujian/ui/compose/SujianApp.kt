package com.xiwei.sujian.ui.compose

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Scaffold
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import com.xiwei.sujian.ui.compose.navigation.SujianNavigationSuite
import com.xiwei.sujian.ui.compose.theme.SujianTheme

@Composable
fun SujianApp() {
    SujianTheme {
        Scaffold(modifier = Modifier.fillMaxSize()) { innerPadding ->
            SujianNavigationSuite(
                modifier = Modifier.padding(innerPadding)
            )
        }
    }
}
