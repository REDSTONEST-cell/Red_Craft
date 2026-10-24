// Release 版本在 Windows 上不显示控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    red_craft_lib::run()
}
