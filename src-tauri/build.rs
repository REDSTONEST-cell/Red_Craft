fn main() {
    // Windows 正式版默认以管理员身份运行；开发时（debug）保持普通权限，方便调试
    let mut attributes = tauri_build::Attributes::new();
    if std::env::var("PROFILE").as_deref() == Ok("release") {
        attributes = attributes
            .windows_attributes(tauri_build::WindowsAttributes::new().app_manifest(include_str!("windows/app.manifest")));
    }
    println!("cargo:rerun-if-changed=windows/app.manifest");
    tauri_build::try_build(attributes).expect("failed to run tauri-build");
}
