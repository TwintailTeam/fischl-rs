extern crate prost_build;

use std::path::Path;

fn main() -> Result<(), ()> {
    if Path::new("proto/").exists() {
        let mut config = prost_build::Config::new();

        config.type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]");
        config.disable_comments(["proto/"]);
        config.out_dir("src/utils");
        config.default_package_filename("proto");

        config.compile_protos(&["proto/SophonManifest.proto", "proto/SophonDiff.proto"], &["proto/"]).unwrap();
    }

    Ok(())
}

