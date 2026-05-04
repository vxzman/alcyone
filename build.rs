fn main() {
    println!(
        "cargo:rustc-env=APP_VERSION={}",
        std::env::var("APP_VERSION").unwrap_or_else(|_| "dev".to_string())
    );
    println!(
        "cargo:rustc-env=APP_COMMIT={}",
        std::env::var("APP_COMMIT").unwrap_or_else(|_| "unknown".to_string())
    );
    println!(
        "cargo:rustc-env=APP_BUILD_DATE={}",
        std::env::var("APP_BUILD_DATE").unwrap_or_else(|_| String::new())
    );
}
