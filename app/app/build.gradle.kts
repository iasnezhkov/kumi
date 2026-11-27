// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
import java.io.ByteArrayOutputStream
import java.io.File
import javax.inject.Inject
import org.gradle.api.DefaultTask
import org.gradle.api.file.ConfigurableFileCollection
import org.gradle.api.provider.Property
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.InputFiles
import org.gradle.api.tasks.Internal
import org.gradle.process.ExecOperations
import org.gradle.api.file.DirectoryProperty
import org.gradle.api.tasks.OutputDirectory
import org.gradle.api.tasks.PathSensitive
import org.gradle.api.tasks.PathSensitivity
import org.gradle.api.tasks.TaskAction

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

abstract class CargoBuildKumi @Inject constructor(private val exec: ExecOperations) : DefaultTask() {
    @get:InputFiles
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val sources: ConfigurableFileCollection

    @get:Internal
    abstract val workspace: DirectoryProperty

    @get:Input
    abstract val linker: Property<String>

    @get:Input
    abstract val rustup: Property<String>

    @get:OutputDirectory
    abstract val outputDir: DirectoryProperty

    @TaskAction
    fun build() {
        val root = workspace.get().asFile
        val resolvedCargo = ByteArrayOutputStream().also { out ->
            exec.exec {
                workingDir = root
                commandLine(rustup.get(), "which", "cargo")
                standardOutput = out
                isIgnoreExitValue = true
            }
        }.toString().trim()
        val resolvedRustc = ByteArrayOutputStream().also { out ->
            exec.exec {
                workingDir = root
                commandLine(rustup.get(), "which", "rustc")
                standardOutput = out
                isIgnoreExitValue = true
            }
        }.toString().trim()
        val cargoCmd = if (resolvedCargo.isNotEmpty() && File(resolvedCargo).canExecute()) {
            listOf(resolvedCargo)
        } else {
            listOf(rustup.get(), "run", "stable", "cargo")
        }
        val cargoHome = System.getenv("CARGO_HOME")?.takeIf { it.isNotBlank() } ?: File(System.getProperty("user.home"), ".cargo").path
        val existingFlags = System.getenv("CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS").orEmpty().trim()
        val remapFlags = "--remap-path-prefix=$cargoHome=/cargo --remap-path-prefix=${root.absolutePath}=/kumi"
        val rustflags = if (existingFlags.isEmpty()) remapFlags else "$existingFlags $remapFlags"
        exec.exec {
            workingDir = root
            commandLine(cargoCmd + listOf("build", "--locked", "-p", "kumi-jni", "--no-default-features", "--profile", "android", "--target", "aarch64-linux-android"))
            if (resolvedRustc.isNotEmpty() && File(resolvedRustc).canExecute()) {
                environment("RUSTC", resolvedRustc)
            }
            environment("CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER", linker.get())
            environment("CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS", rustflags)
        }
        val targetDir = System.getenv("CARGO_TARGET_DIR")?.takeIf { it.isNotBlank() }?.let(::File) ?: root.resolve("target")
        val out = outputDir.get().asFile.resolve("arm64-v8a")
        out.deleteRecursively()
        out.mkdirs()
        targetDir.resolve("aarch64-linux-android/android/libkumi_jni.so").copyTo(out.resolve("libkumi_jni.so"), overwrite = true)
    }
}

abstract class CopyKumiPackages : DefaultTask() {
    @get:InputFiles
    @get:PathSensitive(PathSensitivity.NAME_ONLY)
    abstract val packages: ConfigurableFileCollection

    @get:OutputDirectory
    abstract val outputDir: DirectoryProperty

    @TaskAction
    fun copy() {
        val dir = outputDir.get().asFile.resolve("kumi")
        outputDir.get().asFile.deleteRecursively()
        dir.mkdirs()
        packages.files.filter { it.isFile && it.name.endsWith(".kumi") }.forEach { it.copyTo(dir.resolve(it.name), overwrite = true) }
    }
}

val defaultKumiWorkspace = if (rootProject.layout.projectDirectory.file("../kumi/Cargo.toml").asFile.isFile) "../kumi" else ".."
val kumiWorkspace = rootProject.layout.projectDirectory.dir(providers.gradleProperty("kumi.workspace").getOrElse(defaultKumiWorkspace))
val ndkVersionName = "28.2.13676358"
val ndkHost = if (System.getProperty("os.name").lowercase().contains("mac")) "darwin-x86_64" else "linux-x86_64"
val skipNative = providers.gradleProperty("kumi.skipNative").map { it.toBoolean() }.getOrElse(false)

val cargoBuildKumi = tasks.register<CargoBuildKumi>("cargoBuildKumi") {
    workspace.set(kumiWorkspace)
    val host = ndkHost
    val version = ndkVersionName
    sources.from(
        kumiWorkspace.file("Cargo.toml"),
        kumiWorkspace.file("Cargo.lock"),
        kumiWorkspace.dir("crates").asFileTree.matching(org.gradle.api.tasks.util.PatternSet().exclude("**/target/**")),
    )
    rustup.set(providers.gradleProperty("kumi.rustup").orElse("rustup"))
    linker.set(
        providers.gradleProperty("kumi.ndkDir").orElse(androidComponents.sdkComponents.sdkDirectory.map { dir -> dir.asFile.resolve("ndk/" + version).path })
            .map { dir -> dir + "/toolchains/llvm/prebuilt/" + host + "/bin/aarch64-linux-android35-clang" },
    )
    outputDir.set(layout.buildDirectory.dir("generated/kumiJni"))
}

val copyKumiPackages = tasks.register<CopyKumiPackages>("copyKumiPackages") {
    providers.gradleProperty("kumi.packagesDir").orNull?.let { packages.from(fileTree(it) { include("*.kumi") }) }
    outputDir.set(layout.buildDirectory.dir("generated/kumiPackages"))
}

val releaseKeyProp = providers.gradleProperty("kumi.releaseKey").getOrElse("").trim()
require(releaseKeyProp.isEmpty() || Regex("^[0-9a-fA-F]{64}$").matches(releaseKeyProp)) {
    "kumi.releaseKey must be empty or 64 hexadecimal characters"
}
val packagesDirProp = providers.gradleProperty("kumi.packagesDir").orNull?.trim().orEmpty()
require(packagesDirProp.isEmpty() || releaseKeyProp.isNotEmpty()) {
    "kumi.packagesDir requires kumi.releaseKey"
}
val ksFileProp = providers.gradleProperty("kumi.keystoreFile").orElse(providers.environmentVariable("KUMI_KEYSTORE_FILE")).orNull?.trim().orEmpty()
val ksPassProp = providers.gradleProperty("kumi.keystorePassword").orElse(providers.environmentVariable("KUMI_KEYSTORE_PASSWORD")).orNull.orEmpty()
val ksAliasProp = providers.gradleProperty("kumi.keyAlias").orElse(providers.environmentVariable("KUMI_KEY_ALIAS")).orNull.orEmpty()
val ksKeyPassProp = providers.gradleProperty("kumi.keyPassword").orElse(providers.environmentVariable("KUMI_KEY_PASSWORD")).orNull.orEmpty()

android {
    namespace = "org.mindone.snapcase"
    compileSdk {
        version = release(37) {
            minorApiLevel = 0
        }
    }

    defaultConfig {
        applicationId = "org.mindone.snapcase"
        minSdk = 35
        targetSdk = 36
        versionCode = providers.gradleProperty("kumi.versionCode").map(String::toInt).getOrElse(8)
        versionName = providers.gradleProperty("kumi.versionName").orElse("1.1.0").get()
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        buildConfigField("boolean", "SYSTEM_BUILD", "false")
        buildConfigField("String", "KUMI_RELEASE_KEY", "\"$releaseKeyProp\"")
        buildConfigField("String", "KUMI_RELEASES_REPO", "\"${providers.gradleProperty("kumi.releasesRepo").getOrElse("iasnezhkov/kumi")}\"")
        ndk {
            abiFilters += "arm64-v8a"
        }
    }

    ndkVersion = ndkVersionName

    dependenciesInfo {
        includeInApk = false
        includeInBundle = false
    }

    if (ksFileProp.isNotEmpty()) {
        signingConfigs {
            create("release") {
                storeFile = file(ksFileProp)
                storePassword = ksPassProp
                keyAlias = ksAliasProp
                keyPassword = ksKeyPassProp
            }
        }
    }

    buildTypes {
        debug {
            val suffix = providers.gradleProperty("kumi.appIdSuffix").getOrElse("").trim()
            if (suffix.isNotEmpty()) applicationIdSuffix = suffix
        }
        release {
            isMinifyEnabled = false
            vcsInfo {
                include = false
            }
            if (ksFileProp.isNotEmpty()) {
                signingConfig = signingConfigs.getByName("release")
            } else if (providers.gradleProperty("kumi.signWithDebug").map { it.toBoolean() }.getOrElse(false)) {
                signingConfig = signingConfigs.getByName("debug")
            }
        }
        create("system") {
            initWith(getByName("release"))
            buildConfigField("boolean", "SYSTEM_BUILD", "true")
            signingConfig = null
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    testOptions {
        unitTests.isReturnDefaultValues = true
    }

    lint {
        abortOnError = true
        checkReleaseBuilds = false
    }
}

val kumiContract = kumiWorkspace.file("crates/kumi-jni/tests/fixtures/contract.json")

tasks.withType<Test>().configureEach {
    inputs.file(kumiContract).withPathSensitivity(PathSensitivity.NONE).withPropertyName("kumiContract")
    systemProperty("kumi.contract", kumiContract.asFile.path)
}

androidComponents {
    onVariants { variant ->
        variant.sources.assets?.addGeneratedSourceDirectory(copyKumiPackages, CopyKumiPackages::outputDir)
        if (!skipNative) variant.sources.jniLibs?.addGeneratedSourceDirectory(cargoBuildKumi, CargoBuildKumi::outputDir)
    }
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.datastore.preferences)
    implementation(libs.kotlinx.coroutines.android)
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.compose.material3)
    debugImplementation(libs.compose.ui.tooling)
    testImplementation(libs.junit)
    testImplementation(libs.json)
    testImplementation(libs.kotlinx.coroutines.test)
}
