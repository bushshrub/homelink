# HomeLink

HomeLink is a small desktop app (Rust and Slint, no web view). It lets a service you use send its web traffic
through your home internet connection, so that traffic comes from your own IP address. HomeLink connects out to the
service's gateway, so you open no ports and change no router settings.

## Use

1. On the service's "Home connection" page, create a code. It looks like `<name>:<secret>`.
2. Start HomeLink, paste the code and press Connect. The code is kept in your operating system's keychain.
3. Switch on "Start at login" if you want HomeLink to connect again after a restart.
4. Press Disconnect, or close the window, to stop. Nothing is routed while HomeLink is closed.

The gateway field is filled in already (`wss://jaa-net.derpz.org`); change it only if the service tells you to.

## Download

Builds are on the Releases page for macOS (Apple silicon and Intel), Windows and Linux. They are not signed yet, so
the system warns on first start:

- macOS: unzip, right-click `HomeLink.app`, choose Open, then Open in the dialog.
- Windows: SmartScreen shows "Windows protected your PC". Choose More info, then Run anyway.
- Linux: unpack the archive and run `./homelink`. The keychain needs a running Secret Service (GNOME Keyring or KWallet).

## Updates

HomeLink looks for a newer release when it starts and every 24 hours, downloads it in the background and installs it,
then shows "Updated to X.Y.Z: restart to finish". It installs only an archive signed with the release key, whose
public half is `update-key.pub` in this repository (embedded in the app); an archive that fails the check is
ignored. The check never interrupts the tunnel. To verify a download by hand, install
[zipsign](https://crates.io/crates/zipsign) (`cargo install zipsign --version 0.2.2 --locked`) and run
`zipsign verify zip homelink-macos-arm64.zip update-key.pub` (`zipsign verify tar` for the Linux `.tar.gz`). The
signature is bound to the archive's file name, so verify the file under the name it has on the Releases page.

## Build from source

Rust 1.88 or newer. The release binary lands in `target/release/homelink`.

- macOS: `xcode-select --install`, then `cargo build --release`.
- Windows: Visual Studio Build Tools (C++ workload), CMake and NASM on PATH (needed by aws-lc), then `cargo build --release`.
- Linux: `sudo apt install build-essential cmake pkg-config libdbus-1-dev libfontconfig1-dev libxkbcommon-dev libwayland-dev libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev`, then `cargo build --release`.

`cargo clippy -- -D warnings` and `cargo test` run on any of them. Build each target on its own OS, because aws-lc
needs that OS's C toolchain.

## License

MIT, see `LICENSE`. The embedded [wstunnel](https://github.com/erebe/wstunnel) client is BSD 3-Clause, see
`THIRD_PARTY_NOTICES.md`.
