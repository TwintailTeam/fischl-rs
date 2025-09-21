#[cfg(target_os = "linux")]
pub mod compat;
pub mod utils;
pub mod download;

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::download::{Extras};
    use crate::download::game::{Game, Kuro, Sophon, Zipped};
    use crate::utils::{extract_archive, prettify_bytes};

    #[test]
    fn download_xxmi_test() {
        let dest = "/home/tukan/.local/share/twintaillauncher/extras/xxmi/testing";
        let success = Extras::download_xxmi(String::from("SpectrumQT/XXMI-Libs-Package"), dest.to_string(), true, move |current, total| {
            println!("current: {}, total: {}", current, total);
        });
        if success {
            let finaldest = Path::new(&dest).join("xxmi.zip");
            let extract = extract_archive("".to_owned(), finaldest.to_str().unwrap().to_string(), dest.to_string(), false);

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
        let success = Extras::download_xxmi_packages(String::from("SilentNightSound/GIMI-Package"), String::from("SpectrumQT/SRMI-Package"), String::from("leotorrez/ZZMI-Package"), String::from("SpectrumQT/WWMI-Package"), String::from("leotorrez/HIMI-Package"), dest.to_string());
        if success {
            let d = Path::new(&dest);
            extract_archive("".to_owned(), d.join("gimi.zip").to_str().unwrap().to_string(), d.join("gimi").to_str().unwrap().to_string(), false);
            extract_archive("".to_owned(), d.join("srmi.zip").to_str().unwrap().to_string(), d.join("srmi").to_str().unwrap().to_string(), false);
            extract_archive("".to_owned(), d.join("zzmi.zip").to_str().unwrap().to_string(), d.join("zzmi").to_str().unwrap().to_string(), false);
            extract_archive("".to_owned(), d.join("wwmi.zip").to_str().unwrap().to_string(), d.join("wwmi").to_str().unwrap().to_string(), false);
            extract_archive("".to_owned(), d.join("himi.zip").to_str().unwrap().to_string(), d.join("himi").to_str().unwrap().to_string(), false);
            println!("xxmi packages extracted!")
        } else {
            println!("Failed to download xxmi packages");
        }
    }

    #[test]
    fn download_fpsunlock_test() {
        let dest = "/home/tukan/.local/share/twintaillauncher/extras/fps_unlock/testing";

        let success = Extras::download_fps_unlock(String::from("TwintailTeam/KeqingUnlock"), dest.to_string(), move |current, total| {
            println!("current: {}, total: {}", current, total);
        });
        if success {
            println!("fps unlock downloaded!")
        } else {
            println!("failed to download fpsunlock");
        }
    }

    #[test]
    fn download_jadeite_test() {
        let dest = "/home/tukan/.local/share/twintaillauncher/extras/jadeite/testing";

        let success = Extras::download_jadeite(String::from("mkrsym1/jadeite"), dest.to_string(), move |current, total| {
            println!("current: {}, total: {}", current, total);
        });
        if success {
            let finaldest = Path::new(dest).join("jadeite.zip");
            let extract = extract_archive("".to_owned(), finaldest.to_str().unwrap().to_string(), dest.to_string(), false);

            if extract {
                println!("jadeite extracted!")
            } else {
                println!("Failed to extract!");
            }
        } else {
            println!("failed to download jadeite");
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
        let rep = <Game as Zipped>::download(urls, path.parse().unwrap(), |current, total| {
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
        let rep = <Game as Zipped>::patch(url.parse().unwrap(), path.parse().unwrap(), |current,total| {
            println!("current: {}, total: {}", prettify_bytes(current), prettify_bytes(total));
        });
        if rep {
            println!("diff_game success!");
        } else {
            println!("diff_game failure!");
        }
    }

    #[test]
    fn repair_game_test() {
        let res_list = String::from("https://autopatchhk.yuanshen.com/client_app/download/pc_zip/20250314110016_HcIQuDGRmsbByeAE/ScatteredFiles");
        let path = "/games/hoyo/hk4e_global/live";
        let rep = <Game as Zipped>::repair_game(res_list, path.parse().unwrap(), false, |_, _| {});
        if rep {
            println!("repair_game success!");
        } else {
            println!("repair_game failure!");
        }
    }

    // KuroGame
    #[tokio::test]
    async fn download_fullgame_kuro_test() {
        let manifest = "https://zspms-alicdn-gamestarter.kurogame.net/launcher/game/G143/50015/3.8.0/fQwueYbrJvAInbczEGotEDqvDPbdCklY/resource/50015/3.8.0/indexFile.json";
        let chunkurl = "https://zspms-alicdn-gamestarter.kurogame.net/launcher/game/G143/50015/3.8.0/fQwueYbrJvAInbczEGotEDqvDPbdCklY/zip";

        //let manifest = "https://hw-pcdownload-aws.aki-game.net/launcher/game/G153/50004/2.6.2/TmryWWDzYshLRahsXoGizseCUnInEDtj/resource/50004/2.6.2/indexFile.json";
        //let chunkurl = "https://hw-pcdownload-aws.aki-game.net/launcher/game/G153/50004/2.6.2/TmryWWDzYshLRahsXoGizseCUnInEDtj/zip";

        let path = "/games/kuro/wuwa_global/testing";
        let rep = <Game as Kuro>::download(manifest.to_string(), chunkurl.to_string(), path.parse().unwrap(), |current, total| {
            println!("current: {} | total: {}", current, total)
        }).await;
        if rep {
            println!("full_game_kuro success!");
        } else {
            println!("full_game_kuro failure!");
        }
    }

    #[tokio::test]
    async fn download_diff_kuro_test() {
        let manifest = "https://zspms-alicdn-gamestarter.kurogame.net/launcher/game/G143/50015/3.8.0/fQwueYbrJvAInbczEGotEDqvDPbdCklY/resource/50015/3.8.0/3.5.0/indexFile.json";
        let chunk_res = "https://zspms-alicdn-gamestarter.kurogame.net/launcher/game/G143/50015/3.8.0/fQwueYbrJvAInbczEGotEDqvDPbdCklY/resource/50015/3.8.0/3.5.0/resources/";
        let chunk_zip = "https://zspms-alicdn-gamestarter.kurogame.net/launcher/game/G143/50015/3.8.0/fQwueYbrJvAInbczEGotEDqvDPbdCklY/zip";

        let path = "/games/kuro/wuwa_global/testing";
        let krdiffbin = "/home/tukan/Desktop/Programming/rust/workspace/KeqingLauncher/src-tauri/resources/krpatchz".to_string();
        let rep = <Game as Kuro>::patch(manifest.to_string(), chunk_res.to_string(), chunk_zip.to_string(), path.parse().unwrap(), krdiffbin, false, |current,total| {
            println!("current: {}, total: {}", current, total);
        }).await;
        if rep {
            println!("diff_game_kuro success!");
        } else {
            println!("diff_game_kuro failure!");
        }
    }

    #[tokio::test]
    async fn repair_game_kuro_test() {
        let manifest = "https://zspms-alicdn-gamestarter.kurogame.net/launcher/game/G143/50015/3.8.0/fQwueYbrJvAInbczEGotEDqvDPbdCklY/resource/50015/3.8.0/indexFile.json";
        let chunkurl = "https://zspms-alicdn-gamestarter.kurogame.net/launcher/game/G143/50015/3.8.0/fQwueYbrJvAInbczEGotEDqvDPbdCklY/zip";

        //let manifest = "https://hw-pcdownload-aws.aki-game.net/launcher/game/G153/50004/2.5.1/IYOwHBLfeAXMxVgHwGybWvvSqiDnPlbs/resource/50004/2.5.1/indexFile.json";
        //let chunkurl = "https://hw-pcdownload-aws.aki-game.net/launcher/game/G153/50004/2.5.1/IYOwHBLfeAXMxVgHwGybWvvSqiDnPlbs/zip";

        let path = "/games/kuro/wuwa_global/testing";
        let rep = <Game as Kuro>::repair_game(manifest.to_string(), chunkurl.to_string(), path.parse().unwrap(), false, |_, _| {}).await;
        if rep {
            println!("repair_game_kuro success!");
        } else {
            println!("repair_game_kuro failure!");
        }
    }

    #[tokio::test]
    async fn download_preload_kuro_test() {
        let manifest = "https://zspms-alicdn-gamestarter.kurogame.net/launcher/game/G143/50015/3.8.0/fQwueYbrJvAInbczEGotEDqvDPbdCklY/resource/50015/3.8.0/3.5.0/indexFile.json";
        let chunkurl_res = "https://zspms-alicdn-gamestarter.kurogame.net/launcher/game/G143/50015/3.8.0/fQwueYbrJvAInbczEGotEDqvDPbdCklY/resource/50015/3.8.0/3.5.0/resources/";
        let chunkurl_zip = "https://zspms-alicdn-gamestarter.kurogame.net/launcher/game/G143/50015/3.8.0/fQwueYbrJvAInbczEGotEDqvDPbdCklY/zip";

        /*let manifest = "https://hw-pcdownload-aws.aki-game.net/launcher/game/G153/50004/2.6.1/hjetrIFhEnolsFvBMHVnqaASBoWlvNNx/resource/50004/2.6.1/2.5.1/indexFile.json";
        let chunkurl_res = "https://hw-pcdownload-aws.aki-game.net/launcher/game/G153/50004/2.6.0/vjRSVRVUcliHOxTgAIIBaYdozsxSvujT/resource/50004/2.6.0/2.5.1/resources/";
        let chunkurl_zip = "https://hw-pcdownload-aws.aki-game.net/launcher/game/G153/50004/2.6.1/hjetrIFhEnolsFvBMHVnqaASBoWlvNNx/zip";*/

        let path = "/games/kuro/wuwa_global/testing";
        let rep = <Game as Kuro>::preload(manifest.to_string(), chunkurl_res.to_string(), chunkurl_zip.to_string(), path.parse().unwrap(),|current,total| {
            println!("current: {}, total: {}", current, total);
        }).await;
        if rep {
            println!("preload_game_kuro success!");
        } else {
            println!("preload_game_kuro failure!");
        }
    }

    // Sophon
    #[tokio::test]
    async fn download_fullgame_hoyo_sophon_test() {
        let manifest = "https://autopatchhk.yuanshen.com/client_app/sophon/manifests/cxhpq4g4rgg0/d6lo39gA1LIA/manifest_5c3b9426a2f209d1_3934c9f0cc2b5ef1b468bad832be87bb";
        let chunkurl = "https://autopatchhk.yuanshen.com/client_app/sophon/chunks/cxhpq4g4rgg0/d6lo39gA1LIA";
        let path = "/games/hoyo/hk4e_global/testing";

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
        let manifest = "https://autopatchhk.yuanshen.com/client_app/sophon/manifests/cxhpq4g4rgg0/DphJOTQP5dDn/manifest_68690fa573e2b399_07f4d65a4f4d42157f863d7d177efaa4";
        let chunkurl = "https://autopatchhk.yuanshen.com/client_app/sophon/diffs/cxhpq4g4rgg0/DphJOTQP5dDn/10016";
        let path = "/games/hoyo/hk4e_global/testing";

        let rep = <Game as Sophon>::patch(manifest.to_string(), "5.6.0".to_string(), chunkurl.to_string(), path.parse().unwrap(), "".to_string(), true, false, |current, total| {
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
        let manifest = "https://autopatchhk.yuanshen.com/client_app/sophon/manifests/cxhpq4g4rgg0/d6lo39gA1LIA/manifest_5c3b9426a2f209d1_3934c9f0cc2b5ef1b468bad832be87bb";
        let chunkurl = "https://autopatchhk.yuanshen.com/client_app/sophon/chunks/cxhpq4g4rgg0/d6lo39gA1LIA";
        let path = "/games/hoyo/hk4e_global/testing";

        let rep = <Game as Sophon>::repair_game(manifest.to_string(), chunkurl.to_string(), path.parse().unwrap(), false,|current, total| {
            println!("current: {} | total: {}", current, total)
        }).await;
        if rep {
            println!("repair_game_sophon success!");
        } else {
            println!("repair_game_sophon failure!");
        }
    }

    #[tokio::test]
    async fn download_preload_hoyo_sophon_test() {
        let manifest = "https://autopatchhk.yuanshen.com/client_app/sophon/manifests/cxhpq4g4rgg0/DphJOTQP5dDn/manifest_68690fa573e2b399_07f4d65a4f4d42157f863d7d177efaa4";
        let chunkurl = "https://autopatchhk.yuanshen.com/client_app/sophon/diffs/cxhpq4g4rgg0/DphJOTQP5dDn/10016";
        let path = "/games/hoyo/hk4e_global/testing";

        let rep = <Game as Sophon>::preload(manifest.to_string(), "5.6.0".to_string(), chunkurl.to_string(), path.parse().unwrap(), |current, total| {
            println!("current: {} | total: {}", current, total)
        }).await;
        if rep {
            println!("preload_game_sophon success!");
        } else {
            println!("preload_game_sophon failure!");
        }
    }
}
