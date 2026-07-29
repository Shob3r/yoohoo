#[cfg(not(target_env = "msvc"))]
use tikv_jemallocator::Jemalloc;

use gtk::{Application, ApplicationWindow, glib};
use gtk::{Button, prelude::*};
use std::env::set_var;

use crate::core::fs::{BaseDirectory, full_path};

pub mod core;
pub mod util;

const APP_ID: &str = "app.elysiae.Elysiae";

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

fn main() -> glib::ExitCode {
    // Set environment variables required to get proton working
    unsafe {
        let compat_path = full_path(None, Some(BaseDirectory::Compat)).unwrap(); // If this fails then the app shouldn't be running in the first place

        set_var("STEAM_COMPAT_DATA_PATH", compat_path);
        set_var("STEAM_COMPAT_CLIENT_INSTALL_PATH", "");
    }

    // Create a new application
    let app = Application::builder().application_id(APP_ID).build();

    // Connect to "activate" signal of `app`
    app.connect_activate(build_ui);

    // Run the application
    app.run()
}

fn build_ui(app: &Application) {
    // Create a button with label and margins
    let button = Button::builder()
        .label("Press me!")
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();

    // Connect to "clicked" signal of `button`
    button.connect_clicked(|button| {
        // Set the label to "Hello World!" after the button has been clicked on
        button.set_label("Hello World!");
    });

    // Create a window
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Elysiae")
        .child(&button)
        .width_request(1200)
        .height_request(700)
        .resizable(false)
        .build();

    // Present window
    window.present();
}
