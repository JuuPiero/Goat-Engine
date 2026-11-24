//! Converts a PNG / JPEG into an ETC1 KTX with mipmaps (sized to powers of two):
//! `cargo run -p goat-assets --example to_ktx -- in.png out.ktx`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output] = args.as_slice() else {
        eprintln!("usage: to_ktx <in.png|in.jpg> <out.ktx>");
        std::process::exit(2);
    };
    let bytes = std::fs::read(input).expect("read input");
    let t = if bytes.starts_with(&[0xFF, 0xD8]) { goat_assets::image::decode_jpeg(&bytes) } else { goat_assets::image::decode_png(&bytes) }.expect("decode");
    let t = goat_assets::image::to_power_of_two(&t).with_mipmaps();
    std::fs::write(output, goat_assets::image::encode_ktx_etc1(&t)).expect("write output");
    println!("{output}: {}x{} ETC1", t.width, t.height);
}
