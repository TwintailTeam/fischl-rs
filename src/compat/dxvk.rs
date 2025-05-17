use wincompatlib::dxvk::{InstallParams};
use wincompatlib::prelude::{WineWithExt};
use wincompatlib::wine::{Wine, WineArch};
use crate::compat::Compat;

#[cfg(feature = "compat")]
impl Compat {
    pub fn add_dxvk(wine: String, prefix: String, dxvk: String, repair_dlls: bool) -> Result<bool, String> {
        let wine = Wine::from_binary(wine).with_prefix(prefix).with_arch(WineArch::Win64);
        let ip = InstallParams {
            dxgi: true,
            d3d9: true,
            d3d10core: true,
            d3d11: true,
            repair_dlls,
            arch: Default::default(),
        };

        let wp = wine.install_dxvk(dxvk, ip);
        if wp.is_ok() {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn remove_dxvk(wine: String, prefix: String) -> Result<bool, String> {
        let upd = Wine::from_binary(wine).with_arch(WineArch::Win64).with_prefix(prefix).uninstall_dxvk(InstallParams::default());
        if upd.is_ok() {
            Ok(true)
        } else {
            Ok(false)
        }
    }
}