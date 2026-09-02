#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub use macos::{file_opens, install_open_handler, quit_requests};

#[cfg(not(target_os = "macos"))]
pub fn install_open_handler() {}

#[cfg(not(target_os = "macos"))]
pub fn file_opens() -> iced::Subscription<std::path::PathBuf> {
    // Windows and Linux deliver file associations via argv; there is no
    // separate open-file event to listen for.
    iced::Subscription::none()
}

#[cfg(not(target_os = "macos"))]
pub fn quit_requests() -> iced::Subscription<()> {
    iced::Subscription::none()
}
