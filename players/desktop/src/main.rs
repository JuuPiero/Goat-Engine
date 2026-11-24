// The desktop player: platform + game, nothing else.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    goat_platform_desktop::run(hello_game::HelloGame, goat_platform_desktop::DesktopOptions::from_args());
}
