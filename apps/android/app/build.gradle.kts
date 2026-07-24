import java.io.ByteArrayOutputStream

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
    id("kotlin-parcelize")
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

val ndkVersionValue = "25.2.9519653"

val requestedAndroidAbis: List<String> = providers
    .gradleProperty("sujian.android.abis")
    .orElse("arm64-v8a")
    .map { value ->
        value.split(',')
            .map(String::trim)
            .filter(String::isNotEmpty)
            .distinct()
    }
    .get()

val validAbis = setOf("arm64-v8a", "x86_64")
val invalidAbis = requestedAndroidAbis.filter { it !in validAbis }
require(invalidAbis.isEmpty()) {
    "Invalid Android ABI(s): $invalidAbis. Only ${validAbis.joinToString(", ")} are allowed."
}

val abisSorted = requestedAndroidAbis.sorted()
val abiSuffix = when {
    abisSorted.size > 1 -> "universal"
    abisSorted.size == 1 -> abisSorted.first()
    else -> throw GradleException("No valid ABI specified. At least one ABI must be specified via -Psujian.android.abis")
}

val nativeDir = providers
    .gradleProperty("sujian.android.nativeDir")
    .orElse(layout.buildDirectory.dir("generated/writer-native").map { it.asFile.absolutePath })

android {
    namespace = "com.xiwei.sujian"
    compileSdk = 36
    ndkVersion = ndkVersionValue

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
        targetSdk = 36
        versionCode = appVersionCode
        versionName = appVersionName

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"

        ndk {
            abiFilters.addAll(requestedAndroidAbis)
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

    sourceSets {
        getByName("main") {
            jniLibs.srcDirs(nativeDir)
        }
        getByName("noAi") {
            kotlin.srcDirs(layout.buildDirectory.dir("generated/writer-uniffi/noAi/kotlin"))
        }
        getByName("ai") {
            kotlin.srcDirs(layout.buildDirectory.dir("generated/writer-uniffi/ai/kotlin"))
        }
    }

    buildFeatures {
        buildConfig = true
        compose = true
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
        isCoreLibraryDesugaringEnabled = true
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
}

android.applicationVariants.all {
    val variant = this
    variant.outputs.all {
        val output = this
        if (output is com.android.build.gradle.api.ApkVariantOutput) {
            variant.packageApplicationProvider.configure {
                doLast {
                    val defaultApk = output.outputFile
                    if (defaultApk.exists()) {
                        val flavorName = variant.productFlavors.firstOrNull()?.name ?: variant.name
                        val customName = "sujian-android-${flavorName}-${appVersionName}-${appVersionCode}-${gitCommitSha}-${abiSuffix}.apk"
                        val destFile = File(defaultApk.parentFile, customName)
                        defaultApk.copyTo(destFile, overwrite = true)
                        defaultApk.delete()
                    }
                }
            }
        }
    }
}

tasks.register("buildWriterNative") {
    group = "build"
    description = "Build Rust native libraries for writer core (lifecycle task)"
}

android.applicationVariants.all {
    val variant = this
    val variantCapitalized = variant.name.replaceFirstChar { it.uppercase() }
    val buildNativeTaskName = "build${variantCapitalized}WriterNative"

    val buildNativeTask = tasks.register(buildNativeTaskName) {
        group = "build"
        description = "Build Rust native libraries for $variantCapitalized"

        val nativeOutputDir = file(nativeDir)
        outputs.dir(nativeOutputDir)

        doLast {
            val outDir = nativeOutputDir
            val variantArg = variant.name
            val abiArg = requestedAndroidAbis.joinToString(",")
            val featuresArg = if (variant.name.startsWith("ai")) "ai" else ""
            val scriptPath = file("${project.projectDir}/../../../tools/android/build_native.sh")

            if (!scriptPath.exists()) {
                throw GradleException("build_native.sh not found at ${scriptPath.absolutePath}")
            }

            val command = mutableListOf(
                scriptPath.absolutePath,
                "--variant", variantArg,
                "--abis", abiArg,
                "--output", outDir.absolutePath
            )
            if (featuresArg.isNotEmpty()) {
                command.addAll(listOf("--features", featuresArg))
            }

            exec {
                commandLine(command)
            }

            for (abi in requestedAndroidAbis) {
                val soFile = File(outDir, "$abi/libuniffi_writer_core.so")
                if (!soFile.exists()) {
                    throw GradleException(
                        "Rust native library for ABI '$abi' not found at ${soFile.absolutePath} after build."
                    )
                }
            }
        }
    }

    tasks.named("buildWriterNative").configure {
        dependsOn(buildNativeTask)
    }

    tasks.named("pre${variantCapitalized}Build").configure {
        dependsOn(buildNativeTask)
    }
}

dependencies {
    implementation(project(":core-designsystem"))
    implementation(project(":core-platform"))

    implementation(libs.androidx.core.ktx)

    implementation(libs.gson)

    implementation(libs.androidx.lifecycle.viewmodel.ktx)
    implementation(libs.androidx.lifecycle.runtime.ktx)
    implementation(libs.androidx.lifecycle.process)
    implementation(libs.androidx.activity.ktx)
    implementation(libs.androidx.work.runtime.ktx)

    val composeBom = platform(libs.compose.bom)
    implementation(composeBom)
    implementation(libs.compose.material3)
    implementation(libs.compose.material3.windowsizeclass)
    implementation(libs.compose.material3.adaptive)
    implementation(libs.compose.material3.adaptive.layout)
    implementation(libs.compose.material3.adaptive.navigation)
    implementation(libs.compose.material3.adaptive.navigation.suite)
    implementation(libs.compose.ui)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.compose.foundation)
    implementation(libs.compose.material.icons.extended)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)

    implementation(libs.navigation3.runtime)
    implementation(libs.navigation3.ui)

    implementation(libs.androidx.window)
    implementation(libs.androidx.window.core)

    coreLibraryDesugaring(libs.desugar)

    debugImplementation(libs.compose.ui.tooling)

    testImplementation(libs.junit)
    testImplementation(libs.robolectric)
    androidTestImplementation(libs.androidx.test.ext.junit)
    androidTestImplementation(libs.androidx.test.espresso)
}

android {
    testOptions {
        unitTests {
            isIncludeAndroidResources = true
        }
    }
}
dependencies {
    implementation(libs.jna)
    testImplementation(libs.jna)
}

android {
    lint {
        disable.addAll(listOf("MissingTranslation"))
        baseline = file("lint-baseline.xml")
    }
}
