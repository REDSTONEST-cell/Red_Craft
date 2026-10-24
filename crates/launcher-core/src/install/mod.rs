//! 版本安装与文件补全。

mod assets;
mod vanilla;

pub use assets::{AssetIndex, AssetObject, RESOURCES_URL};
pub use vanilla::{validate_instance_name, Installer, Reporter, Verify};
