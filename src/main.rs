#[cfg(not(target_env = "msvc"))]
use tikv_jemallocator::Jemalloc;

use gtk::{glib};
use gtk::{prelude::*};
use std::env::set_var;
use gtk::gio;

use crate::core::fs::{BaseDirectory, full_path};

mod core;
mod util;
mod window;
mod app;
mod config;

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
    let res = gio::Resource::load(config::resources_file()).expect("Could not load gresource file");
    gio::resources_register(&res);

    let app = app::build_app();

    app.run()
}
