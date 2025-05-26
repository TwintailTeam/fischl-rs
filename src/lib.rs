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
    use crate::download::{Compatibility, Extras};
    use crate::download::game::{Game, Hoyo, Kuro, Sophon};
    use crate::utils::{extract_archive, prettify_bytes, KuroFile};
    use crate::utils::game::VoiceLocale;

    #[test]
    fn download_xxmi_test() {
        let dest = "/home/tukan/.local/share/twintaillauncher/extras/xxmi/testing";
        let success = Extras::download_xxmi(String::from("SpectrumQT/XXMI-Libs-Package"), dest.to_string(), true);
        if success {
            let finaldest = Path::new(&dest).join("xxmi.zip");
            let extract = extract_archive(finaldest.to_str().unwrap().to_string(), dest.to_string(), false);

            if extract {
                println!("xxmi extracted!");
            } else {
                println!("Failed to extract!");
            }
        } else {
            println!("failed to download xxmi");
        }
    }

    #[test]
    fn download_xxmi_packages_test() {
        let dest = "/home/tukan/.local/share/twintaillauncher/extras/xxmi/testing";
        let success = Extras::download_xxmi_packages(String::from("SilentNightSound/GIMI-Package"), String::from("SpectrumQT/SRMI-Package"), String::from("leotorrez/ZZMI-Package"), String::from("SpectrumQT/WWMI-Package"), dest.to_string(), false);
        if success {
            let d = Path::new(&dest);
            extract_archive(d.join("gimi.zip").to_str().unwrap().to_string(), d.join("gimi").to_str().unwrap().to_string(), false);
            extract_archive(d.join("srmi.zip").to_str().unwrap().to_string(), d.join("srmi").to_str().unwrap().to_string(), false);
            extract_archive(d.join("zzmi.zip").to_str().unwrap().to_string(), d.join("zzmi").to_str().unwrap().to_string(), false);
            extract_archive(d.join("wwmi.zip").to_str().unwrap().to_string(), d.join("wwmi").to_str().unwrap().to_string(), false);
            println!("xxmi packages extracted!")
        } else {
            println!("Failed to download xxmi packages");
        }
    }

    #[test]
    fn download_runner_test() {
        let dest = "/home/tukan/.local/share/com.keqinglauncher.app/compatibility/runners/10.4-wine-vanilla";
        let url = "https://github.com/Kron4ek/Wine-Builds/releases/download/10.4/wine-10.4-amd64.tar.xz";

        let success = Compatibility::download_runner(url.to_string(), dest.to_string());
        if success {
            let finaldest = Path::new(dest).join("runner.zip");
            let extract = extract_archive(finaldest.to_str().unwrap().to_string(), dest.to_string(), true);

            if extract {
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
        let dest = "/home/tukan/.local/share/com.keqinglauncher.app/compatibility/dxvk/2.6.0-vanilla";
        let url = "https://github.com/doitsujin/dxvk/releases/download/v2.6/dxvk-2.6.tar.gz";

        let success = Compatibility::download_dxvk(url.to_string(), dest.to_string());
        if success {
            let finaldest = Path::new(dest).join("dxvk.zip");
            let extract = extract_archive(finaldest.to_str().unwrap().to_string(), dest.to_string(), true);

            if extract {
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
        let dest = "/home/tukan/.local/share/com.keqinglauncher.app/extras/fps_unlock/testing";

        let success = Extras::download_fps_unlock(String::from("mkrsym1/fpsunlock"), dest.to_string());
        if success {
            println!("fps unlock downloaded!")
        } else {
            println!("failed to download fpsunlock");
        }
    }

    #[test]
    fn download_jadeite_test() {
        let dest = "/home/tukan/.local/share/com.keqinglauncher.app/extras/jadeite/testing";

        let success = Extras::download_jadeite(String::from("mkrsym1/jadeite"), dest.to_string());
        if success {
            let finaldest = Path::new(dest).join("jadeite.zip");
            let extract = extract_archive(finaldest.to_str().unwrap().to_string(), dest.to_string(), false);

            if extract {
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
        let rep = <Game as Hoyo>::repair_game(res_list, path.parse().unwrap(), false, |_, _| {});
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

        let rep = <Game as Hoyo>::repair_audio(res_list, VoiceLocale::English.to_folder().to_string(), path.parse().unwrap(), false, |_, _| {});
        if rep {
            println!("repair_audio success!");
        } else {
            println!("repair_audio failure!");
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
        urls.push(String::from("https://autopatchhk.yuanshen.com/client_app/download/pc_zip/20250314110016_HcIQuDGRmsbByeAE/Audio_English(US)_5.5.0.zip"));

        let path = "/games/hoyo/hk4e_global/live/testing";
        let rep = <Game as Hoyo>::download(urls, path.parse().unwrap(), |current, total| {
            println!("current: {} | total: {}", current, total);
        });
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
        let rep = <Game as Hoyo>::patch(url.parse().unwrap(), path.parse().unwrap(), |current,total| {
            println!("current: {}, total: {}", prettify_bytes(current), prettify_bytes(total));
        });
        if rep {
            println!("diff_game success!");
        } else {
            println!("diff_game failure!");
        }
    }

    // WARNING: Repair game test will take A REALLY LONG time!
    #[test]
    fn repair_game_kuro_test() {
        let index = String::from("https://hw-pcdownload-aws.aki-game.net/launcher/game/G153/2.2.1/jtXpViyIFgkkhKwyeqWwjoZhEBuXONiu/resource/50004/2.2.1/indexFile.json");
        let res_list = String::from("https://hw-pcdownload-aws.aki-game.net/launcher/game/G153/2.2.1/jtXpViyIFgkkhKwyeqWwjoZhEBuXONiu/zip");

        let path = "/games/kuro/wuwa_global/live";
        let rep = <Game as Kuro>::repair_game(index, res_list, path.parse().unwrap(), false, |_, _| {});
        if rep {
            println!("repair_game_kuro success!");
        } else {
            println!("repair_game_kuro failure!");
        }
    }
    #[test]
    fn download_fullgame_kuro_test() {
        let mut urls = Vec::<KuroFile>::new();
        urls.push(KuroFile { url: "https://hw-pcdownload-aws.aki-game.net/launcher/game/G153/2.2.0/onnOqcAkPIKgfEoFdwJcgRzLRNLohWAm/zip/ClearThirdParty.exe".to_string(), path: "ClearThirdParty.exe".to_string(), hash: "17678258f163d0a7d9413b633f5929ba".to_string(), size: "223032".to_string() });
        urls.push(KuroFile { url: "https://hw-pcdownload-aws.aki-game.net/launcher/game/G153/2.2.0/onnOqcAkPIKgfEoFdwJcgRzLRNLohWAm/zip/Client/Config/AMD.json".to_string(), path: "Client/Config/AMD.json".to_string(), hash: "16450068a58d20d2057e0ecfcefc55dd".to_string(), size: "6".to_string() });
        urls.push(KuroFile { url: "https://hw-pcdownload-aws.aki-game.net/launcher/game/G153/2.2.0/onnOqcAkPIKgfEoFdwJcgRzLRNLohWAm/zip/Client/Content/Paks/pakchunk0-WindowsNoEditor.pak".to_string(), path: "Client/Content/Paks/pakchunk0-WindowsNoEditor.pak".to_string(), hash: "d53bc50f56b44d8c6bb72895a9d4d158".to_string(), size: "1074693423".to_string() });

        let path = "/games/kuro/wuwa_global/live/testing";
        let rep = <Game as Kuro>::download(urls, path.parse().unwrap(), |_, _| {});
        if rep {
            println!("full_game_kuro success!");
        } else {
            println!("full_game_kuro failure!");
        }
    }

    #[test]
    fn download_hdiff_kuro_test() {
        //let url = "https://hw-pcdownload-aws.aki-game.net/launcher/game/G153/2.2.0/onnOqcAkPIKgfEoFdwJcgRzLRNLohWAm/resource/50004/2.2.0/2.1.0/resources/2.1.0_2.2.0_1742208942208.krdiff";
        let url = "https://hw-pcdownload-aws.aki-game.net/launcher/game/G153/2.2.0/onnOqcAkPIKgfEoFdwJcgRzLRNLohWAm/resource/50004/2.2.0/2.1.1/resources/2.1.1_2.2.0_1742202752561.krdiff";

        let path = "/games/kuro/wuwa_global/live/testing";
        let rep = <Game as Kuro>::patch(url.parse().unwrap(), path.parse().unwrap(), |current,total| {
            println!("current: {}, total: {}", prettify_bytes(current), prettify_bytes(total));
        });
        if rep {
            println!("diff_game_kuro success!");
        } else {
            println!("diff_game_kuro failure!");
        }
    }

    // Sophon
    #[tokio::test]
    async fn download_fullgame_hoyo_sophon_test() {
        let manifest = "https://autopatchhk.yuanshen.com/client_app/sophon/manifests/cxhpq4g4rgg0/q3h361jUEuu0/manifest_6194a90dbfdae455_f2f91f7cf5f869009f4816d8489f66ca";
        let chunkurl = "https://autopatchhk.yuanshen.com/client_app/sophon/chunks/cxhpq4g4rgg0/q3h361jUEuu0";
        let path = "/games/hoyo/hk4e_global/live/testing";

        let rep = <Game as Sophon>::download(manifest.to_string(), chunkurl.to_string(), path.parse().unwrap(), |current, total| {
            println!("current: {} | total: {}", current, total)
        }).await;
        if rep {
            println!("full_game_sophon success!");
        } else {
            println!("full_game_sophon failure!");
        }
    }

    #[tokio::test]
    async fn download_diff_hoyo_sophon_test() {
        let manifest = "https://autopatchhk.yuanshen.com/client_app/sophon/manifests/cxhpq4g4rgg0/SHvGg74waz43/manifest_4260c944120924aa_f8e484154edc19122cac61fe10c83640";
        let chunkurl = "https://autopatchhk.yuanshen.com/client_app/sophon/diffs/cxhpq4g4rgg0/SHvGg74waz43/10016";
        let path = "/games/hoyo/hk4e_global/live/testing";

        let rep = <Game as Sophon>::patch(manifest.to_string(), "5.5.0".to_string(), chunkurl.to_string(), path.parse().unwrap(), |current, total| {
            println!("current: {} | total: {}", current, total)
        }).await;
        if rep {
            println!("diff_game_sophon success!");
        } else {
            println!("diff_game_sophon failure!");
        }
    }

    #[tokio::test]
    async fn repair_hoyo_sophon_test() {
        let manifest = "https://autopatchhk.yuanshen.com/client_app/sophon/manifests/cxhpq4g4rgg0/q3h361jUEuu0/manifest_6194a90dbfdae455_f2f91f7cf5f869009f4816d8489f66ca";
        let chunkurl = "https://autopatchhk.yuanshen.com/client_app/sophon/chunks/cxhpq4g4rgg0/q3h361jUEuu0";
        let path = "/games/hoyo/hk4e_global/live/testing";

        let rep = <Game as Sophon>::repair_game(manifest.to_string(), chunkurl.to_string(), path.parse().unwrap(), true,|current, total| {
            println!("current: {} | total: {}", current, total)
        }).await;
        if rep {
            println!("repair_game_sophon success!");
        } else {
            println!("repair_game_sophon failure!");
        }
    }
}
