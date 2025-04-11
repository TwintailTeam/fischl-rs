use wincompatlib::dxvk::{InstallParams};
use wincompatlib::prelude::{WineWithExt};
use wincompatlib::wine::{Wine, WineArch};
use crate::compat::Compat;

#[cfg(feature = "compat")]
impl Compat {
    pub fn add_dxvk(wine: String, prefix: String, dxvk: String) -> Result<bool, String> {
        let wine = Wine::from_binary(wine).with_prefix(prefix).with_arch(WineArch::Win64);
        let wp = wine.install_dxvk(dxvk, InstallParams::default());
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