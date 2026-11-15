plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

android {
    namespace = "br.com.ilmar.ilmarfy"
    compileSdk = 37

    defaultConfig {
        applicationId = "br.com.ilmar.ilmarfy"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        ndk { abiFilters += listOf("arm64-v8a") }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            // Instalação direta (sem Play Store): assina com a chave de debug até existir uma chave própria
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    buildFeatures { compose = true }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

// Classes Kotlin geradas pelo UniFFI a partir do núcleo em Rust (build-rust.sh)
androidComponents {
    onVariants { variant ->
        variant.sources.kotlin?.addStaticSourceDirectory("build/generated/uniffi")
    }
}

// Compila o núcleo Rust (libilmarfy_ffi.so) e gera os bindings antes de cada build
val buildRust by tasks.registering(Exec::class) {
    workingDir = rootProject.projectDir
    commandLine("./build-rust.sh")
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
