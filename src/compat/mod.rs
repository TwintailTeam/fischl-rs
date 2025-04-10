#[cfg(feature = "compat")]
use wincompatlib::wine::Wine;

#[cfg(feature = "compat")]
pub mod prefix;
#[cfg(feature = "compat")]
pub mod dxvk;

#[cfg(feature = "compat")]
pub struct Compat {
    pub wine: Wine,
}