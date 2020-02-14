mod channel;
mod client;
mod replies;
mod server;

use server::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    return Server::new().accept().await;
}
