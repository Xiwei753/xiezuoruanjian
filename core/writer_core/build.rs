fn main() {
    #[allow(clippy::unwrap_used)]
    uniffi::generate_scaffolding("./src/api.udl").unwrap();
}
