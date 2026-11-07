//! The token and gateway live in the OS keychain; start-at-login lives in the OS login items.

use anyhow::{Context, Result};
use auto_launch::{AutoLaunch, AutoLaunchBuilder};
use keyring::Entry;

use crate::tunnel::DEFAULT_GATEWAY;

const SERVICE: &str = "org.derpz.homelink";
const APP_NAME: &str = "HomeLink";

pub struct Saved {
    pub token: String,
    pub gateway: String,
}

fn entry(name: &str) -> keyring::Result<Entry> {
    Entry::new(SERVICE, name)
}

fn read(name: &str) -> Option<String> {
    entry(name).ok()?.get_password().ok()
}

pub fn load() -> Saved {
    Saved {
        token: read("token").unwrap_or_default(),
        gateway: read("gateway").unwrap_or_else(|| DEFAULT_GATEWAY.to_string()),
    }
}

/// Best effort: a keychain that refuses means the user pastes the token again next time.
pub fn save(token: &str, gateway: &str) {
    for (name, value) in [("token", token), ("gateway", gateway)] {
        if let Ok(entry) = entry(name) {
            let _ = entry.set_password(value);
        }
    }
}

fn login_item() -> Result<AutoLaunch> {
    let exe = std::env::current_exe().context("cannot find the app's own path")?;
    AutoLaunchBuilder::new()
        .set_app_name(APP_NAME)
        .set_app_path(&exe.to_string_lossy())
        .set_macos_launch_mode(auto_launch::MacOSLaunchMode::LaunchAgent)
        .build()
        .context("cannot set up the login item")
}

pub fn start_at_login() -> bool {
    login_item().and_then(|item| item.is_enabled().context("cannot read the login item")).unwrap_or(false)
}

pub fn set_start_at_login(on: bool) -> Result<()> {
    let item = login_item()?;
    if on { item.enable() } else { item.disable() }.context("the system refused")
}
