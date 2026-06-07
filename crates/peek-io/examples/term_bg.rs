//! Diagnostic: probe the terminal background color via OSC 11.
//!
//! Run from a real terminal (it needs a controlling tty):
//!
//! ```sh
//! cargo run -p peek-io --example term_bg
//! ```

fn main() {
    match peek_io::query_background_color() {
        Some(bg) => {
            let kind = if bg.is_light() { "light" } else { "dark" };
            println!(
                "background = #{:02x}{:02x}{:02x}  luma={:.3}  -> {kind}",
                bg.r,
                bg.g,
                bg.b,
                bg.luma()
            );
        }
        None => println!("no reply (not a terminal, or terminal ignored OSC 11)"),
    }
}
