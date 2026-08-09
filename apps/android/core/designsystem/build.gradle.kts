plugins {
    alias(libs.plugins.android.library)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.detekt)
}

android {
    namespace = "com.xiwei.sujian.core.designsystem"
    compileSdk = 36

    defaultConfig {
        minSdk = 30
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
    buildFeatures {
        compose = true
    }
}

dependencies {
    val composeBom = platform(libs.compose.bom)
    api(composeBom)
    api(libs.compose.ui)
    api(libs.compose.material3)
    api(libs.compose.material3.adaptive)
    api(libs.compose.material3.adaptive.layout)
    api(libs.compose.material3.adaptive.navigation)

    implementation(libs.compose.foundation)
    implementation(libs.compose.material.icons.extended)
}

detekt {
    config.setFrom(rootProject.files("config/detekt.yml"))
    buildUponDefaultConfig = true
    parallel = true
    ignoreFailures = false
}

// 排除构建产物与 UniFFI 生成绑定：生成代码不可手改（AGENTS.md），
// noAi/ai flavor 把 generated/writer-uniffi 加入了 kotlin.srcDirs，必须显式排除。
// Detekt task 继承自 SourceTask，exclude(vararg) 是标准 Gradle API。
tasks.withType<io.gitlab.arturbosch.detekt.Detekt>().configureEach {
    exclude("**/build/**", "**/generated/**")
}
