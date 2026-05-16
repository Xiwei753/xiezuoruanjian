use std::env;

fn main() {
    println!("cargo:rerun-if-changed=qml/main.qml");
    println!("cargo:rerun-if-changed=src/qml.qrc");
    let mut config = cpp_build::Config::new();
    config.include(env::var("DEP_QT_INCLUDE").unwrap_or_else(|_| "".into()));
    config.build("src/main.rs");
}
