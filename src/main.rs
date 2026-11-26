mod app;
mod editor;
mod session;
mod ssh;
mod store;
mod vault;
mod widgets;

use anyhow::{Context, Result, bail};
use gtk::{gdk, gio, glib, prelude::*};
use std::{cell::RefCell, path::PathBuf, rc::Rc};

const APP_ID: &str = "org.sshmanager.Desktop";

fn checkout() -> Option<PathBuf> {
    let binary = std::env::current_exe().ok()?;
    binary
        .ancestors()
        .skip(1)
        .take(4)
        .find(|p| p.join("Cargo.toml").is_file() && p.join("icons/hicolor").is_dir())
        .map(|p| p.to_owned())
}

fn install_launcher() -> Result<()> {
    let root = checkout().context("Run this command from the checkout build. The Debian package installs its launcher automatically.")?;
    let executable = std::env::current_exe()?;
    let path = executable
        .to_str()
        .context("The executable path must be UTF-8.")?;
    let mut quoted = String::new();
    for c in path.chars() {
        if c == '%' {
            quoted.push('%');
        }
        if matches!(c, '\\' | '"' | '`' | '$') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    let file = glib::KeyFile::new();
    file.load_from_data(
        include_str!("../packaging/org.sshmanager.Desktop.desktop"),
        glib::KeyFileFlags::NONE,
    )?;
    file.set_string(
        "Desktop Entry",
        "Exec",
        &format!("/usr/bin/env \"{quoted}\""),
    );
    file.set_string("Desktop Entry", "TryExec", path);
    let icon = root.join(format!("icons/hicolor/256x256/apps/{APP_ID}.png"));
    file.set_string(
        "Desktop Entry",
        "Icon",
        icon.to_str().context("The icon path must be UTF-8.")?,
    );
    let dir = glib::user_data_dir().join("applications");
    std::fs::create_dir_all(&dir)?;
    let target = dir.join(format!("{APP_ID}.desktop"));
    file.save_to_file(&target)?;
    println!("Registered SSH Manager at {}", target.display());
    Ok(())
}

fn run_ui(args: &[String]) -> i32 {
    glib::set_prgname(Some(APP_ID));
    glib::set_application_name("SSH Manager");
    let application = gtk::Application::new(Some(APP_ID), gio::ApplicationFlags::empty());
    application.connect_startup(|_| {
        if let Some(display) = gdk::Display::default() {
            if let Some(root) = checkout() {
                gtk::IconTheme::for_display(&display).add_search_path(root.join("icons"));
            }
            gtk::Window::set_default_icon_name(APP_ID);
            widgets::load_style(&display);
        }
    });
    let current: Rc<RefCell<Option<Rc<app::Window>>>> = Rc::default();
    let windows = current.clone();
    application.connect_activate(move |application| {
        if let Some(window) = application.active_window() {
            window.present();
            return;
        }
        match app::Window::new(application) {
            Ok(ui) => {
                ui.window.present();
                windows.replace(Some(ui));
            }
            Err(error) => {
                let dialog = gtk::MessageDialog::builder()
                    .application(application)
                    .modal(true)
                    .message_type(gtk::MessageType::Error)
                    .buttons(gtk::ButtonsType::Close)
                    .text("Could not open your host library")
                    .secondary_text(format!("{error:#}"))
                    .build();
                dialog.connect_response(|dialog, _| dialog.close());
                dialog.present();
            }
        }
    });
    let status = application.run_with_args(args);
    current.take();
    i32::from(status)
}

fn run() -> Result<i32> {
    let args: Vec<_> = std::env::args().collect();
    if std::env::var_os("SSH_MANAGER_ASKPASS").as_deref() == Some(std::ffi::OsStr::new("1"))
        && args.len() == 2
    {
        ssh::askpass(&args[1])?;
        return Ok(0);
    }
    match args.get(1).map(String::as_str) {
        Some("--session") if args.len() == 3 => ssh::run_session(&args[2])?,
        Some("--askpass") if args.len() == 3 => ssh::askpass(&args[2])?,
        Some("--install-launcher") if args.len() == 2 => install_launcher()?,
        Some("--version" | "-V") => println!("SSH Manager {} (Rust)", env!("CARGO_PKG_VERSION")),
        Some("--session" | "--askpass" | "--install-launcher") => bail!("Invalid arguments."),
        _ => return Ok(run_ui(&args)),
    }
    Ok(0)
}

fn main() {
    let code = run().unwrap_or_else(|error| {
        eprintln!("SSH Manager: {error:#}");
        1
    });
    std::process::exit(code);
}
