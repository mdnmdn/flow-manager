fn main() {
    let version =
        std::env::var("FM_VERSION").unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string());
    println!("cargo:rustc-env=FM_VERSION={}", version);
    println!("cargo:rerun-if-env-changed=FM_VERSION");
}
