//! macOS updates that replace `Torno.app` where it runs and restart, the way the other Nodalia
//! apps do, through the shared `nodalia-update` crate.
//!
//! The update window, the release feed and the SHA-256 and signature checks against the release's
//! signed list stay Torno's (`torno_ui_egui::update`). Only two steps come from the shared crate:
//! keeping the verified disk image ([`stage_replacement`]) and swapping the bundle in as Torno
//! quits ([`replace_bundle`]), which also checks that the app inside is signed by the same
//! publisher as the installed one and puts the old copy back if the new one doesn't start.

use std::path::{Path, PathBuf};

use torno_ui_egui::update::FetchError;
use torno_ui_egui::update::install::StageReplacement;

/// The app as `nodalia-update` knows it.
const APP: nodalia_update::App =
    nodalia_update::App { name: "Torno", repo: "getnodalia/torno", bundle: "Torno.app", version: env!("CARGO_PKG_VERSION"), edition: None };

/// The in-place replacement's download, when this copy of Torno can be replaced where it runs:
/// a macOS bundle in a folder the user can write, not run from a disk image or a translocated
/// copy. Otherwise None, and the update opens the disk image for the user to drag, as before.
pub fn stage_replacement() -> Option<StageReplacement> {
    if !cfg!(target_os = "macos") || !nodalia_update::install_kind(&APP).can_apply_in_place() {
        return None;
    }
    Some(std::sync::Arc::new(|name, url, size, sha256, progress| {
        // The digest came from the signed list the window already checked; `download` keeps the
        // image only when it matches, and records it for the swap.
        let release = nodalia_update::Release {
            tag: String::new(),
            version: String::new(),
            notes: String::new(),
            page: String::new(),
            asset: Some(nodalia_update::ReleaseAsset {
                name: name.to_string(),
                url: url.to_string(),
                sha256: Some(sha256.to_ascii_lowercase()),
                size: Some(size),
            }),
        };
        nodalia_update::download(&APP, &release, |done, total| {
            progress(done, total);
        })
        .map_err(|e| FetchError::Failed(e.describe(nodalia_update::Language::English)))
    }))
}

/// As Torno quits: hand `package` (the verified disk image) to the helper that waits for this
/// process to end, replaces `Torno.app` and starts the new version. If the swap can't be set up,
/// the disk image is opened instead so the person can still drag the app over by hand.
pub fn replace_bundle(package: &Path) {
    if let Err(error) = nodalia_update::apply(&APP, package, || Ok(())) {
        log::error!("update: {}", error.describe(nodalia_update::Language::English));
        open_disk_image(package);
    }
}

fn open_disk_image(package: &Path) {
    let image: PathBuf = package.to_path_buf();
    if let Err(e) = std::process::Command::new("open").arg(&image).spawn() {
        log::error!("update: opening {}: {e}", image.display());
    }
}
