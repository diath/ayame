mod client;
mod server;

use crate::server::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    return Server::new().run().await;
}
