plugins { id("com.android.library") }
android {
    namespace = "com.termux.emulator"
    compileSdk = 35
    ndkVersion = "27.2.12479018"
    defaultConfig {
        minSdk = 26
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
        externalNativeBuild {
            ndkBuild {
                cFlags += listOf("-std=c11", "-Wall", "-Wextra", "-Werror", "-O2")
                arguments += "APP_SUPPORT_FLEXIBLE_PAGE_SIZES=true"
            }
        }
    }
    externalNativeBuild { ndkBuild { path = file("src/main/jni/Android.mk") } }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    testOptions { unitTests.isReturnDefaultValues = true }
    sourceSets["main"].java.srcDir(rootProject.file("terminal-adapter/src/main/java"))
    sourceSets["test"].java.srcDir(rootProject.file("terminal-adapter/src/test/java"))
}
dependencies {
    implementation("androidx.annotation:annotation:1.9.1")
    testImplementation("junit:junit:4.13.2")
}
