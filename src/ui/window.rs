use crate::ui::create_tab::CreateTab;
use crate::ui::verify_tab::VerifyTab;
use gtk4::prelude::*;
use libadwaita::prelude::*;
use libadwaita::{Application, ApplicationWindow, HeaderBar, ViewStack, ViewSwitcher};

pub fn build_window(app: &Application) {
    let css = gtk4::CssProvider::new();
    css.load_from_data(include_str!("../../resources/style.css"));
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("Could not connect to display"),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let window = ApplicationWindow::builder()
        .application(app)
        .title("HashMyFiles")
        .default_width(1100)
        .default_height(780)
        .build();

    let stack = ViewStack::new();
    stack.set_vexpand(true);

    let create_tab = CreateTab::new();
    let create_page = stack.add_titled(create_tab.widget(), Some("create"), "Create Database");
    create_page.set_icon_name(Some("document-new-symbolic"));

    let verify_tab = VerifyTab::new();
    let verify_page = stack.add_titled(verify_tab.widget(), Some("verify"), "Verify Database");
    verify_page.set_icon_name(Some("emblem-default-symbolic"));

    // Tab switcher in header
    let switcher = ViewSwitcher::new();
    switcher.set_stack(Some(&stack));
    switcher.set_policy(libadwaita::ViewSwitcherPolicy::Wide);

    // Light/Dark toggle button
    let theme_btn = gtk4::Button::with_label("☀ Light");
    theme_btn.add_css_class("retro-btn");
    theme_btn.set_tooltip_text(Some("Toggle light/dark mode"));

    let header = HeaderBar::new();
    header.set_title_widget(Some(&switcher));
    header.pack_end(&theme_btn);

    // Wire theme toggle
    {
        let window_weak = window.downgrade();
        let light_mode = std::rc::Rc::new(std::cell::Cell::new(false));
        theme_btn.connect_clicked(move |btn| {
            let window = match window_weak.upgrade() { Some(w) => w, None => return };
            light_mode.set(!light_mode.get());
            if light_mode.get() {
                window.add_css_class("light-mode");
                btn.set_label("🌙 Dark");
            } else {
                window.remove_css_class("light-mode");
                btn.set_label("☀ Light");
            }
        });
    }

    let main_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    main_box.append(&header);
    main_box.append(&stack);

    window.set_content(Some(&main_box));
    window.present();
}
