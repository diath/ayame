mod ayame;
mod channel;
mod client;
mod cloak;
mod config;
mod mask;
mod replies;
mod server;

use chrono;
use std::io::Write;

use env_logger;
use log;

use ayame::*;
use server::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let default_log_filter = if cfg!(debug_assertions) {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };

    env_logger::Builder::new()
        .format(|buffer, record| {
            writeln!(
                buffer,
                "[{}] [{}] {}",
                chrono::Local::now().format("%d %b %Y %H:%M:%S"),
                record.level(),
                record.args()
            )
        })
        .filter(None, default_log_filter)
        .init();

    log::info!("{} {} ({})", IRCD_NAME, IRCD_VERSION, IRCD_REPOSITORY);
    return Server::new().accept().await;
}
