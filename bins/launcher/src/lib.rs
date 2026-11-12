// SPDX-License-Identifier: GPL-3.0-or-later
//! The player launcher (M2 spec section 6): logs in to a community server (password or Discord)
//! and starts New World directly under Proton with the server's launch arguments, in a private
//! network view that leads the server's Amazon names to the server ([`mirror`]).

pub mod api;
pub mod app;
/// The loopback proxy (`docs/re/2026-09-20-windows-launcher-network.md`) for a client patched
/// with `--aws-proxy`. It sends the game's Amazon calls to the server's own ports.
pub mod aws_proxy;
pub mod game;
pub mod mirror;
pub mod news;
pub mod news_html;
pub mod news_view;
pub mod options;
pub mod server;
pub mod settings;
pub mod steam;
#[cfg(test)]
mod test_server;
pub mod theme;

/// The Blighted icon (`docs/images`), 256 x 256. Windows also gets it in the program file
/// through `launcher.rc`, from the `.ico` of the same picture.
const ICON_PNG: &[u8] = include_bytes!("../../../docs/images/blighted-emu-icon-256.png");

/// The window icon: title bar and task bar.
///
/// # Errors
/// The built-in picture is not a valid PNG.
pub fn window_icon() -> Result<eframe::egui::IconData, image::ImageError> {
    eframe::icon_data::from_png_bytes(ICON_PNG)
}

/// Installs the `ring` TLS crypto provider for this process. A second call has no effect.
pub fn install_crypto_provider() {
    // Err only means a provider is already installed.
    let _ = rustls::crypto::ring::default_provider().install_default();
}

#[cfg(test)]
mod tests {
    use super::window_icon;

    const ICON_SIDE: u32 = 256;
    const RGBA_BYTES: usize = 4;

    #[test]
    fn the_window_icon_is_the_square_blighted_picture() {
        let icon = window_icon().expect("icon");
        assert_eq!((icon.width, icon.height), (ICON_SIDE, ICON_SIDE));
        assert_eq!(
            icon.rgba.len(),
            (ICON_SIDE * ICON_SIDE) as usize * RGBA_BYTES
        );
    }
}
