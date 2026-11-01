buildscript {
    repositories {
        google()
        mavenCentral()
    }
    dependencies {
        classpath("com.android.tools.build:gradle:9.3.1")
        // KGP 2.4.21 supports AGP 9.3.1 and Gradle 9.6.1 together.
        // KGP 2.2.10 predates both and falls outside Kotlin's supported matrix.
        classpath("org.jetbrains.kotlin:kotlin-gradle-plugin:2.4.21")
    }
}

allprojects {
    repositories {
        google()
        mavenCentral()
    }
}

tasks.register("clean").configure {
    delete("build")
}
