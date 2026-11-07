//! HomeLink: one small window that keeps a reverse SOCKS5 tunnel open to the gateway.
//! Closing the window quits the app (a tray icon does not run cleanly beside Slint's event loop on Linux).

#![cfg_attr(windows, windows_subsystem = "windows")]

mod settings;
mod tunnel;

use std::cell::RefCell;
use std::rc::Rc;

use tunnel::Status;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let ui = MainWindow::new()?;
    let saved = settings::load();
    ui.set_token(saved.token.into());
    ui.set_gateway(saved.gateway.into());
    ui.set_status_text("Not connected".into());
    ui.set_start_at_login(settings::start_at_login());

    // Status comes from wstunnel's own events, on other threads.
    let weak = ui.as_weak();
    tunnel::watch(move |status| {
        let _ = weak.upgrade_in_event_loop(move |ui| {
            if ui.get_phase() != Phase::Disconnected {
                show(&ui, &status);
            }
        });
    });

    let running: Rc<RefCell<Option<tunnel::Tunnel>>> = Rc::default();

    ui.on_connect({
        let weak = ui.as_weak();
        let running = running.clone();
        move || {
            let ui = weak.unwrap();
            let (token, gateway) = (ui.get_token().to_string(), ui.get_gateway().to_string());
            match tunnel::start(&gateway, &token) {
                Ok(started) => {
                    *running.borrow_mut() = Some(started);
                    settings::save(&token, &gateway);
                    show(&ui, &Status::Connecting);
                }
                Err(err) => {
                    ui.set_error_text(err.to_string().into());
                }
            }
        }
    });

    ui.on_disconnect({
        let weak = ui.as_weak();
        let running = running.clone();
        move || {
            running.borrow_mut().take();
            let ui = weak.unwrap();
            ui.set_phase(Phase::Disconnected);
            ui.set_status_text("Not connected".into());
            ui.set_error_text("".into());
        }
    });

    ui.on_login_toggled({
        let weak = ui.as_weak();
        move |on| {
            if let Err(err) = settings::set_start_at_login(on) {
                let ui = weak.unwrap();
                ui.set_start_at_login(!on);
                ui.set_error_text(format!("Could not change start at login: {err}").into());
            }
        }
    });

    ui.run()
}

fn show(ui: &MainWindow, status: &Status) {
    match status {
        Status::Connecting => {
            ui.set_phase(Phase::Connecting);
            ui.set_status_text("Connecting…".into());
            ui.set_error_text("".into());
        }
        Status::Connected => {
            ui.set_phase(Phase::Connected);
            ui.set_status_text("Connected".into());
            ui.set_error_text("".into());
        }
        Status::Retrying(why) => {
            ui.set_phase(Phase::Connecting);
            ui.set_status_text("Connecting…".into());
            ui.set_error_text(why.into());
        }
    }
}
