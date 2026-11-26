use crate::{
    APP_ID,
    editor::Editor,
    session::Session,
    store::{Host, Store},
    widgets::*,
};
use anyhow::{Result, anyhow};
use gtk::{gdk, gio, glib, prelude::*};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

pub struct Window {
    pub window: gtk::ApplicationWindow,
    store: Store,
    hosts: RefCell<Vec<Host>>,
    selected: RefCell<Option<String>>,
    editor: RefCell<Option<Editor>>,
    sessions: RefCell<Vec<Rc<Session>>>,
    busy: Cell<bool>,
    selecting: Cell<bool>,
    closing: Cell<bool>,
    allow_close: Cell<bool>,
    sidebar: gtk::Box,
    search: gtk::SearchEntry,
    list: gtk::ListBox,
    placeholder: gtk::Label,
    host_count: gtk::Label,
    tab_count: gtk::Label,
    tabs: gtk::Notebook,
    library: gtk::ScrolledWindow,
    content: gtk::Box,
    notice: gtk::Box,
    notice_text: gtk::Label,
}

impl Window {
    pub fn new(app: &gtk::Application) -> Result<Rc<Self>> {
        let store = Store::desktop()?;
        let hosts = store.hosts()?;
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("SSH Manager")
            .default_width(1040)
            .default_height(736)
            .icon_name(APP_ID)
            .build();
        window.set_size_request(704, 470);
        let header = gtk::HeaderBar::new();
        header.add_css_class("app-header");
        header.set_show_title_buttons(true);
        header.set_decoration_layout(Some(":minimize,maximize,close"));
        header.set_title_widget(Some(&row(0)));
        let brand = row(10);
        brand.append(&icon(APP_ID, 32));
        brand.append(&label("SSH Manager", "brand-title"));
        header.pack_start(&brand);
        window.set_titlebar(Some(&header));

        let sidebar = column(0);
        sidebar.add_css_class("sidebar");
        sidebar.set_width_request(184);
        let controls = column(10);
        controls.add_css_class("side-controls");
        let search = gtk::SearchEntry::new();
        search.add_css_class("host-search");
        search.set_placeholder_text(Some("Search hosts"));
        search.set_tooltip_text(Some("Find a host or folder (Ctrl+F)"));
        controls.append(&search);
        let add = gtk::Button::with_label("＋  Add host");
        add.add_css_class("side-add");
        add.update_property(&[gtk::accessible::Property::Label("Add host")]);
        add.set_tooltip_text(Some("Add a host (Ctrl+N)"));
        controls.append(&add);
        sidebar.append(&controls);
        let summary = row(6);
        summary.add_css_class("side-heading");
        let caption = label("Saved hosts", "side-caption");
        caption.set_hexpand(true);
        summary.append(&caption);
        let host_count = label("", "side-count");
        summary.append(&host_count);
        sidebar.append(&summary);
        let list = gtk::ListBox::new();
        list.add_css_class("host-list");
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.set_activate_on_single_click(false);
        let placeholder = label("", "side-empty");
        placeholder.set_valign(gtk::Align::Start);
        list.set_placeholder(Some(&placeholder));
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&list)
            .build();
        sidebar.append(&scroller);
        let footer = row(8);
        footer.add_css_class("side-footer");
        footer.append(&icon("utilities-terminal-symbolic", 15));
        let tab_count = label("No terminal tabs", "");
        footer.append(&tab_count);
        sidebar.append(&footer);

        let workspace = column(0);
        workspace.add_css_class("workspace");
        let notice = row(8);
        notice.add_css_class("notice");
        let notice_text = label("", "");
        notice_text.set_wrap(true);
        notice_text.set_hexpand(true);
        notice.append(&notice_text);
        let banner = notice.downgrade();
        notice.append(&icon_button(
            "window-close-symbolic",
            "Dismiss message",
            "quiet",
            move || {
                if let Some(b) = banner.upgrade() {
                    b.set_visible(false)
                }
            },
        ));
        notice.set_visible(false);
        workspace.append(&notice);
        let tabs = gtk::Notebook::new();
        tabs.add_css_class("tabs");
        tabs.set_scrollable(true);
        tabs.set_show_border(false);
        tabs.set_vexpand(true);
        workspace.append(&tabs);
        let content = column(0);
        content.add_css_class("page-content");
        content.set_valign(gtk::Align::Start);
        let library = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&content)
            .build();
        let library_tab = row(7);
        library_tab.append(&icon("view-list-symbolic", 15));
        library_tab.append(&label("Host library", ""));
        tabs.append_page(&library, Some(&library_tab));
        let split = gtk::Paned::new(gtk::Orientation::Horizontal);
        split.set_start_child(Some(&sidebar));
        split.set_end_child(Some(&workspace));
        split.set_resize_start_child(false);
        split.set_shrink_start_child(false);
        split.set_shrink_end_child(false);
        split.set_position(232);
        window.set_child(Some(&split));
        let ui = Rc::new(Self {
            window,
            store,
            hosts: RefCell::new(hosts),
            selected: RefCell::default(),
            editor: RefCell::default(),
            sessions: RefCell::default(),
            busy: Cell::new(false),
            selecting: Cell::new(false),
            closing: Cell::new(false),
            allow_close: Cell::new(false),
            sidebar,
            search,
            list,
            placeholder,
            host_count,
            tab_count,
            tabs,
            library,
            content,
            notice,
            notice_text,
        });
        let action = ui.action(|ui| ui.navigate(|ui| ui.edit(None)));
        add.connect_clicked(move |_| action());
        ui.bind();
        ui.refresh_sidebar();
        let first = ui.hosts.borrow().first().cloned();
        match first {
            Some(h) => ui.details(h),
            None => ui.edit(None),
        }
        let store = ui.store.clone();
        let weak = Rc::downgrade(&ui);
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || store.retry_cleanup()).await;
            if let Some(ui) = weak.upgrade() {
                match result {
                    Ok(Ok(done)) => ui.cleanup_result(done),
                    Ok(Err(e)) => ui.message(&format!("{e:#}")),
                    _ => {}
                }
            }
        });
        Ok(ui)
    }

    fn action(self: &Rc<Self>, f: impl Fn(&Rc<Self>) + 'static) -> impl Fn() + 'static {
        let weak = Rc::downgrade(self);
        move || {
            if let Some(ui) = weak.upgrade() {
                f(&ui)
            }
        }
    }

    fn bind(self: &Rc<Self>) {
        let refresh = self.action(|ui| ui.refresh_sidebar());
        self.search.connect_search_changed(move |_| refresh());
        let weak = Rc::downgrade(self);
        self.list.connect_row_selected(move |_, r| {
            let (Some(ui), Some(r)) = (weak.upgrade(), r) else {
                return;
            };
            if !ui.selecting.get()
                && let Some(host) = ui.host(&r.widget_name())
            {
                ui.navigate(move |ui| ui.details(host));
            }
        });
        let weak = Rc::downgrade(self);
        self.list.connect_row_activated(move |_, r| {
            if let Some(ui) = weak.upgrade()
                && let Some(host) = ui.host(&r.widget_name())
            {
                ui.navigate(move |ui| {
                    ui.details(host.clone());
                    ui.connect_host(host)
                });
            }
        });
        let weak = Rc::downgrade(self);
        self.tabs.connect_switch_page(move |_, page, _| {
            if let Some(ui) = weak.upgrade() {
                let sessions = ui.sessions.borrow();
                if let Some(s) = sessions
                    .iter()
                    .find(|s| s.root.upcast_ref::<gtk::Widget>() == page)
                {
                    s.terminal.grab_focus();
                }
            }
        });
        let weak = Rc::downgrade(self);
        self.window.connect_close_request(move |_| {
            weak.upgrade()
                .map_or(glib::Propagation::Proceed, |ui| ui.close_request())
        });
        let weak = Rc::downgrade(self);
        let keyboard = gtk::EventControllerKey::new();
        keyboard.set_propagation_phase(gtk::PropagationPhase::Capture);
        keyboard.connect_key_pressed(move |_, k, _, mods| {
            let Some(ui) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let mods = mods & gtk::accelerator_get_default_mod_mask();
            let library = ui.tabs.current_page() == Some(0);
            if library && mods == gdk::ModifierType::CONTROL_MASK && !ui.busy.get() {
                match k.to_lower() {
                    gdk::Key::n => ui.navigate(|ui| ui.edit(None)),
                    gdk::Key::f => {
                        ui.search.grab_focus();
                    }
                    gdk::Key::s if ui.editor.borrow().is_some() => ui.save(false),
                    _ => return glib::Propagation::Proceed,
                }
            } else if !library
                && k.to_lower() == gdk::Key::w
                && mods == (gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK)
            {
                let page = ui.tabs.nth_page(ui.tabs.current_page());
                let target = ui
                    .sessions
                    .borrow()
                    .iter()
                    .find(|s| Some(s.root.upcast_ref()) == page.as_ref())
                    .cloned();
                if let Some(s) = target {
                    ui.close_tab(s)
                }
            } else {
                return glib::Propagation::Proceed;
            }
            glib::Propagation::Stop
        });
        self.window.add_controller(keyboard);
    }

    fn host(&self, id: &str) -> Option<Host> {
        self.hosts.borrow().iter().find(|h| h.id == id).cloned()
    }
    fn message(&self, text: &str) {
        self.notice_text.set_text(text);
        self.notice.set_visible(true);
    }
    fn cleanup_result(&self, clean: bool) {
        if !clean {
            self.message("Keyring cleanup is pending. Unlock your desktop keyring. SSH Manager will retry after the next change or when reopened.")
        }
    }
    fn refresh_sidebar(&self) {
        self.selecting.set(true);
        while let Some(r) = self.list.row_at_index(0) {
            self.list.remove(&r)
        }
        let query = glib::casefold(self.search.text().trim());
        let hosts = self.hosts.borrow();
        self.host_count.set_text(&hosts.len().to_string());
        self.placeholder.set_text(if query.is_empty() {
            "No saved hosts yet"
        } else {
            "No matching hosts"
        });
        let grouped = hosts.iter().any(|h| !h.group.is_empty());
        let groups: std::collections::HashMap<_, _> = hosts
            .iter()
            .map(|h| (h.id.clone(), h.group.clone()))
            .collect();
        self.list.set_header_func(move |item, before| {
            let group = groups.get(item.widget_name().as_str());
            let previous = before.and_then(|r| groups.get(r.widget_name().as_str()));
            if !grouped || group == previous {
                item.set_header(gtk::Widget::NONE);
                return;
            }
            let head = row(6);
            head.add_css_class("folder-heading");
            head.append(&icon("folder-symbolic", 13));
            head.append(&label(
                group
                    .filter(|s| !s.is_empty())
                    .map_or("Ungrouped", String::as_str),
                "",
            ));
            item.set_header(Some(&head));
        });
        for host in hosts.iter() {
            let destination = host.destination();
            if !glib::casefold(format!("{} {destination} {}", host.title(), host.group))
                .contains(query.as_str())
            {
                continue;
            }
            let item = gtk::ListBoxRow::new();
            item.set_widget_name(&host.id);
            let line = row(9);
            line.add_css_class("host-row");
            line.append(&icon("network-server-symbolic", 16));
            let texts = column(2);
            for (value, class) in [(host.title(), "host-name"), (&destination, "host-address")] {
                let l = label(value, class);
                l.set_ellipsize(gtk::pango::EllipsizeMode::End);
                texts.append(&l);
            }
            texts.set_hexpand(true);
            line.append(&texts);
            item.set_child(Some(&line));
            item.set_tooltip_text(Some(&format!("{destination}\nDouble-click to connect")));
            self.list.append(&item);
        }
        drop(hosts);
        self.sync_selection();
    }
    fn sync_selection(&self) {
        self.selecting.set(true);
        let id = self.selected.borrow();
        let mut i = 0;
        self.list.unselect_all();
        while let Some(r) = self.list.row_at_index(i) {
            if id.as_deref() == Some(r.widget_name().as_str()) {
                self.list.select_row(Some(&r));
                break;
            }
            i += 1;
        }
        self.selecting.set(false);
    }
    fn show_page(&self, page: &gtk::Box) {
        while let Some(child) = self.content.first_child() {
            self.content.remove(&child)
        }
        self.content.append(page);
        self.tabs.set_current_page(Some(0));
        self.library.vadjustment().set_value(0.0);
        self.sync_selection();
    }
    fn navigate(self: &Rc<Self>, action: impl FnOnce(&Rc<Self>) + 'static) {
        if self.busy.get() {
            return;
        }
        if !self.editor.borrow().as_ref().is_some_and(Editor::dirty) {
            action(self);
            return;
        }
        self.sync_selection();
        let ui = self.clone();
        glib::spawn_future_local(async move {
            if confirm(
                &ui.window,
                "Discard these changes?",
                "Your edits to this host have not been saved.",
                "Discard changes",
            )
            .await
            {
                action(&ui)
            }
        });
    }
    fn edit(self: &Rc<Self>, host: Option<Host>) {
        let back = host
            .clone()
            .or_else(|| self.hosts.borrow().first().cloned());
        let cancel = back.map(|h| {
            Box::new(self.action(move |ui| {
                let h = h.clone();
                ui.navigate(move |ui| ui.details(h));
            })) as Box<dyn Fn()>
        });
        self.selected.replace(host.as_ref().map(|h| h.id.clone()));
        let weak = Rc::downgrade(self);
        let editor = Editor::new(
            &self.window,
            host,
            move |connect| {
                if let Some(ui) = weak.upgrade() {
                    ui.save(connect)
                }
            },
            cancel,
        );
        self.editor.replace(Some(editor.clone()));
        self.show_page(&editor.root);
        editor.focus();
    }

    fn details(self: &Rc<Self>, host: Host) {
        self.editor.take();
        self.selected.replace(Some(host.id.clone()));
        let page = column(18);
        page.append(&heading(host.title(), &host.destination()));
        let actions = row(9);
        let target = host.clone();
        actions.append(&button(
            "Connect",
            Some("utilities-terminal-symbolic"),
            "primary",
            self.action(move |ui| ui.connect_host(target.clone())),
        ));
        let target = host.clone();
        actions.append(&button(
            "Edit host",
            Some("document-edit-symbolic"),
            "secondary",
            self.action(move |ui| ui.edit(Some(target.clone()))),
        ));
        page.append(&actions);
        let details = card("Connection", "network-wired-symbolic");
        let value_line = |title: &str, value: &str| {
            let line = row(14);
            let caption = label(title, "muted");
            caption.set_width_chars(12);
            line.append(&caption);
            let text = label(value, "");
            text.set_selectable(true);
            text.set_wrap(true);
            line.append(&text);
            line
        };
        details.append(&value_line("Hostname", &host.hostname));
        details.append(&value_line(
            "Port",
            &host
                .port
                .map(|p| p.to_string())
                .unwrap_or_else(|| "SSH default (usually 22)".into()),
        ));
        details.append(&value_line(
            "Username",
            if host.username.is_empty() {
                "SSH default"
            } else {
                &host.username
            },
        ));
        details.append(&value_line(
            "Folder",
            if host.group.is_empty() {
                "Ungrouped"
            } else {
                &host.group
            },
        ));
        page.append(&details);
        let auth = card("Authentication", "channel-secure-symbolic");
        auth.append(&value_line(
            "Method",
            if host.secret.is_some() {
                "Saved password · desktop keyring"
            } else {
                "SSH key or prompt"
            },
        ));
        if !host.key.is_empty() {
            auth.append(&value_line("Private key", &host.key))
        }
        page.append(&auth);
        let remove = button(
            "Remove host",
            Some("user-trash-symbolic"),
            "remove",
            self.action(move |ui| ui.remove(host.clone())),
        );
        remove.set_halign(gtk::Align::Start);
        page.append(&remove);
        self.show_page(&page);
    }

    fn change<T: Send + 'static>(
        self: &Rc<Self>,
        message: &str,
        job: impl FnOnce(Store) -> Result<T> + Send + 'static,
        done: impl FnOnce(&Rc<Self>, T) + 'static,
    ) {
        if self.busy.replace(true) {
            return;
        }
        self.sidebar.set_sensitive(false);
        self.library.set_sensitive(false);
        self.message(message);
        let ui = self.clone();
        let store = self.store.clone();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || job(store))
                .await
                .unwrap_or_else(|_| Err(anyhow!("The keyring operation was interrupted.")));
            ui.busy.set(false);
            ui.sidebar.set_sensitive(true);
            ui.library.set_sensitive(true);
            ui.notice.set_visible(false);
            match result {
                Ok(result) => match ui.store.hosts() {
                    Ok(hosts) => {
                        ui.hosts.replace(hosts);
                        ui.refresh_sidebar();
                        done(&ui, result);
                    }
                    Err(err) => ui.message(&format!("Could not reload the host library. {err:#}")),
                },
                Err(err) => {
                    if let Some(editor) = ui.editor.borrow().as_ref() {
                        editor.show_error(&err)
                    } else {
                        ui.message(&format!("{err:#}"))
                    }
                }
            }
        });
    }
    fn save(self: &Rc<Self>, connect: bool) {
        if self.busy.get() {
            return;
        }
        let Some(editor) = self.editor.borrow().clone() else {
            return;
        };
        let host = match editor.read() {
            Ok(h) => h,
            Err(e) => {
                editor.show_error(&e);
                return;
            }
        };
        let (original, vault, password) = (
            editor.original.clone(),
            editor.use_vault(),
            editor.password(),
        );
        self.change(
            "Saving host…",
            move |s| s.save(host, original.as_ref(), vault, &password),
            move |ui, (host, clean)| {
                editor.clear_password();
                ui.search.set_text("");
                ui.details(host.clone());
                ui.cleanup_result(clean);
                if connect {
                    ui.connect_host(host)
                }
            },
        );
    }
    fn remove(self: &Rc<Self>, host: Host) {
        let ui = self.clone();
        glib::spawn_future_local(async move {
            let text = if host.secret.is_some() {
                "This removes the host and its saved password from the desktop keyring."
            } else {
                "This removes the connection from your host library."
            };
            if confirm(
                &ui.window,
                &format!("Remove {}?", host.title()),
                text,
                "Remove host",
            )
            .await
            {
                ui.change(
                    "Removing host…",
                    move |s| s.remove(&host),
                    |ui, clean| {
                        let first = ui.hosts.borrow().first().cloned();
                        match first {
                            Some(h) => ui.details(h),
                            None => ui.edit(None),
                        }
                        ui.cleanup_result(clean);
                    },
                );
            }
        });
    }
    fn connect_host(self: &Rc<Self>, host: Host) {
        let s = Session::new(&host);
        let weak = Rc::downgrade(&s);
        let close = self.action(move |ui| {
            if let Some(s) = weak.upgrade() {
                ui.close_tab(s)
            }
        });
        s.close.connect_clicked(move |_| close());
        let weak = Rc::downgrade(&s);
        let retry = self.action(move |ui| {
            if let Some(s) = weak.upgrade() {
                match ui.store.get(&s.id) {
                    Ok(host) => s.start(&host),
                    Err(e) => ui.message(&e.to_string()),
                }
            }
        });
        s.retry.connect_clicked(move |_| retry());
        self.sessions.borrow_mut().push(s.clone());
        let i = self.tabs.append_page(&s.root, Some(&s.tab));
        self.tabs.set_current_page(Some(i));
        self.update_count();
        s.start(&host);
    }
    fn close_tab(self: &Rc<Self>, s: Rc<Session>) {
        let ui = self.clone();
        glib::spawn_future_local(async move {
            if s.active.get()
                && !confirm(
                    &ui.window,
                    "Close this terminal?",
                    &format!("The SSH session for {} will end.", s.title.text()),
                    "Close terminal",
                )
                .await
            {
                return;
            }
            s.shutdown();
            let index = ui.tabs.page_num(&s.root);
            if let Some(i) = index {
                ui.tabs.remove_page(Some(i))
            }
            ui.sessions
                .borrow_mut()
                .retain(|item| !Rc::ptr_eq(item, &s));
            ui.update_count();
        });
    }
    fn update_count(&self) {
        let n = self.sessions.borrow().len();
        self.tab_count.set_text(&match n {
            0 => "No terminal tabs".into(),
            1 => "1 terminal tab".into(),
            _ => format!("{n} terminal tabs"),
        });
    }
    fn close_request(self: &Rc<Self>) -> glib::Propagation {
        if self.busy.get() {
            self.message("Finishing your host changes…");
            return glib::Propagation::Stop;
        }
        let dirty = self.editor.borrow().as_ref().is_some_and(Editor::dirty);
        let active = self.sessions.borrow().iter().any(|s| s.active.get());
        if self.allow_close.get() || (!dirty && !active) {
            for s in self.sessions.borrow().iter() {
                s.shutdown()
            }
            return glib::Propagation::Proceed;
        }
        if !self.closing.replace(true) {
            let ui = self.clone();
            glib::spawn_future_local(async move {
                let description = match (dirty, active) {
                    (true, true) => {
                        "Your host edits will be discarded and open SSH sessions will disconnect."
                    }
                    (true, false) => "Your edits to this host have not been saved.",
                    _ => "Closing the window will disconnect the open SSH sessions.",
                };
                let accepted = confirm(
                    &ui.window,
                    "Close SSH Manager?",
                    description,
                    "Close window",
                )
                .await;
                ui.closing.set(false);
                if accepted {
                    ui.allow_close.set(true);
                    ui.window.close()
                }
            });
        }
        glib::Propagation::Stop
    }
}
