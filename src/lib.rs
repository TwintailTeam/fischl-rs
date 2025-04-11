pub mod compat;
pub mod utils;
mod download;

#[cfg(test)]
mod tests {

    /*#[test]
    fn prefix_new() {
        let wp = "/home/tukan/.local/share/com.keqinglauncher.app/compatibility/runners/8.26-wine-ge-proton/bin/wine64".to_string();
        let pp = "/home/tukan/.local/share/com.keqinglauncher.app/compatibility/prefixes/nap_global/1.6.0".to_string();
        let prefix = Compat::setup_prefix(wp, pp);
        if prefix.is_ok() {
            let p = prefix.unwrap();
            Compat::end_session(p.wine.wineloader().to_str().unwrap().to_string(), p.wine.prefix.to_str().unwrap().to_string()).unwrap();
            Compat::shutdown(p.wine.wineloader().to_str().unwrap().to_string(), p.wine.prefix.to_str().unwrap().to_string()).unwrap();
            println!("Created prefix without issues!");
        } else {
            println!("Failed to create a prefix!");
        }
    }*/
    use std::path::Path;
    use crate::download::compatibility::{download_dxvk, download_runner};
    use crate::download::xxmi::{download_xxmi, download_xxmi_packages};
    use crate::utils::extract_archive;

    #[test]
    fn download_xxmi_test() {
        let dest = Path::new("/home/tukan/.local/share/com.keqinglauncher.app/extras/xxmi");
        let success = download_xxmi(dest);
        if success.is_some() {
            let finaldest = dest.join("xxmi.zip");
            let extract = extract_archive(finaldest.to_str().unwrap().to_string(), dest.join("testing").to_str().unwrap().to_string());

            if extract.is_some() {
                println!("xxmi extracted!")
            } else {
                println!("Failed to extract!");
            }
        } else {
            println!("failed to download extras");
        }
    }

    #[test]
    fn download_xxmi_packages_test() {
        let dest = Path::new("/home/tukan/.local/share/com.keqinglauncher.app/extras/xxmi/testing");
        let success = download_xxmi_packages(dest);
        if success.is_some() {
            extract_archive(dest.join("gimi.zip").to_str().unwrap().to_string(), dest.join("gimi").to_str().unwrap().to_string());
            extract_archive(dest.join("srmi.zip").to_str().unwrap().to_string(), dest.join("srmi").to_str().unwrap().to_string());
            extract_archive(dest.join("zzmi.zip").to_str().unwrap().to_string(), dest.join("zzmi").to_str().unwrap().to_string());
            extract_archive(dest.join("wwmi.zip").to_str().unwrap().to_string(), dest.join("wwmi").to_str().unwrap().to_string());
            println!("xxmi packages extracted!")
        } else {
            println!("Failed to download xxmi packages");
        }
    }

    #[test]
    fn download_runner_test() {
        let dest = Path::new("/home/tukan/.local/share/com.keqinglauncher.app/compatibility/runners/10.4-wine-vanilla");
        let url = "https://github.com/Kron4ek/Wine-Builds/releases/download/10.4/wine-10.4-amd64.tar.xz";

        let success = download_runner(url.to_string(), dest);
        if success.is_some() {
            let finaldest = dest.join("runner.zip");
            let extract = extract_archive(finaldest.to_str().unwrap().to_string(), dest.to_str().unwrap().to_string());

            if extract.is_some() {
                println!("runner extracted!")
            } else {
                println!("Failed to extract!");
            }
        } else {
            println!("failed to download runner");
        }
    }

    #[test]
    fn download_dxvk_test() {
        let dest = Path::new("/home/tukan/.local/share/com.keqinglauncher.app/compatibility/dxvk/2.6.0-vanilla");
        let url = "https://github.com/doitsujin/dxvk/releases/download/v2.6/dxvk-2.6.tar.gz";

        let success = download_dxvk(url.to_string(), dest);
        if success.is_some() {
            let finaldest = dest.join("dxvk.zip");
            let extract = extract_archive(finaldest.to_str().unwrap().to_string(), dest.to_str().unwrap().to_string());

            if extract.is_some() {
                println!("dxvk extracted!")
            } else {
                println!("Failed to extract!");
            }
        } else {
            println!("failed to download dxvk");
        }
    }
}
