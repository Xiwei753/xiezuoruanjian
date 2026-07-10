package com.xiwei.sujian.ui.compose.settings

import android.content.Intent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import com.xiwei.sujian.ui.SettingsActivity

@Composable
fun SettingsScreen(
    onReturnFromSettings: (() -> Unit)? = null,
    modifier: Modifier = Modifier
) {
    val context = LocalContext.current

    val launcher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.StartActivityForResult()
    ) {
        onReturnFromSettings?.invoke()
    }

    LaunchedEffect(Unit) {
        val intent = Intent(context, SettingsActivity::class.java)
        launcher.launch(intent)
    }

    Box(modifier = modifier.fillMaxSize())
}
