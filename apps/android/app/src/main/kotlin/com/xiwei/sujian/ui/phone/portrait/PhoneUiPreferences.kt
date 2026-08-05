package com.xiwei.sujian.ui.phone.portrait

import android.content.Context

class PhoneUiPreferences(context: Context) {
    private val prefs = context.applicationContext.getSharedPreferences(
        "sujian_phone_ui", Context.MODE_PRIVATE
    )

    fun getExpandedSettingsSections(): Set<String> {
        return prefs.getStringSet(KEY_EXPANDED_SECTIONS, null) ?: emptySet()
    }

    fun saveExpandedSettingsSections(sectionNames: Set<String>) {
        prefs.edit().putStringSet(KEY_EXPANDED_SECTIONS, sectionNames).apply()
    }

    fun getSelectedPhoneRoot(): String? {
        return prefs.getString(KEY_SELECTED_ROOT, null)
    }

    fun saveSelectedPhoneRoot(rootName: String) {
        prefs.edit().putString(KEY_SELECTED_ROOT, rootName).apply()
    }

    companion object {
        private const val KEY_EXPANDED_SECTIONS = "expanded_settings_sections"
        private const val KEY_SELECTED_ROOT = "selected_phone_root"
    }
}
