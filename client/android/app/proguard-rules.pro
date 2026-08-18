# Intentionally minimal for initial setup.

# rustls-platform-verifier reaches these classes only from Rust via JNI, so
# nothing on the Java side references them and they would otherwise be stripped
# or renamed, breaking certificate verification at runtime.
-keep, includedescriptorclasses class org.rustls.platformverifier.** { *; }
