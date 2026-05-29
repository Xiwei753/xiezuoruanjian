1. The goal is to completely rewrite FFI via explicit strong typing for Android JNI, avoiding UniFFI generation pitfalls (since UniFFI requires invasive UDL bindings and generates complicated bridge code that I couldn't perfect automatically with limited passes).

Actually, the user asked me to migrate "Android FFI". The issue is the user expects `UniFFI` OR "strongly-typed JNI". Since UniFFI failed twice due to edge-cases in `udl` macros and struct mappings in the previous runs, manual JNI is actually safer.

Wait, manual JNI is robust. Let's just create an `AppServiceBridge.kt` and `bindings/android/src/lib.rs` that use strong types directly.
Is that simpler? No, writing 30 JNI endpoints manually (C to Rust and back) will take a very long time and is prone to JNI Signature mistakes.

Let me retry UniFFI. The last error with UniFFI was `E0220: associated type Io not found for Self`. This occurs because I defined `[Error] enum WriterError` in UDL but I derived `uniffi::Error` on the Rust enum, and uniffi flat error expects specific implementations.
If I use `uniffi::Error` macro, I should just use `#[derive(uniffi::Error)]` without `[Error]` in UDL? No, `api.udl` needs it.

Wait, I don't even need a `.udl` file anymore in UniFFI 0.28! UniFFI 0.24+ supports proc-macros instead of UDL. I can just write pure Rust, add `#[uniffi::export]` and it will generate the bindings! This is amazing!
If I remove `interface WriterAppService` from UDL and just use `#[uniffi::export]` in Rust, UniFFI 0.28 handles everything!

Let's test this theory.
