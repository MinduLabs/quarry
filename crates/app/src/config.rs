//! Hằng số build. App ID phải khớp với tên file trong `data/` và manifest Flatpak.

pub const APP_ID: &str = "io.github.mindulabs.Quarry";
pub const APP_NAME: &str = "Quarry";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const GETTEXT_PACKAGE: &str = "quarry";
pub const LOCALEDIR: &str = match option_env!("QUARRY_LOCALEDIR") {
    Some(dir) => dir,
    None => "/app/share/locale",
};
pub const WEBSITE: &str = "https://github.com/MinduLabs/quarry";
pub const ISSUE_URL: &str = "https://github.com/MinduLabs/quarry/issues";
