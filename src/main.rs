mod ayame;
mod channel;
mod client;
mod config;
mod replies;
mod server;

use ayame::*;
use server::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{} {} ({})", IRCD_NAME, IRCD_VERSION, IRCD_REPOSITORY);
    return Server::new().accept().await;
}
