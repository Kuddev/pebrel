plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "io.github.kuddev.pebrel.ssh"
    compileSdk = 35
    defaultConfig {
        minSdk = 26
        consumerProguardFiles("consumer-rules.pro")
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    sourceSets["main"].jniLibs.srcDir("build/jniLibs")
    sourceSets["main"].assets.srcDir("build/assets")
}

tasks.register("verifyNativeInput") {
    doLast {
        listOf("arm64-v8a", "x86_64").forEach { abi ->
            check(file("build/jniLibs/$abi/libpebrel_ssh.so").isFile) {
                "Build public russh with mobile/tools/build_russh.py before packaging"
            }
        }
    }
}
tasks.named("preBuild") { dependsOn("verifyNativeInput") }
