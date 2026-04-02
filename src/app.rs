use crate::ui::window::build_window;
use libadwaita::prelude::*;
use libadwaita::Application;

pub const APP_ID: &str = "com.hashmyfiles.app";

pub fn build_app() -> Application {
    let app = Application::builder()
        .application_id(APP_ID)
        .build();

    app.connect_activate(|app| {
        build_window(app);
    });

    app
}
