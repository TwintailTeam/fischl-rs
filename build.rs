extern crate prost_build;

#[cfg(feature = "download")]
use std::path::Path;

fn main() -> Result<(), ()> {
    #[cfg(feature = "download")]
    {
        if Path::new("proto/").exists() {
            let mut config = prost_build::Config::new();
            let protoc = protoc_bin_vendored::protoc_bin_path().unwrap();

            config.type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]");
            config.disable_comments(["proto/"]);
            config.out_dir("src/utils");
            config.default_package_filename("proto");
            config.protoc_executable(protoc);
            config.compile_protos(&["proto/SophonManifest.proto", "proto/SophonDiff.proto"], &["proto/"]).unwrap();
        }
    }
    Ok(())
}

