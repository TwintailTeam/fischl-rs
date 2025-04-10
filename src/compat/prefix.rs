use wincompatlib::prelude::{WineArch, WineBootExt, WineWithExt};
use wincompatlib::wine::Wine;
use crate::compat::Compat;

#[cfg(feature = "compat")]
impl Compat {
    pub fn setup_prefix(wine: String, prefix: String) -> Result<Self, String> {
        let wine = Wine::from_binary(wine).with_prefix(prefix).with_arch(WineArch::Win64);
        let wp = wine.init_prefix(None::<&str>);
        if wp.is_ok() {
            Ok(Compat { wine })
        } else {
            Err("Failed to create wine prefix!".into())
        }
    }

    pub fn update_prefix(wine: String, prefix: String) -> Result<bool, String> {
        let upd = Wine::from_binary(wine).with_arch(WineArch::Win64).update_prefix(Some(prefix));
        if upd.is_ok() {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn end_session(wine: String, prefix: String) -> Result<bool, String> {
        let session = Wine::from_binary(wine).with_arch(WineArch::Win64).with_prefix(prefix).end_session();
        if session.is_ok() {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn shutdown(wine: String, prefix: String) -> Result<bool, String> {
        let session = Wine::from_binary(wine).with_arch(WineArch::Win64).with_prefix(prefix).shutdown();
        if session.is_ok() {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn reboot(wine: String, prefix: String) -> Result<bool, String> {
        let session = Wine::from_binary(wine).with_arch(WineArch::Win64).with_prefix(prefix).restart();
        if session.is_ok() {
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
