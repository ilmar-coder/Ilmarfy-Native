plugins {
    alias(libs.plugins.android.application) apply false
    alias(libs.plugins.kotlin.compose) apply false
}

// Documentos é sincronizado com o iCloud, que duplica arquivos de build ("arquivo 2.xml") e quebra o Gradle.
// Pastas terminadas em .nosync ficam fora do iCloud.
allprojects {
    layout.buildDirectory = rootProject.layout.projectDirectory.dir("build.nosync/${project.name}")
}
