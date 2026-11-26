use crate::store::{Fields, Host, InputError};
use crate::widgets::*;
use gtk::{glib, prelude::*};
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct Editor {
    pub root: gtk::Box,
    pub original: Option<Host>,
    hostname: gtk::Entry,
    port: gtk::Entry,
    username: gtk::Entry,
    name: gtk::Entry,
    group: gtk::Entry,
    key: gtk::Entry,
    password: gtk::Entry,
    vault: gtk::ToggleButton,
    error: gtk::Label,
    initial: Fields,
}

fn entry(value: &str, hint: &str) -> gtk::Entry {
    let widget = gtk::Entry::builder()
        .text(value)
        .placeholder_text(hint)
        .width_chars(7)
        .build();
    widget.add_css_class("host-input");
    widget
}

impl Editor {
    pub fn new(
        window: &gtk::ApplicationWindow,
        host: Option<Host>,
        save: impl Fn(bool) + 'static,
        cancel: Option<Box<dyn Fn()>>,
    ) -> Self {
        let initial = host.as_ref().map(Host::fields).unwrap_or_default();
        let root = column(18);
        root.append(&heading(
            if host.is_some() {
                "Edit host"
            } else {
                "Add a host"
            },
            "Use a hostname, IP address, or alias from your SSH configuration.",
        ));
        let connection = card("Connection", "network-wired-symbolic");
        root.append(&connection);
        let address = row(14);
        let hostname = entry(&initial.hostname, "server.example.com");
        let hostname_field = field("Hostname or IP address", &hostname, &hostname);
        hostname_field.set_hexpand(true);
        address.append(&hostname_field);
        let port = entry(&initial.port, "Auto (22)");
        port.set_max_length(5);
        port.set_width_chars(8);
        port.set_input_purpose(gtk::InputPurpose::Digits);
        address.append(&field("Port", &port, &port));
        connection.append(&address);
        let metadata = row(14);
        metadata.set_homogeneous(true);
        let username = entry(&initial.username, "SSH default");
        let name = entry(&initial.name, "Use hostname");
        let group = entry(&initial.group, "Ungrouped");
        for (title, item) in [
            ("Username", &username),
            ("Display name", &name),
            ("Folder", &group),
        ] {
            metadata.append(&field(title, item, item));
        }
        connection.append(&metadata);

        let auth = card("Authentication", "channel-secure-symbolic");
        let modes = row(0);
        modes.set_homogeneous(true);
        modes.add_css_class("method-picker");
        modes.add_css_class("linked");
        let method = |title, symbol| {
            let b = gtk::ToggleButton::new();
            let line = row(8);
            line.set_halign(gtk::Align::Center);
            line.append(&icon(symbol, 16));
            line.append(&label(title, ""));
            b.set_child(Some(&line));
            b.update_property(&[gtk::accessible::Property::Label(title)]);
            modes.append(&b);
            b
        };
        let standard = method("SSH key or prompt", "dialog-password-symbolic");
        let vault = method("Saved password", "changes-prevent-symbolic");
        vault.set_group(Some(&standard));
        auth.append(&modes);
        let options = gtk::Stack::builder()
            .hhomogeneous(false)
            .vhomogeneous(false)
            .build();
        let key = entry(&initial.key, "Use SSH agent or configured keys");
        key.set_hexpand(true);
        let key_line = row(8);
        key_line.append(&key);
        let parent = window.downgrade();
        let input = key.downgrade();
        key_line.append(&icon_button(
            "folder-open-symbolic",
            "Choose a private key",
            "secondary",
            move || {
                let (Some(parent), Some(input)) = (parent.upgrade(), input.upgrade()) else {
                    return;
                };
                glib::spawn_future_local(async move {
                    let chooser = gtk::FileChooserNative::new(
                        Some("Choose a private key"),
                        Some(&parent),
                        gtk::FileChooserAction::Open,
                        Some("Choose key"),
                        Some("Cancel"),
                    );
                    if chooser.run_future().await == gtk::ResponseType::Accept
                        && let Some(path) = chooser.file().and_then(|f| f.path())
                    {
                        input.set_text(&path.to_string_lossy());
                    }
                    chooser.destroy();
                });
            },
        ));
        options.add_named(&field("Private key file", &key_line, &key), Some("ssh"));
        let saved = host.as_ref().is_some_and(|h| h.secret.is_some());
        let password = entry(
            "",
            if saved {
                "Leave blank to keep the saved password"
            } else {
                "Enter SSH password"
            },
        );
        password.set_visibility(false);
        password.set_input_purpose(gtk::InputPurpose::Password);
        let passwords = column(7);
        passwords.append(&field("Password", &password, &password));
        passwords.append(&label("Saved in your Linux desktop keyring.", "muted"));
        options.add_named(&passwords, Some("password"));
        auth.append(&options);
        root.append(&auth);
        let pw = password.clone();
        vault.connect_toggled(move |v| {
            options.set_visible_child_name(if v.is_active() {
                "password"
            } else {
                pw.set_text("");
                "ssh"
            });
        });
        if saved {
            vault.set_active(true)
        } else {
            standard.set_active(true)
        }
        let error = label("", "form-error");
        error.set_wrap(true);
        error.set_visible(false);
        root.append(&error);
        let actions = row(8);
        if let Some(cancel) = cancel {
            actions.append(&button("Cancel", None, "quiet", cancel));
        }
        let space = row(0);
        space.set_hexpand(true);
        actions.append(&space);
        let save = std::rc::Rc::new(save);
        let save_only = save.clone();
        actions.append(&button("Save host", None, "secondary", move || {
            save_only(false)
        }));
        actions.append(&button(
            "Save and connect",
            Some("utilities-terminal-symbolic"),
            "primary",
            move || save(true),
        ));
        root.append(&actions);
        Self {
            root,
            original: host,
            hostname,
            port,
            username,
            name,
            group,
            key,
            password,
            vault,
            error,
            initial,
        }
    }
    pub fn use_vault(&self) -> bool {
        self.vault.is_active()
    }
    fn fields(&self) -> Fields {
        Fields {
            hostname: self.hostname.text().into(),
            port: self.port.text().into(),
            username: self.username.text().into(),
            name: self.name.text().into(),
            group: self.group.text().into(),
            key: self.key.text().into(),
        }
    }
    pub fn dirty(&self) -> bool {
        self.fields() != self.initial
            || !self.password.text().is_empty()
            || self.use_vault() != self.original.as_ref().is_some_and(|h| h.secret.is_some())
    }
    pub fn read(&self) -> anyhow::Result<Host> {
        Host::from_fields(self.fields(), self.original.as_ref(), self.use_vault())
    }
    pub fn password(&self) -> Zeroizing<String> {
        Zeroizing::new(self.password.text().to_string())
    }
    pub fn clear_password(&self) {
        self.password.set_text("");
    }
    pub fn focus(&self) {
        self.hostname.grab_focus();
    }
    pub fn show_error(&self, error: &anyhow::Error) {
        self.error.set_text(&format!("{error:#}"));
        self.error.set_visible(true);
        if let Some(input) = error.downcast_ref::<InputError>() {
            let entry = match input.0 {
                "port" => &self.port,
                "username" => &self.username,
                "key" => &self.key,
                "password" => &self.password,
                _ => &self.hostname,
            };
            entry.grab_focus();
        }
    }
}
