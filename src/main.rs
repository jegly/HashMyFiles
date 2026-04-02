mod app;
mod database;
mod hasher;
mod scanner;
mod ui;
mod utils;
mod verifier;

use app::build_app;
use libadwaita::prelude::*;

fn main() {
    let app = build_app();
    std::process::exit(app.run().into());
}
