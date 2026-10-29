//! The wire between the engine and a driver that runs in its own process.
//!
//! A driver binary answers two commands: `hello` prints a [`Hello`] frame (the drivers it serves,
//! their capabilities and dialects) and exits; `serve <id>` serves one session of driver `<id>`
//! over stdin and stdout until the host sends `Close`. The engine runs a binary per session
//! through [`ProcessDriver`].

mod client;
mod rows;
mod server;
mod wire;

use std::process::ExitCode;
use std::sync::Arc;

use quarry_driver_api::Driver;
use tracing_subscriber::EnvFilter;

pub use client::{ProcessDriver, hello};
pub use server::serve;
pub use wire::{DriverInfo, FrameError, Hello, PROTOCOL};

/// The whole `main` of a driver binary serving `drivers`. Logs go to stderr (`QUARRY_LOG`).
pub fn driver_main(drivers: Vec<Arc<dyn Driver>>) -> ExitCode {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_env("QUARRY_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            tracing::error!(%err, "no runtime");
            return ExitCode::FAILURE;
        }
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["hello"] => runtime.block_on(print_hello(&drivers)),
        ["serve", id] => {
            let Some(driver) = drivers.iter().find(|d| d.id().0 == *id).cloned() else {
                tracing::error!(id, "this binary does not serve that driver");
                return ExitCode::from(2);
            };
            runtime.block_on(serve(driver, tokio::io::stdin(), tokio::io::stdout()))
        }
        _ => {
            tracing::error!("usage: hello | serve <driver id>");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!(%err, "driver stopped");
            ExitCode::FAILURE
        }
    }
}

async fn print_hello(drivers: &[Arc<dyn Driver>]) -> Result<(), FrameError> {
    let hello = Hello {
        protocol: PROTOCOL,
        version: env!("CARGO_PKG_VERSION").to_owned(),
        drivers: drivers
            .iter()
            .map(|d| DriverInfo {
                id: d.id(),
                capabilities: d.capabilities(),
                dialect: d.dialect().spec().clone(),
            })
            .collect(),
    };
    wire::write_frame(&mut tokio::io::stdout(), &hello).await
}

#[cfg(test)]
mod tests;
