//! With the `dlss` feature, copy NVIDIA's DLSS runtime libraries next to the executable, where
//! the DLSS runtime looks for them.

fn main() {
    println!("cargo:rerun-if-env-changed=DLSS_SDK");
    if std::env::var_os("CARGO_FEATURE_DLSS").is_none() {
        return;
    }
    let Some(sdk) = std::env::var_os("DLSS_SDK") else { return };
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    // OUT_DIR is target/<profile>/build/<pkg>/out
    let Some(profile_dir) = out.ancestors().nth(3) else { return };
    let (dir, names): (_, &[&str]) = if cfg!(target_os = "windows") {
        ("lib/Windows_x86_64/rel", &["nvngx_dlss.dll", "nvngx_dlssd.dll"])
    } else {
        ("lib/Linux_x86_64/rel", &["libnvidia-ngx-dlss.so.310.5.3", "libnvidia-ngx-dlssd.so.310.5.3"])
    };
    for name in names {
        let src = std::path::Path::new(&sdk).join(dir).join(name);
        if src.exists() {
            let _ = std::fs::copy(&src, profile_dir.join(name));
        }
        println!("cargo:rerun-if-changed={}", src.display());
    }
}
