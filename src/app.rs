use gtk::{
    CssProvider,
    gio::prelude::ApplicationExt,
    prelude::{GtkApplicationExt, GtkWindowExt},
    style_context_add_provider_for_display,
};

use crate::config;

const OVERRIDE_EVERY_OTHER_THEME_THAT_COULD_BE_DEFINED_BY_A_USER_PRIORITY: u32 = u32::MAX;

pub fn build_app() -> gtk::Application {
    let app = gtk::Application::builder()
        .application_id(config::app_id())
        .build();

    app.connect_startup(|_| {
        load_css();
    });

    app.connect_activate(build_ui);
    app
}

fn load_css() {
    let provider = CssProvider::new();
    provider.load_from_resource("/app/elysiae/Elysiae/style.css");

    style_context_add_provider_for_display(
        &gtk::gdk::Display::default().expect("Could not connect to a display"),
        &provider,
        OVERRIDE_EVERY_OTHER_THEME_THAT_COULD_BE_DEFINED_BY_A_USER_PRIORITY,
    );
}

fn build_ui(app: &gtk::Application) {
    if let Some(window) = app.active_window() {
        window.present();
        return;
    }

    let window = crate::window::build_window(app);
    window.present();
}
