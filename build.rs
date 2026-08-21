use std::{env, fs, path::Path};

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest = Path::new(&out_dir);
    let creds_path = Path::new("credentials.json");
    let creds_file = dest.join("credentials.rs");
    if creds_path.exists() {
        let creds = fs::read_to_string(creds_path).unwrap();
        fs::write(
            &creds_file,
            format!("pub const EMBEDDED_CREDENTIALS: Option<&str> = Some({creds:?});"),
        )
        .unwrap();
        println!("cargo:rerun-if-changed=credentials.json");
    } else {
        fs::write(
            &creds_file,
            "pub const EMBEDDED_CREDENTIALS: Option<&str> = None;",
        )
        .unwrap();
    }
}
