// 릴리스 빌드에서 윈도우 콘솔 창이 뜨지 않게 함.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tonghap_module_lib::run()
}
