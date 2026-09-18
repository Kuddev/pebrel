plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
    id("org.jetbrains.kotlin.plugin.serialization")
}

android {
    namespace = "io.github.kuddev.pebrel.mobile"
    compileSdk = 35
    ndkVersion = "27.2.12479018"
    defaultConfig {
        applicationId = "io.github.kuddev.pebrel.mobile"
        minSdk = 26
        targetSdk = 35
        versionCode = 12
        versionName = "0.4.5-preview"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }
    buildFeatures { compose = true }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    signingConfigs {
        create("pebrelPreview") {
            // Public development identity. Never use this key for the release package.
            storeFile = rootProject.file("pebrel-preview.p12")
            storeType = "PKCS12"
            storePassword = "android"
            keyAlias = "pebrel-preview"
            keyPassword = "android"
        }
    }
    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
        create("preview") {
            initWith(getByName("release"))
            applicationIdSuffix = ".preview"
            signingConfig = signingConfigs.getByName("pebrelPreview")
            matchingFallbacks += "release"
            testProguardFiles("test-proguard-rules.pro")
            proguardFiles("preview-proguard-rules.pro")
        }
    }
    packaging { resources.excludes += setOf("META-INF/DEPENDENCIES", "META-INF/versions/9/OSGI-INF/MANIFEST.MF") }
    lint { abortOnError = true }
    testBuildType = "preview"
    testOptions { unitTests.isIncludeAndroidResources = true }
    sourceSets["main"].assets.srcDir("build/generated/pebrelAssets")
    sourceSets["main"].assets.srcDir("build/generated/nativeRelayAssets")
    sourceSets["main"].res.srcDir("build/generated/pebrelResources")
}

dependencies {
    implementation(project(":ghostty"))
    implementation(project(":ssh"))
    implementation(project(":voice"))
    implementation(platform("androidx.compose:compose-bom:2025.05.01"))
    implementation("androidx.activity:activity-compose:1.10.1")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-core")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.9.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.8.1")
    implementation("com.squareup.okhttp3:okhttp:4.12.0")
    implementation("com.journeyapps:zxing-android-embedded:4.3.0")
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.10.2")
    testImplementation("org.robolectric:robolectric:4.14.1")
    testImplementation("com.squareup.okhttp3:mockwebserver:4.12.0")
    testImplementation("com.squareup.okhttp3:okhttp-tls:4.12.0")
    testImplementation("androidx.compose.ui:ui-test-junit4")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
    androidTestImplementation("androidx.test:runner:1.6.2")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("androidx.test:rules:1.6.1")
    androidTestImplementation("androidx.test.uiautomator:uiautomator:2.3.0")
    // AndroidX Test references these annotations when its APK is shrunk by R8.
    androidTestImplementation("com.google.errorprone:error_prone_annotations:2.36.0")
}

val generateAssets by tasks.registering(Exec::class) {
    val root = rootProject.projectDir.resolve("../..").canonicalFile
    inputs.files(root.resolve("nebula_settings/src/lib.rs"), root.resolve("nebula_settings/src/themes.rs"),
        root.resolve("nebula_app/src/display/ui/os_icons.rs"), root.resolve("assets/fonts/MapleMono-NF-CN-Regular.ttf"),
        root.resolve("assets/fonts/JetBrainsMono-Regular.ttf"), root.resolve("extra/logo/nebula-titanium.png"))
    inputs.dir(root.resolve("mobile/tools"))
    inputs.files(fileTree(root.resolve("mobile/relay")) { include("*.mjs", "*.json", "*.md", "*.yaml", "Caddyfile", "Dockerfile") })
    inputs.file(root.resolve("mobile/protocol/bridge-policy.json"))
    inputs.file(root.resolve("LICENSE"))
    inputs.dir(root.resolve("mobile/android/third_party/licenses"))
    inputs.files(root.resolve("mobile/android/third_party/THIRD-PARTY-NOTICES.md"),
        root.resolve("mobile/android/ghostty/UPSTREAM.json"))
    inputs.dir(root.resolve("mobile/android/ghostty/build/upstream/arm64-v8a/licenses"))
    outputs.dir(layout.buildDirectory.dir("generated/pebrelAssets"))
    outputs.dir(layout.buildDirectory.dir("generated/pebrelResources"))
    commandLine("python3", root.resolve("mobile/tools/generate_assets.py"),
        "--output", layout.buildDirectory.dir("generated/pebrelAssets").get().asFile,
        "--resources", layout.buildDirectory.dir("generated/pebrelResources").get().asFile)
}
tasks.named("preBuild") { dependsOn(generateAssets) }
