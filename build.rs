use std::{env, fs, path::Path};

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest = Path::new(&out_dir).join("credentials.rs");
    let creds_path = Path::new("credentials.json");

    if creds_path.exists() {
        let creds = fs::read_to_string(creds_path).unwrap();
        fs::write(
            &dest,
            format!("pub const EMBEDDED_CREDENTIALS: Option<&str> = Some({creds:?});"),
        )
        .unwrap();
        println!("cargo:rerun-if-changed=credentials.json");
    } else {
        fs::write(&dest, "pub const EMBEDDED_CREDENTIALS: Option<&str> = None;").unwrap();
    }
}
