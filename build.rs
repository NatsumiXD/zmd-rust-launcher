fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        assert_eq!(
            std::env::var("CARGO_CFG_TARGET_ENV").as_deref(),
            Ok("msvc"),
            "The Windows launcher requires MSVC to embed its administrator manifest"
        );

        // Scope elevation to the launcher; process-probe must remain unelevated.
        println!("cargo:rustc-link-arg-bin=endfield-direct-launch=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-bin=endfield-direct-launch=/MANIFESTUAC:level='requireAdministrator' uiAccess='false'"
        );
    }
}
