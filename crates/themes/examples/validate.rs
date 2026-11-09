//! Checks theme files: `cargo run -p themes --example validate -- <file.toml>...`
fn main() {
    let mut bad = 0;
    for path in std::env::args().skip(1) {
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                println!("{path}: cannot read ({e})");
                bad += 1;
                continue;
            }
        };
        match themes::Theme::from_toml_str(&text) {
            Ok((theme, warnings)) => {
                println!(
                    "{path}: \"{}\" ok, {} warning(s)",
                    theme.meta.name,
                    warnings.len()
                );
                for w in &warnings {
                    println!("  - {w}");
                }
                bad += warnings.len();
            }
            Err(e) => {
                println!("{path}: invalid ({e})");
                bad += 1;
            }
        }
    }
    std::process::exit(if bad > 0 { 1 } else { 0 });
}
