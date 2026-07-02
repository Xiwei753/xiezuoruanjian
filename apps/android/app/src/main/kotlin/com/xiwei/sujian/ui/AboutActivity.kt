package com.xiwei.sujian.ui

import android.os.Bundle
import android.text.method.LinkMovementMethod
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import com.google.android.material.appbar.MaterialToolbar
import com.xiwei.sujian.R

class AboutActivity : AppCompatActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_about)

        val toolbar: MaterialToolbar = findViewById(R.id.toolbar)
        setSupportActionBar(toolbar)
        toolbar.setNavigationOnClickListener { finish() }
        toolbar.title = getString(R.string.about_title)

        val tvAppName: TextView = findViewById(R.id.tvAppName)
        val tvAuthor: TextView = findViewById(R.id.tvAuthor)
        val tvGitHub: TextView = findViewById(R.id.tvGitHub)
        val tvLicense: TextView = findViewById(R.id.tvLicense)
        val tvVersionDetail: TextView = findViewById(R.id.tvVersionDetail)

        tvAppName.text = getString(R.string.about_app_name)
        tvAuthor.text = getString(R.string.about_author)
        tvGitHub.text = getString(R.string.about_github)
        tvLicense.text = getString(R.string.about_license)

        // Make links clickable
        tvGitHub.movementMethod = LinkMovementMethod.getInstance()

        // Version info
        val appVersion = try {
            val pi = packageManager.getPackageInfo(packageName, 0)
            pi.versionName ?: "unknown"
        } catch (e: Exception) { "unknown" }

        val coreVersion = try {
            // core_version() API not yet available; show "dev" as fallback
            "dev"
        } catch (e: Exception) { "unknown" }

        tvVersionDetail.text = getString(R.string.about_version_detail, appVersion, coreVersion, "Android")
    }
}
