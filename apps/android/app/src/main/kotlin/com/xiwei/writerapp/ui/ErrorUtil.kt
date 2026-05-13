package com.xiwei.writerapp.ui

import android.content.Context
import android.widget.Toast
import com.xiwei.writerapp.data.RepositoryException

object ErrorUtil {
    fun <T> safeRun(context: Context, fallback: T, action: () -> T): T {
        return try {
            action()
        } catch (e: RepositoryException) {
            e.printStackTrace()
            Toast.makeText(context, e.message, Toast.LENGTH_LONG).show()
            fallback
        }
    }

    fun safeRun(context: Context, action: () -> Unit) {
        try {
            action()
        } catch (e: RepositoryException) {
            e.printStackTrace()
            Toast.makeText(context, e.message, Toast.LENGTH_LONG).show()
        }
    }
}
