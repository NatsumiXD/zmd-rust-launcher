fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    for name in ["GITHUB_SHA", "GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT"] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|value| value.trim().to_string())
    };
    for name in ["HEAD", "packed-refs"] {
        if let Some(path) = git(&["rev-parse", "--git-path", name])
            && std::path::Path::new(&path).is_file()
        {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    if let Some(branch) = git(&["symbolic-ref", "--quiet", "HEAD"])
        && let Some(path) = git(&["rev-parse", "--git-path", &branch])
        && std::path::Path::new(&path).is_file()
    {
        println!("cargo:rerun-if-changed={path}");
    }
    let commit = std::env::var("GITHUB_SHA")
        .ok()
        .or_else(|| git(&["rev-parse", "HEAD"]))
        .filter(|value| value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .unwrap_or_else(|| "unknown".into());
    let build = match (
        std::env::var("GITHUB_RUN_ID"),
        std::env::var("GITHUB_RUN_ATTEMPT"),
    ) {
        (Ok(run), Ok(attempt))
            if [run.as_str(), attempt.as_str()].iter().all(|value| {
                !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
            }) =>
        {
            format!("{run}.{attempt}")
        }
        _ => "local".into(),
    };
    println!("cargo:rustc-env=LAUNCHER_COMMIT={commit}");
    println!("cargo:rustc-env=LAUNCHER_BUILD={build}");

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
