fn main() {
    // Minimal build script for WinlatorXR — no shaders, no git version
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap();

    let vrclient_name = match (target_os.as_str(), target_arch.as_str()) {
        ("windows", "x86_64") => "vrclient_x64",
        _ => "vrclient",
    };

    let platform_location = match (target_os.as_str(), target_arch.as_str()) {
        ("windows", "x86") | ("windows", "x86_64") => "bin/",
        ("linux", "x86") => "bin/",
        ("linux", "x86_64") => "bin/linux64/",
        ("linux", "aarch64") => "bin/linuxarm64/",
        _ => "bin/",
    };

    println!("cargo:rustc-env=XRIZER_OPENVR_PLATFORM_DIR={platform_location}");
    println!("cargo:rustc-env=XRIZER_OPENVR_VRCLIENT_NAME={vrclient_name}");

    // Provide fallback for VERGEN_GIT_DESCRIBE
    let version = if let Ok(describe) = std::process::Command::new("git")
        .args(["describe", "--always", "--dirty"])
        .output()
    {
        if describe.status.success() {
            String::from_utf8_lossy(&describe.stdout).trim().to_string()
        } else {
            env!("CARGO_PKG_VERSION").to_string()
        }
    } else {
        env!("CARGO_PKG_VERSION").to_string()
    };
    println!("cargo:rustc-env=VERGEN_GIT_DESCRIBE={version}");

    // On Windows, we don't need shaders since we're using WinlatorXR's XrAPI
    // Instead, we'll create empty stub shader files to satisfy the build
    if target_os == "windows" {
        let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());

        std::fs::write(out_dir.join("vert_overlay.spv"), &[]).unwrap();
        std::fs::write(out_dir.join("frag_overlay.spv"), &[]).unwrap();

        println!("cargo:rustc-link-lib=d3d11");
        println!("cargo:rustc-link-lib=dxgi");
    }
}