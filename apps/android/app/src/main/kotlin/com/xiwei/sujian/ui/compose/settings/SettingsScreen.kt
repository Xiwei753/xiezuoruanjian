package com.xiwei.sujian.ui.compose.settings

import android.content.Intent
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import com.xiwei.sujian.ui.SettingsActivity

@Composable
fun SettingsScreen(
    onReturnFromSettings: (() -> Unit)? = null,
    modifier: Modifier = Modifier
) {
    val context = LocalContext.current

    DisposableEffect(Unit) {
        val intent = Intent(context, SettingsActivity::class.java)
        context.startActivity(intent)
        onDispose {
            onReturnFromSettings?.invoke()
        }
    }

    Box(modifier = modifier.fillMaxSize())
}
