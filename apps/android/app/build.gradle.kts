import java.io.ByteArrayOutputStream

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

fun queryGitCommitCount(): Int {
    return try {
        val stdout = ByteArrayOutputStream()
        project.exec {
            commandLine("git", "rev-list", "--count", "HEAD")
            standardOutput = stdout
        }
        stdout.toString().trim().toInt()
    } catch (e: Exception) {
        1
    }
}

fun queryGitCommitShortSha(): String {
    return try {
        val stdout = ByteArrayOutputStream()
        project.exec {
            commandLine("git", "rev-parse", "--short", "HEAD")
            standardOutput = stdout
        }
        stdout.toString().trim()
    } catch (e: Exception) {
        "unknown"
    }
}

val gitCommitCount = queryGitCommitCount()
val gitCommitSha = queryGitCommitShortSha()
val appVersionCode = gitCommitCount
val appVersionName = "0.1.1"

android {
    namespace = "com.xiwei.sujian"
    compileSdk = 34

    signingConfigs {
        create("stable") {
            val keystorePath = System.getenv("WRITER_ANDROID_KEYSTORE_PATH")
            if (keystorePath != null && file(keystorePath).exists()) {
                storeFile = file(keystorePath)
                storePassword = System.getenv("WRITER_ANDROID_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("WRITER_ANDROID_KEY_ALIAS")
                keyPassword = System.getenv("WRITER_ANDROID_KEY_PASSWORD")
            }
        }
    }

    defaultConfig {
        applicationId = "com.xiwei.sujian"
        minSdk = 24
        targetSdk = 34
        versionCode = appVersionCode
        versionName = appVersionName

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"

        ndk {
            // The official Android target is arm64-v8a.
            // x86_64 Android / emulators are not officially supported.
            abiFilters.addAll(listOf("arm64-v8a"))
        }
    }

    flavorDimensions += "ai"
    productFlavors {
        create("noAi") {
            dimension = "ai"
            isDefault = true
        }
        create("ai") {
            applicationIdSuffix = ".ai"
            versionNameSuffix = "-ai"
            dimension = "ai"
        }
    }

    buildFeatures {
        buildConfig = true
    }

    buildTypes {
        getByName("debug") {
            if (System.getenv("WRITER_ANDROID_KEYSTORE_PATH") != null && file(System.getenv("WRITER_ANDROID_KEYSTORE_PATH")).exists()) {
                signingConfig = signingConfigs.getByName("stable")
            }
        }
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
            if (System.getenv("WRITER_ANDROID_KEYSTORE_PATH") != null && file(System.getenv("WRITER_ANDROID_KEYSTORE_PATH")).exists()) {
                signingConfig = signingConfigs.getByName("stable")
            }
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
    kotlinOptions {
        jvmTarget = "1.8"
    }

    applicationVariants.all {
        val variant = this
        variant.outputs.all {
            val output = this
            if (output is com.android.build.gradle.api.ApkVariantOutput) {
                val abi = output.filters.find { it.filterType == "ABI" }?.identifier ?: "all"
                variant.packageApplicationProvider.configure {
                    doLast {
                        val defaultApk = output.outputFile
                        if (defaultApk.exists()) {
                            val flavorName = variant.productFlavors.firstOrNull()?.name ?: variant.name
                            val customName = "sujian-android-${flavorName}-${appVersionName}-${appVersionCode}-${gitCommitSha}-${abi}.apk"
                            val destFile = File(defaultApk.parentFile, customName)
                            defaultApk.copyTo(destFile, overwrite = true)
                            println("Successfully copied custom-named APK to ${destFile.absolutePath}")
                        }
                    }
                }
            }
        }
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.12.0")
    implementation("androidx.appcompat:appcompat:1.6.1")
    implementation("com.google.android.material:material:1.11.0")
    implementation("androidx.constraintlayout:constraintlayout:2.1.4")
    implementation("com.google.code.gson:gson:2.10.1")

    // ViewModel & Lifecycle
    implementation("androidx.lifecycle:lifecycle-viewmodel-ktx:2.7.0")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.7.0")
    implementation("androidx.lifecycle:lifecycle-process:2.7.0")
    implementation("androidx.activity:activity-ktx:1.8.2")
    implementation("androidx.fragment:fragment-ktx:1.6.2")
    implementation("androidx.work:work-runtime-ktx:2.9.1")

    testImplementation("junit:junit:4.13.2")
    testImplementation("org.robolectric:robolectric:4.11.1")
    androidTestImplementation("androidx.test.ext:junit:1.1.5")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.1")
}

android {
    testOptions {
        unitTests {
            isIncludeAndroidResources = true
        }
    }
}
dependencies {
    implementation("net.java.dev.jna:jna:5.13.0@aar")
}

android {
    lint {
        disable.addAll(listOf("MissingTranslation", "SetTextI18n", "HardcodedText", "NewApi"))
    }
}
