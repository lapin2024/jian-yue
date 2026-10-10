fn main() {
    println!("cargo:rerun-if-changed=assets/icons/app.ico");
    println!("cargo:rerun-if-changed=assets/icons/app.rc");
    if std::env::var("TARGET").as_deref() == Ok("aarch64-pc-windows-gnullvm") {
        // LLVM windres' BFD target aliases differ for ARM64. Compile .res and
        // convert to an explicit ARM64 COFF object, avoiding the BFD alias.
        let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
        let res = out.join("app-icons.res");
        let obj = out.join("app-icons.obj");
        let status = std::process::Command::new("llvm-rc")
            .arg("/no-preprocess")
            .args(["/C", "65001"])
            .arg("/fo")
            .arg(&res)
            .arg("assets/icons/app.rc")
            .status()
            .expect("llvm-rc is required for ARM64 icon resources");
        assert!(status.success(), "ARM64 icon RC compilation failed");
        let status = std::process::Command::new("llvm-cvtres")
            .arg("/machine:ARM64")
            .arg(format!("/out:{}", obj.display()))
            .arg(&res)
            .status()
            .expect("llvm-cvtres is required for ARM64 icon resources");
        assert!(status.success(), "ARM64 icon COFF conversion failed");
        println!("cargo:rustc-link-arg={}", obj.display());
    } else if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("assets/icons/app.rc", embed_resource::NONE)
            .manifest_required()
            .expect("Windows icon resource compilation failed");
    }
}
