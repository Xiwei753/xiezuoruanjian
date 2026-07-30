import java.io.ByteArrayOutputStream

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.kotlin.serialization)
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

val ndkVersionValue = providers
    .gradleProperty("sujian.android.ndkVersion")
    .orElse("25.2.9519653")
    .get()

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
        getByName("noAi") {
            jniLibs.srcDirs(layout.buildDirectory.dir("generated/writer-native/noAiDebug"))
            kotlin.srcDirs(layout.buildDirectory.dir("generated/writer-uniffi/noAi/kotlin"))
        }
        getByName("ai") {
            jniLibs.srcDirs(layout.buildDirectory.dir("generated/writer-native/aiDebug"))
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

    val nativeDirOverride = providers.gradleProperty("sujian.android.nativeDir").orNull
    val variantNativeDir = if (nativeDirOverride != null) {
        file(nativeDirOverride)
    } else {
        layout.buildDirectory.dir("generated/writer-native/${variant.name}").get().asFile
    }

    val buildNativeTask = tasks.register(buildNativeTaskName) {
        group = "build"
        description = "Build Rust native libraries for $variantCapitalized"

        outputs.dir(variantNativeDir)

        doLast {
            val outDir = variantNativeDir

            for (abi in requestedAndroidAbis) {
                val soFile = File(outDir, "$abi/libuniffi_writer_core.so")
                if (soFile.exists()) {
                    logger.lifecycle("Rust native library for ABI '$abi' already present at ${soFile.absolutePath}, skipping build.")
                }
            }

            val allPresent = requestedAndroidAbis.all { abi ->
                File(outDir, "$abi/libuniffi_writer_core.so").exists()
            }

            if (allPresent) {
                logger.lifecycle("All requested ABI native libraries present, skipping Rust build.")
            } else {
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
    implementation(libs.kotlinx.serialization.core)

    implementation(libs.androidx.window)
    implementation(libs.androidx.window.core)
    implementation(libs.androidx.datastore.preferences)

    coreLibraryDesugaring(libs.desugar)

    debugImplementation(libs.compose.ui.tooling)

    androidTestImplementation(platform(libs.compose.bom))
    androidTestImplementation(libs.compose.ui.test.junit4)
    debugImplementation(libs.compose.ui.test.manifest)

    testImplementation(libs.junit)
    testImplementation(libs.robolectric)
    testImplementation(libs.androidx.test.core)
    testImplementation(libs.kotlinx.coroutines.test)
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
    implementation("${libs.jna.get().group}:${libs.jna.get().name}:${libs.jna.get().version}@aar")
    testImplementation(libs.jna)
    androidTestImplementation(libs.jna)
    androidTestImplementation("androidx.test:runner:1.6.2")
}

android {
    lint {
        disable.addAll(listOf("MissingTranslation"))
        baseline = file("lint-baseline.xml")
    }
}

// Static verification of test classification.
// All concrete androidTest classes must belong to exactly one category.
// Does NOT read or apply any dynamic filter — that is done at the
// AndroidJUnitRunner level via the standard Gradle property:
//   -Pandroid.testInstrumentationRunnerArguments.annotation=<fully.qualified.AnnotationClass>
tasks.register("testDiscoveryCheck") {
    doLast {
        val androidTestDir = project.projectDir.resolve("src/androidTest")
        if (!androidTestDir.exists()) {
            throw GradleException("androidTest source directory not found: $androidTestDir")
        }

        data class TestClass(val path: String, val annotations: List<String>)

        val testFiles = fileTree(androidTestDir) {
            include("**/*Test.kt", "**/*Test.java")
        }.files

        val validAnnotations = setOf(
            "com.xiwei.sujian.support.SujianSmallTest",
            "com.xiwei.sujian.support.SujianMediumTest",
            "com.xiwei.sujian.support.SujianLargeTest"
        )
        val annotationSimpleNames = validAnnotations.map { it.substringAfterLast('.') }

        val testClasses = mutableListOf<TestClass>()

        for (file in testFiles) {
            val relativePath = file.relativeTo(androidTestDir).path
            val content = file.readText()

            // Skip abstract / open base classes
            if (content.contains(Regex("\\babstract\\s+class\\b")) ||
                content.contains(Regex("\\bopen\\s+class\\b"))
            ) {
                println("SKIP (abstract/open): $relativePath")
                continue
            }

            val foundAnnotations = annotationSimpleNames.filter { name ->
                Regex("@${name}\\b").containsMatchIn(content)
            }

            testClasses.add(TestClass(relativePath, foundAnnotations))
        }

        val errors = mutableListOf<String>()

        for (tc in testClasses) {
            when {
                tc.annotations.isEmpty() -> {
                    errors.add("MISSING CATEGORY: ${tc.path} has no Small/Medium/Large annotation")
                }
                tc.annotations.size > 1 -> {
                    errors.add("DUPLICATE CATEGORY: ${tc.path} has multiple: ${tc.annotations}")
                }
            }
        }

        // Verify workflow contains all three standard runner argument strings
        val workflowFile = project.rootProject.projectDir.parentFile.parentFile
            .resolve(".github/workflows/android_debug_build.yml")
        if (workflowFile.exists()) {
            val workflowContent = workflowFile.readText()
            val annotationToVar = mapOf(
                "com.xiwei.sujian.support.SujianSmallTest" to "SMALL_ANNOTATION",
                "com.xiwei.sujian.support.SujianMediumTest" to "MEDIUM_ANNOTATION",
                "com.xiwei.sujian.support.SujianLargeTest" to "LARGE_ANNOTATION"
            )
            for ((_, varName) in annotationToVar) {
                val expectedPattern = "android.testInstrumentationRunnerArguments.annotation=\$${varName}"
                if (!workflowContent.contains(expectedPattern)) {
                    errors.add("WORKFLOW MISSING: $expectedPattern not found in android_debug_build.yml")
                }
            }
        } else {
            errors.add("WORKFLOW NOT FOUND: .github/workflows/android_debug_build.yml")
        }

        if (errors.isNotEmpty()) {
            throw GradleException(
                "Test classification integrity check FAILED:\n  " +
                errors.joinToString("\n  ")
            )
        }

        val totalTests = testClasses.size
        val smallCount = testClasses.count { "SujianSmallTest" in it.annotations }
        val mediumCount = testClasses.count { "SujianMediumTest" in it.annotations }
        val largeCount = testClasses.count { "SujianLargeTest" in it.annotations }
        println("testDiscoveryCheck PASSED: $totalTests concrete test classes ($smallCount small, $mediumCount medium, $largeCount large)")
    }
}

tasks.register("printTestCategories") {
    doLast {
        println("""
Sujian Android Test Categories:
  All tests:        ./gradlew connectedNoAiDebugAndroidTest
  Small tests:      ./gradlew connectedNoAiDebugAndroidTest -Pandroid.testInstrumentationRunnerArguments.annotation=com.xiwei.sujian.support.SujianSmallTest
  Medium tests:     ./gradlew connectedNoAiDebugAndroidTest -Pandroid.testInstrumentationRunnerArguments.annotation=com.xiwei.sujian.support.SujianMediumTest
  Large tests:      ./gradlew connectedNoAiDebugAndroidTest -Pandroid.testInstrumentationRunnerArguments.annotation=com.xiwei.sujian.support.SujianLargeTest
  JVM unit tests:   ./gradlew testNoAiDebugUnitTest
  All verification: ./gradlew connectedNoAiDebugAndroidTest
        """.trimIndent())
    }
}
