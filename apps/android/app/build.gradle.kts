import java.io.ByteArrayOutputStream

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.kotlin.serialization)
    id("kotlin-parcelize")
    alias(libs.plugins.detekt)
    alias(libs.plugins.ktlint)
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

val ndkVersionValue =
    providers
        .gradleProperty("sujian.android.ndkVersion")
        .orElse("25.2.9519653")
        .get()

val requestedAndroidAbis: List<String> =
    providers
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
val abiSuffix =
    when {
        abisSorted.size > 1 -> "universal"
        abisSorted.size == 1 -> abisSorted.first()
        else -> throw GradleException(
            "No valid ABI specified. At least one ABI must be specified via -Psujian.android.abis",
        )
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
        // Issue #600：真实共享目录 + 普通文件路径（Rust std::fs / Git）路线要求
        // MANAGE_EXTERNAL_STORAGE（API 30 才有），基线直接设为 API 30。
        minSdk = 30
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
        // ABI 边界：jniLibs 打包路径只允许 generated/writer-native/<variant>。
        // main 源集默认的 src/main/jniLibs 必须显式清空，否则本地惰性存放的
        // 未跟踪 .so（gitignore 的 src/main/jniLibs/）会进入打包路径，造成
        // ABI 残留混入或陈旧原生库覆盖新产物。
        getByName("main") {
            jniLibs.setSrcDirs(emptyList<Any>())
        }
        getByName("noAi") {
            jniLibs.srcDirs(layout.buildDirectory.dir("generated/writer-native/noAiDebug"))
            kotlin.srcDirs(layout.buildDirectory.dir("generated/writer-uniffi/noAi/kotlin"))
        }
        getByName("ai") {
            jniLibs.srcDirs(layout.buildDirectory.dir("generated/writer-native/aiDebug"))
            kotlin.srcDirs(layout.buildDirectory.dir("generated/writer-uniffi/ai/kotlin"))
        }
        // AI flavor 专属测试源集：只放 ai 变体才需要的单元/设备测试。
        // noAi 变体不依赖这些测试，避免 noAi 误触 AI 路径（AGENTS.md 跨平台边界）。
        getByName("testAi") {
            java.srcDirs("src/testAi/kotlin")
        }
        getByName("androidTestAi") {
            java.srcDirs("src/androidTestAi/kotlin")
        }
    }

    buildFeatures {
        buildConfig = true
        compose = true
    }

    buildTypes {
        getByName("debug") {
            val keystorePath = System.getenv("WRITER_ANDROID_KEYSTORE_PATH")
            if (keystorePath != null && file(keystorePath).exists()) {
                signingConfig = signingConfigs.getByName("stable")
            }
        }
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
            val keystorePath = System.getenv("WRITER_ANDROID_KEYSTORE_PATH")
            if (keystorePath != null && file(keystorePath).exists()) {
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
                        val customName =
                            "sujian-android-$flavorName-$appVersionName-$appVersionCode-$gitCommitSha-$abiSuffix.apk"
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
    val variantNativeDir =
        if (nativeDirOverride != null) {
            file(nativeDirOverride)
        } else {
            layout.buildDirectory.dir("generated/writer-native/${variant.name}").get().asFile
        }

    val buildNativeTask =
        tasks.register(buildNativeTaskName) {
            group = "build"
            description = "Build Rust native libraries for $variantCapitalized"

            outputs.dir(variantNativeDir)

            doLast {
                val outDir = variantNativeDir

                for (abi in requestedAndroidAbis) {
                    val soFile = File(outDir, "$abi/libuniffi_writer_core.so")
                    if (soFile.exists()) {
                        logger.lifecycle(
                            "Rust native library for ABI '$abi' already present at " +
                                "${soFile.absolutePath}, skipping build.",
                        )
                    }
                }

                val allPresent =
                    requestedAndroidAbis.all { abi ->
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

                    val command =
                        mutableListOf(
                            scriptPath.absolutePath,
                            "--variant",
                            variantArg,
                            "--abis",
                            abiArg,
                            "--output",
                            outDir.absolutePath,
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
                                "Rust native library for ABI '$abi' not found at ${soFile.absolutePath} after build.",
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
    implementation(project(":core:designsystem"))
    implementation(project(":core:platform"))

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
}

android {
    lint {
        // GradleDependency/AndroidGradlePluginVersion 是版本提示，非代码质量问题；
        // 依赖升级应是有意决策，不由 lint 驱动。
        disable.addAll(listOf("MissingTranslation", "GradleDependency", "AndroidGradlePluginVersion"))
        // lint 无 baseline：UniFFI 生成的 NewApi 已通过 uniffi.toml
        // android_cleaner = true 从上游契约修复（生成代码改用 @RequiresApi(34)
        // 的 SystemCleaner + SDK_INT 运行时分支），不再需要冻结任何预存问题。
    }
}

// testAiDebugUnitTest 测试过滤：只运行 com.xiwei.sujian.ai.* 包下的测试。
// isFailOnNoMatchingTests = true 防止过滤被静默掏空（如 ai 源集被误删时构建仍绿）。
// 用 tasks.matching + configureEach 而非 tasks.named，因为 AGP 的 test task 按
// 变体惰性创建，matching 能在任务实际创建时再应用过滤，避免配置期任务不存在错误。
tasks.matching { it.name == "testAiDebugUnitTest" }.configureEach {
    (this as org.gradle.api.tasks.testing.Test).filter {
        includeTestsMatching("com.xiwei.sujian.ai.*")
        isFailOnNoMatchingTests = true
    }
}

detekt {
    config.setFrom(rootProject.files("config/detekt.yml"))
    buildUponDefaultConfig = true
    parallel = true
    ignoreFailures = false
    // 包含 Gradle Kotlin 脚本（.kts）的扫描（Issue #597）。
    source.setFrom(
        files("src/main/kotlin", "src/test/kotlin", "src/debug/kotlin", "src/release/kotlin")
            .asFileTree
            .matching { include("**/*.kt", "**/*.kts") },
    )
}

// ktlint 格式检查配置。
// 排除构建产物与 UniFFI 生成绑定：生成代码不可手改（AGENTS.md）。
ktlint {
    android = true
    ignoreFailures = false
    filter {
        exclude("**/build/**", "**/generated/**")
    }
}

// 排除构建产物与 UniFFI 生成绑定：生成代码不可手改（AGENTS.md），
// noAi/ai flavor 把 generated/writer-uniffi 加入了 kotlin.srcDirs，必须显式排除。
// Detekt task 继承自 SourceTask，exclude(vararg) 是标准 Gradle API。
tasks.withType<io.gitlab.arturbosch.detekt.Detekt>().configureEach {
    exclude("**/build/**", "**/generated/**", "uniffi/**")
}

// #597：架构约束不再通过 JUnit/Gradle 单元测试任务运行（正文六）。
// 分层规则已迁移为普通源码扫描：tools/check_android_architecture.py，
// 由 .github/workflows/static_analysis.yml 直接运行，不编译 Android App。
// 因此这里不再注册 testArchNoAiDebug 任务，也不再对 testNoAiDebugUnitTest
// 施加 arch 包 include/exclude 过滤。

// 生成绑定排除补充：flavor（noAi/ai）源集任务的 FileTree 以生成目录为根，
// 相对路径是 uniffi/writer_core/...，扩展 filter 的 **/build/** 模式匹配不到。
// 生成代码不可手改（AGENTS.md），按生成包路径精确排除。
tasks.withType<org.jlleitschuh.gradle.ktlint.tasks.BaseKtLintCheckTask>().configureEach {
    exclude("uniffi/**")
}
