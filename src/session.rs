use crate::{store::Host, widgets::*};
use gtk::{gdk, gio, glib, prelude::*};
use std::{
    cell::{Cell, RefCell},
    os::unix::process::ExitStatusExt,
    process::ExitStatus,
    rc::Rc,
};
use vte::prelude::*;

pub struct Session {
    pub root: gtk::Box,
    pub tab: gtk::Box,
    pub close: gtk::Button,
    pub retry: gtk::Button,
    pub terminal: vte::Terminal,
    pub id: String,
    pub title: gtk::Label,
    address: gtk::Label,
    state: gtk::Label,
    pub active: Cell<bool>,
    closed: Cell<bool>,
    pid: Cell<Option<i32>>,
    spawning: RefCell<Option<gio::Cancellable>>,
}

impl Session {
    pub fn new(host: &Host) -> Rc<Self> {
        let root = column(0);
        let bar = row(10);
        bar.add_css_class("session-bar");
        bar.append(&icon("network-server-symbolic", 16));
        let address = label(&host.destination(), "mono");
        address.set_hexpand(true);
        address.set_selectable(true);
        address.set_ellipsize(gtk::pango::EllipsizeMode::End);
        bar.append(&address);
        let state = label("", "session-state");
        bar.append(&state);
        let retry = gtk::Button::with_label("Reconnect");
        retry.add_css_class("secondary");
        retry.set_visible(false);
        bar.append(&retry);
        root.append(&bar);
        let terminal = vte::Terminal::new();
        terminal.set_font(Some(&gtk::pango::FontDescription::from_string(
            "Monospace 11",
        )));
        terminal.set_scrollback_lines(10_000);
        terminal.set_scroll_on_keystroke(true);
        terminal.set_scroll_on_output(false);
        terminal.set_mouse_autohide(true);
        terminal.set_audible_bell(false);
        terminal.set_size(48, 12);
        let color = |s: &str| gdk::RGBA::parse(s).expect("valid terminal palette");
        let palette = [
            "#202a31", "#ee8180", "#79c79b", "#ddbf7e", "#82adde", "#b19bd5", "#74c4c3", "#dce4e7",
            "#76868f", "#ff9b97", "#a0dcb1", "#f1d393", "#a6c8ee", "#ceb2eb", "#97d8d7", "#ffffff",
        ]
        .map(color);
        terminal.set_colors(
            Some(&color("#e5edef")),
            Some(&color("#152129")),
            &palette.iter().collect::<Vec<_>>(),
        );
        terminal.set_color_cursor(Some(&color("#8cd5b4")));
        terminal.set_hexpand(true);
        terminal.set_vexpand(true);
        let pane = row(0);
        pane.add_css_class("terminal-pane");
        pane.append(&terminal);
        pane.append(&gtk::Scrollbar::new(
            gtk::Orientation::Vertical,
            terminal.vadjustment().as_ref(),
        ));
        root.append(&pane);
        let tab = row(6);
        tab.append(&icon("utilities-terminal-symbolic", 15));
        let title = label(host.title(), "");
        title.set_max_width_chars(19);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        tab.append(&title);
        let close = gtk::Button::from_icon_name("window-close-symbolic");
        close.add_css_class("tab-close");
        close.set_tooltip_text(Some("Close terminal"));
        close.update_property(&[gtk::accessible::Property::Label("Close terminal")]);
        tab.append(&close);
        let session = Rc::new(Self {
            root,
            tab,
            close,
            retry,
            terminal,
            id: host.id.clone(),
            title,
            address,
            state,
            active: Cell::new(false),
            closed: Cell::new(false),
            pid: Cell::new(None),
            spawning: RefCell::new(None),
        });
        let weak = Rc::downgrade(&session);
        session.terminal.connect_child_exited(move |_, status| {
            if let Some(s) = weak.upgrade() {
                let exit = ExitStatus::from_raw(status);
                s.ended(&if exit.success() {
                    "Disconnected".into()
                } else {
                    format!("SSH exited · {exit}")
                });
            }
        });
        session.clipboard_controls();
        session
    }

    pub fn start(self: &Rc<Self>, host: &Host) {
        if self.active.get() || self.closed.get() {
            return;
        }
        let Ok(executable) = std::env::current_exe() else {
            self.ended("Could not locate SSH Manager.");
            return;
        };
        let Some(program) = executable.to_str() else {
            self.ended("The SSH Manager path must be UTF-8.");
            return;
        };
        self.active.set(true);
        self.title.set_text(host.title());
        self.address.set_text(&host.destination());
        self.state.set_text("Starting SSH…");
        self.retry.set_visible(false);
        self.terminal.reset(true, true);
        self.terminal.set_input_enabled(true);
        let cancel = gio::Cancellable::new();
        self.spawning.replace(Some(cancel.clone()));
        let weak = Rc::downgrade(self);
        self.terminal.spawn_async(
            vte::PtyFlags::DEFAULT,
            None,
            &[program, "--session", &host.id],
            &[],
            glib::SpawnFlags::DEFAULT,
            || {},
            -1,
            Some(&cancel),
            move |result| {
                if let Some(s) = weak.upgrade() {
                    s.spawning.take();
                    if s.closed.get() {
                        return;
                    }
                    match result {
                        Ok(pid) => {
                            s.pid.set(Some(pid.0));
                            s.state.set_text("SSH running");
                        }
                        Err(e) => s.ended(&format!("Could not start SSH · {e}")),
                    }
                }
            },
        );
        self.terminal.grab_focus();
    }

    fn ended(&self, message: &str) {
        self.pid.set(None);
        self.active.set(false);
        if !self.closed.get() {
            self.state.set_text(message);
            self.state.set_tooltip_text(Some(message));
            self.terminal.set_input_enabled(false);
            self.retry.set_visible(true);
        }
    }

    pub fn shutdown(&self) {
        self.closed.set(true);
        if let Some(cancel) = self.spawning.take() {
            cancel.cancel();
        }
        if let Some(pid) = self.pid.take() {
            // VTE owns this child until child-exited runs on this same main thread.
            unsafe {
                libc::kill(pid, libc::SIGHUP);
            }
        }
        self.active.set(false);
    }

    fn clipboard_controls(self: &Rc<Self>) {
        let keyboard = gtk::EventControllerKey::new();
        keyboard.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = self.terminal.downgrade();
        keyboard.connect_key_pressed(move |_, key, _, mods| {
            let mask = gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK;
            if mods & gtk::accelerator_get_default_mod_mask() == mask
                && let Some(terminal) = weak.upgrade()
            {
                match key.to_lower() {
                    gdk::Key::c => {
                        terminal.copy_clipboard_format(vte::Format::Text);
                        return glib::Propagation::Stop;
                    }
                    gdk::Key::v => {
                        terminal.paste_clipboard();
                        return glib::Propagation::Stop;
                    }
                    _ => {}
                }
            }
            glib::Propagation::Proceed
        });
        self.terminal.add_controller(keyboard);
        let click = gtk::GestureClick::new();
        click.set_button(3);
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        click.connect_pressed(move |gesture, _, x, y| {
            let Some(s) = weak.upgrade() else { return };
            gesture.set_state(gtk::EventSequenceState::Claimed);
            let popup = gtk::Popover::new();
            let menu = column(2);
            for (title, enabled) in [
                ("Copy", s.terminal.has_selection()),
                ("Paste", s.active.get()),
                ("Select all", true),
            ] {
                let term = s.terminal.downgrade();
                let pop = popup.downgrade();
                let b = button(title, None, "quiet", move || {
                    if let Some(t) = term.upgrade() {
                        match title {
                            "Copy" => t.copy_clipboard_format(vte::Format::Text),
                            "Paste" => t.paste_clipboard(),
                            _ => t.select_all(),
                        }
                    }
                    if let Some(p) = pop.upgrade() {
                        p.popdown();
                    }
                });
                b.set_sensitive(enabled);
                menu.append(&b);
            }
            popup.set_child(Some(&menu));
            popup.set_parent(&s.terminal);
            popup.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popup.connect_closed(|p| p.unparent());
            popup.popup();
        });
        self.terminal.add_controller(click);
    }
}
