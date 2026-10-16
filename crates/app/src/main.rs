mod app;
mod config;
mod connection_dialog;
mod i18n;
mod query_tab;
mod results;
mod sidebar;

use gtk::{gio, glib};
use quarry_engine::EngineHandle;
use relm4::RelmApp;
use tracing_subscriber::EnvFilter;

use crate::app::App;
use crate::config::APP_ID;

fn main() -> glib::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("QUARRY_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    i18n::init();

    if let Err(err) = gio::resources_register_include!("quarry.gresource") {
        tracing::error!(%err, "failed to register resources");
        return glib::ExitCode::FAILURE;
    }

    let (engine, events) = match EngineHandle::spawn() {
        Ok(pair) => pair,
        Err(err) => {
            tracing::error!(%err, "failed to start engine");
            return glib::ExitCode::FAILURE;
        }
    };

    let application = adw::Application::builder().application_id(APP_ID).build();
    RelmApp::from_app(application).run::<App>((engine, events));
    glib::ExitCode::SUCCESS
}
