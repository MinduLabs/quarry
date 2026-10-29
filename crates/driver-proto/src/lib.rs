//! The wire between the engine and a driver that runs in its own process.
//!
//! A driver binary answers two commands: `hello` prints a [`Hello`] frame (the drivers it serves,
//! their capabilities and dialects) and exits; `serve <id>` serves one session of driver `<id>`
//! over stdin and stdout until the host sends `Close`. The engine runs a binary per session
//! through [`ProcessDriver`].

mod client;
mod server;
mod wire;

use std::process::ExitCode;
use std::sync::Arc;

use quarry_driver_api::Driver;
use tokio::io::{AsyncRead, AsyncWrite, BufReader};
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
    // Two workers, so the writer sends one batch while the driver reads the next.
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
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
            runtime.block_on(async {
                let (input, output) = stdio();
                serve(driver, BufReader::new(input), output).await
            })
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
    wire::write_frame(&mut stdio().1, &hello).await
}

type Input = Box<dyn AsyncRead + Unpin + Send>;
type Output = Box<dyn AsyncWrite + Unpin + Send>;

/// Stdin and stdout as async pipes when they are pipes, as the engine starts a driver.
/// `tokio::io::stdin` / `stdout` hand every read and write to the blocking pool, which doubled
/// the time to a query's first rows.
#[cfg(unix)]
fn stdio() -> (Input, Output) {
    use std::os::fd::AsFd;

    use tokio::net::unix::pipe;

    let input: Input = match std::io::stdin()
        .as_fd()
        .try_clone_to_owned()
        .and_then(pipe::Receiver::from_owned_fd)
    {
        Ok(pipe) => Box::new(pipe),
        Err(_) => Box::new(tokio::io::stdin()),
    };
    let output: Output = match std::io::stdout()
        .as_fd()
        .try_clone_to_owned()
        .and_then(pipe::Sender::from_owned_fd)
    {
        Ok(pipe) => Box::new(pipe),
        Err(_) => Box::new(tokio::io::stdout()),
    };
    (input, output)
}

#[cfg(not(unix))]
fn stdio() -> (Input, Output) {
    (Box::new(tokio::io::stdin()), Box::new(tokio::io::stdout()))
}

#[cfg(test)]
mod tests;
