import java.util.Properties

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

fun props(file: File) = Properties().apply { if (file.exists()) file.inputStream().use { load(it) } }

// Versão controlada pelo release.sh
val version = props(rootProject.file("version.properties"))
// Chave de assinatura fixa (fora do repositório); todas as versões precisam dela para atualizar no celular
val keystore = props(rootProject.file("local.properties")).getProperty("ilmarfy.keystore")

android {
    namespace = "br.com.ilmar.ilmarfy"
    compileSdk = 37

    defaultConfig {
        applicationId = "br.com.ilmar.ilmarfy"
        minSdk = 26
        targetSdk = 36
        versionCode = version.getProperty("versionCode").toInt()
        versionName = version.getProperty("versionName")
        ndk { abiFilters += listOf("arm64-v8a") }
    }

    signingConfigs {
        create("ilmarfy") {
            storeFile = file(keystore ?: "${System.getProperty("user.home")}/.android/debug.keystore")
            storePassword = "android"
            keyAlias = "androiddebugkey"
            keyPassword = "android"
        }
    }

    buildTypes {
        debug { signingConfig = signingConfigs.getByName("ilmarfy") }
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName("ilmarfy")
        }
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

// Saída do núcleo Rust (biblioteca .so e classes Kotlin do UniFFI), dentro do build fora do iCloud
val rustOut = layout.buildDirectory.dir("rust").get()

// Classes Kotlin geradas pelo UniFFI a partir do núcleo em Rust (build-rust.sh)
androidComponents {
    onVariants { variant ->
        variant.sources.kotlin?.addStaticSourceDirectory(rustOut.dir("uniffi").asFile.path)
        variant.sources.jniLibs?.addStaticSourceDirectory(rustOut.dir("jniLibs").asFile.path)
    }
}

// Compila o núcleo Rust e gera os bindings antes de cada build
val buildRust by tasks.registering(Exec::class) {
    workingDir = rootProject.projectDir
    commandLine("./build-rust.sh", rustOut.asFile.path)
}
tasks.named("preBuild") { dependsOn(buildRust) }

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.navigation.compose)
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.material3)
    implementation(libs.compose.material.icons)
    implementation(libs.media3.exoplayer)
    implementation(libs.media3.session)
    implementation(libs.media3.datasource)
    implementation(libs.coil.compose)
    implementation(libs.coil.network)
    implementation(libs.coroutines.android)
    implementation(libs.coroutines.guava)
    implementation("${libs.jna.get()}@aar")
}
