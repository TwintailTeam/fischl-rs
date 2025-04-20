pub mod compat;
pub mod utils;
pub mod download;

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
    use crate::download::Downloader;
    use crate::download::game::{Game, Repairer};
    use crate::utils::{extract_archive, prettify_bytes};
    use crate::utils::game::hoyo::voice_locale::VoiceLocale;

    #[test]
    fn download_xxmi_test() {
        let dest = Path::new("/home/tukan/.local/share/com.keqinglauncher.app/extras/xxmi");
        let success = Downloader::download_xxmi(String::from("SpectrumQT/XXMI-Libs-Package"), dest);
        if success.is_some() {
            let finaldest = dest.join("xxmi.zip");
            let extract = extract_archive(finaldest.to_str().unwrap().to_string(), dest.join("testing").to_str().unwrap().to_string(), false);

            if extract.is_some() {
                println!("xxmi extracted!");
                Downloader::download_xxmi_loader(String::from("KeqingLauncher-extras/3dmloader-Package"), &dest.join("testing"), true).unwrap();
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
        let success = Downloader::download_xxmi_packages(String::from("SilentNightSound/GIMI-Package"), String::from("SpectrumQT/SRMI-Package"), String::from("leotorrez/ZZMI-Package"), String::from("SpectrumQT/WWMI-Package"), dest, false);
        if success.is_some() {
            extract_archive(dest.join("gimi.zip").to_str().unwrap().to_string(), dest.join("gimi").to_str().unwrap().to_string(), false);
            extract_archive(dest.join("srmi.zip").to_str().unwrap().to_string(), dest.join("srmi").to_str().unwrap().to_string(), false);
            extract_archive(dest.join("zzmi.zip").to_str().unwrap().to_string(), dest.join("zzmi").to_str().unwrap().to_string(), false);
            extract_archive(dest.join("wwmi.zip").to_str().unwrap().to_string(), dest.join("wwmi").to_str().unwrap().to_string(), false);
            println!("xxmi packages extracted!")
        } else {
            println!("Failed to download xxmi packages");
        }
    }

    #[test]
    fn download_runner_test() {
        let dest = Path::new("/home/tukan/.local/share/com.keqinglauncher.app/compatibility/runners/10.4-wine-vanilla");
        let url = "https://github.com/Kron4ek/Wine-Builds/releases/download/10.4/wine-10.4-amd64.tar.xz";

        let success = Downloader::download_runner(url.to_string(), dest);
        if success.is_some() {
            let finaldest = dest.join("runner.zip");
            let extract = extract_archive(finaldest.to_str().unwrap().to_string(), dest.to_str().unwrap().to_string(), true);

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

        let success = Downloader::download_dxvk(url.to_string(), dest);
        if success.is_some() {
            let finaldest = dest.join("dxvk.zip");
            let extract = extract_archive(finaldest.to_str().unwrap().to_string(), dest.to_str().unwrap().to_string(), true);

            if extract.is_some() {
                println!("dxvk extracted!")
            } else {
                println!("Failed to extract!");
            }
        } else {
            println!("failed to download dxvk");
        }
    }

    #[test]
    fn download_fpsunlock_test() {
        let dest = Path::new("/home/tukan/.local/share/com.keqinglauncher.app/extras/fps_unlock/testing");

        let success = Downloader::download_fps_unlock(String::from("mkrsym1/fpsunlock"), &dest);
        if success.is_some() {
            println!("fps unlock downloaded!")
        } else {
            println!("failed to download dxvk");
        }
    }

    #[test]
    fn download_jadeite_test() {
        let dest = Path::new("/home/tukan/.local/share/com.keqinglauncher.app/extras/jadeite/testing");

        let success = Downloader::download_jadeite(String::from("mkrsym1/jadeite"), &dest);
        if success.is_some() {
            let finaldest = dest.join("jadeite.zip");
            let extract = extract_archive(finaldest.to_str().unwrap().to_string(), dest.to_str().unwrap().to_string(), false);

            if extract.is_some() {
                println!("jadeite extracted!")
            } else {
                println!("Failed to extract!");
            }
        } else {
            println!("failed to download dxvk");
        }
    }

    // WARNING: Repair game test will take A REALLY LONG time!
    #[test]
    fn repair_game_test() {
        let res_list = String::from("https://autopatchhk.yuanshen.com/client_app/download/pc_zip/20250314110016_HcIQuDGRmsbByeAE/ScatteredFiles");
        let path = "/games/hoyo/hk4e_global/live";
        let rep = Repairer::repair_game(res_list, path.parse().unwrap(), false);
        if rep { 
            println!("repair_game success!");
        } else {
            println!("repair_game failure!");
        }
    }

    // WARNING: Repair audio test will take A REALLY LONG time!
    #[test]
    fn repair_audio_test() {
        let res_list = String::from("https://autopatchhk.yuanshen.com/client_app/download/pc_zip/20250314110016_HcIQuDGRmsbByeAE/ScatteredFiles");
        let path = "/games/hoyo/hk4e_global/live";
        let rep = Repairer::repair_audio(res_list, VoiceLocale::English.to_folder().to_string(), path.parse().unwrap(), false);
        if rep {
            println!("repair_audio success!");
        } else {
            println!("repair_audio failure!");
        }
    }

    #[test]
    fn repair_purgeunused_test() {
        let path = "/games/hoyo/hk4e_global/live";
        let rep = Repairer::remove_unused(path.parse().unwrap(), Vec::new(), Vec::new());
        if rep {
            println!("purge_unused success!");
        } else {
            println!("purge_unused failure!");
        }
    }

    #[test]
    fn download_fullgame_test() {
        let mut urls = Vec::<String>::new();
        urls.push(String::from("https://autopatchhk.yuanshen.com/client_app/download/pc_zip/20250314110016_HcIQuDGRmsbByeAE/GenshinImpact_5.5.0.zip.001"));
        urls.push(String::from("https://autopatchhk.yuanshen.com/client_app/download/pc_zip/20250314110016_HcIQuDGRmsbByeAE/GenshinImpact_5.5.0.zip.002"));
        urls.push(String::from("https://autopatchhk.yuanshen.com/client_app/download/pc_zip/20250314110016_HcIQuDGRmsbByeAE/GenshinImpact_5.5.0.zip.003"));
        urls.push(String::from("https://autopatchhk.yuanshen.com/client_app/download/pc_zip/20250314110016_HcIQuDGRmsbByeAE/GenshinImpact_5.5.0.zip.004"));
        urls.push(String::from("https://autopatchhk.yuanshen.com/client_app/download/pc_zip/20250314110016_HcIQuDGRmsbByeAE/GenshinImpact_5.5.0.zip.005"));
        urls.push(String::from("https://autopatchhk.yuanshen.com/client_app/download/pc_zip/20250314110016_HcIQuDGRmsbByeAE/GenshinImpact_5.5.0.zip.006"));
        urls.push(String::from("https://autopatchhk.yuanshen.com/client_app/download/pc_zip/20250314110016_HcIQuDGRmsbByeAE/GenshinImpact_5.5.0.zip.007"));
        urls.push(String::from("https://autopatchhk.yuanshen.com/client_app/download/pc_zip/20250314110016_HcIQuDGRmsbByeAE/GenshinImpact_5.5.0.zip.008"));

        let path = "/games/hoyo/hk4e_global/live/testing";
        let rep = Game::download(urls, path.parse().unwrap());
        if rep {
            println!("full_game success!");
        } else {
            println!("full_game failure!");
        }
    }

    #[test]
    fn download_hdiff_test() {
        let url = "https://autopatchhk.yuanshen.com/client_app/update/hk4e_global/game_5.4.0_5.5.0_hdiff_IlvHovyEdpXnwiCH.zip";
        let path = "/games/hoyo/hk4e_global/live/testing";
        let rep = Game::patch(url.parse().unwrap(), path.parse().unwrap(), |current,total| {
            println!("current: {}, total: {}", prettify_bytes(current), prettify_bytes(total));
        });
        if rep {
            println!("diff_game success!");
        } else {
            println!("diff_game failure!");
        }
    }
}
