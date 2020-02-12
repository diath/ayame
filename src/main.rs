mod client;
mod server;

use server::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    return Server::new().accept().await;
}
