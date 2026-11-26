use gtk::{gdk, prelude::*};

pub fn column(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, spacing)
}
pub fn row(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Horizontal, spacing)
}
pub fn label(text: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    if !class.is_empty() {
        label.add_css_class(class);
    }
    label
}
pub fn icon(name: &str, size: i32) -> gtk::Image {
    let image = gtk::Image::from_icon_name(name);
    image.set_pixel_size(size);
    image
}
pub fn button(
    text: &str,
    symbol: Option<&str>,
    class: &str,
    action: impl Fn() + 'static,
) -> gtk::Button {
    let b = gtk::Button::new();
    let content = row(7);
    content.set_halign(gtk::Align::Center);
    if let Some(name) = symbol {
        content.append(&icon(name, 16));
    }
    if !text.is_empty() {
        content.append(&label(text, ""));
        b.update_property(&[gtk::accessible::Property::Label(text)]);
    }
    b.set_child(Some(&content));
    b.add_css_class(class);
    b.connect_clicked(move |_| action());
    b
}
pub fn icon_button(
    name: &str,
    title: &str,
    class: &str,
    action: impl Fn() + 'static,
) -> gtk::Button {
    let b = button("", Some(name), class, action);
    b.set_tooltip_text(Some(title));
    b.update_property(&[gtk::accessible::Property::Label(title)]);
    b
}
pub fn card(title: &str, symbol: &str) -> gtk::Box {
    let panel = column(14);
    panel.add_css_class("card");
    let line = row(8);
    let image = icon(symbol, 17);
    image.add_css_class("section-icon");
    line.append(&image);
    line.append(&label(title, "card-title"));
    panel.append(&line);
    panel
}
pub fn heading(title: &str, subtitle: &str) -> gtk::Box {
    let line = row(13);
    let image = icon("network-server-symbolic", 23);
    image.add_css_class("page-icon");
    image.set_valign(gtk::Align::Center);
    line.append(&image);
    let texts = column(3);
    let name = label(title, "page-title");
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    texts.append(&name);
    let sub = label(subtitle, "muted");
    sub.set_wrap(true);
    texts.append(&sub);
    line.append(&texts);
    line
}
pub fn field(title: &str, widget: &impl IsA<gtk::Widget>, entry: &gtk::Entry) -> gtk::Box {
    let area = column(6);
    let caption = label(title, "field-label");
    caption.set_mnemonic_widget(Some(entry));
    entry.update_property(&[gtk::accessible::Property::Label(title)]);
    area.append(&caption);
    area.append(widget);
    area
}
pub async fn confirm(
    window: &gtk::ApplicationWindow,
    title: &str,
    text: &str,
    accept: &str,
) -> bool {
    let dialog = gtk::MessageDialog::builder()
        .transient_for(window)
        .modal(true)
        .destroy_with_parent(true)
        .message_type(gtk::MessageType::Question)
        .text(title)
        .secondary_text(text)
        .build();
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    dialog.add_button(accept, gtk::ResponseType::Accept);
    dialog.set_default_response(gtk::ResponseType::Cancel);
    let response = dialog.run_future().await;
    dialog.close();
    response == gtk::ResponseType::Accept
}
pub fn load_style(display: &gdk::Display) {
    let css = gtk::CssProvider::new();
    css.load_from_data(concat!(
        include_str!("style.css"),
        "\n",
        include_str!("gtk4.css")
    ));
    gtk::style_context_add_provider_for_display(
        display,
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
