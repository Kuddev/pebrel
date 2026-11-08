use std::{env, fs, path::PathBuf};

fn main() {
    // Reuse the installer's existing Explorer label translations. This DLL
    // runs in the Windows COM host, independent of the application's UI.
    let installer = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../scripts/installer.iss");
    println!("cargo:rerun-if-changed={}", installer.display());
    let source = fs::read_to_string(installer).unwrap();
    let title = |key: &str| {
        source
            .lines()
            .find_map(|line| line.strip_prefix(key))
            .expect("installer menu title")
            .encode_utf16()
            .chain(Some(0))
            .collect::<Vec<_>>()
    };
    let output = format!(
        "const OPEN_IN_PEBREL_EN: &[u16] = &{:?};\nconst OPEN_IN_PEBREL_ZH: &[u16] = &{:?};\n",
        title("english.OpenInPebrel="),
        title("chinesesimplified.OpenInPebrel=")
    );
    fs::write(PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("titles.rs"), output).unwrap();
}
