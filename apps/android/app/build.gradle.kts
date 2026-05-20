plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.xiwei.writerapp"
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
        applicationId = "com.xiwei.writerapp"
        minSdk = 24
        targetSdk = 34
        versionCode = 1
        versionName = "1.0"

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"

        ndk {
            // The official Android target is arm64-v8a.
            // x86_64 Android / emulators are not officially supported.
            abiFilters.addAll(listOf("arm64-v8a"))
        }
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

    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.5")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.1")
}
