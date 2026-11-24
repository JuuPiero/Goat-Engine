// The 3DS player: platform + game, nothing else.

fn main() {
    #[cfg(target_os = "horizon")]
    goat_platform_3ds::run(hello_game::HelloGame);
}
