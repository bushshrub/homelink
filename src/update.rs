//! Looks for a newer release at startup and every 24 hours, and installs it in the background.
//! A release is only installed when its archive carries a signature that matches `update-key.pub`.
//! A failed check or a bad signature is one line on stderr; the tunnel is not touched.

use std::time::Duration;

use self_update::VerifyingKey;
use self_update::backends::github::Update;

const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
const KEY: VerifyingKey = *include_bytes!("../update-key.pub");

/// The part of the release asset name (`homelink-<target>.zip` or `.tar.gz`) that picks this platform.
const TARGET: &str = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
    "macos-arm64"
} else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
    "macos-x86_64"
} else if cfg!(all(windows, target_arch = "x86_64")) {
    "windows-x86_64"
} else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
    "linux-x86_64"
} else {
    "unsupported"
};

/// Calls `installed` with the new version once, then stops checking.
pub fn spawn(installed: impl Fn(String) + Send + 'static) {
    std::thread::spawn(move || {
        loop {
            match check() {
                Ok(Some(version)) => {
                    installed(version);
                    return;
                }
                Ok(None) => {}
                Err(err) => eprintln!("HomeLink: update check failed: {err}"),
            }
            std::thread::sleep(CHECK_EVERY);
        }
    });
}

fn check() -> self_update::errors::Result<Option<String>> {
    let mut update = Update::configure();
    update
        .repo_owner("bushshrub")
        .repo_name("homelink")
        .bin_name("homelink")
        .target(TARGET)
        .current_version(self_update::cargo_crate_version!())
        .verifying_keys([KEY])
        .show_download_progress(false)
        .show_output(false)
        .no_confirm(true);
    if cfg!(target_os = "macos") {
        // The archive holds the whole app; the running app is replaced, found from the running binary.
        update.bundle_path_in_archive("HomeLink.app");
    } else if cfg!(windows) {
        update.bin_path_in_archive("HomeLink.exe");
    } else {
        update.bin_path_in_archive("homelink");
    }
    let status = update.build()?.update()?;
    Ok(status.is_updated().then(|| status.version().to_string()))
}

#[cfg(test)]
mod tests {
    use self_update::update::{Release, ReleaseAsset};

    // The four asset names `.github/workflows/release.yml` uploads.
    const ASSETS: [&str; 4] = [
        "homelink-macos-arm64.zip",
        "homelink-macos-x86_64.zip",
        "homelink-windows-x86_64.zip",
        "homelink-linux-x86_64.tar.gz",
    ];

    #[test]
    fn target_picks_exactly_one_release_asset() {
        let release = Release::builder()
            .version("9.9.9")
            .assets(ASSETS.map(|name| ReleaseAsset::new(name, format!("https://example.invalid/{name}"))))
            .build()
            .unwrap();
        let asset = release.asset_for(super::TARGET, None).expect("this platform has an asset");
        assert!(ASSETS.contains(&asset.name()));
    }
}
