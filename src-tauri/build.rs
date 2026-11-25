use std::path::Path;

fn main() {
    // 陶瓦联机发行包（scripts/fetch-terracotta.sh 下载并校验）：存在时嵌入启动器，
    // 否则联机功能显示为「此版本未包含联机组件」，不影响其它功能的编译与测试。
    println!("cargo::rustc-check-cfg=cfg(terracotta_bundled)");
    let dir = Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("terracotta");
    println!("cargo:rerun-if-changed={}", dir.display());
    let target = std::env::var("TARGET").unwrap();
    let pkg = dir.join(format!("{target}.tar.gz"));
    if pkg.is_file() {
        println!("cargo:rustc-cfg=terracotta_bundled");
        println!("cargo:rustc-env=RCL_TERRACOTTA_PKG={}", pkg.display());
    }
    tauri_build::build()
}
