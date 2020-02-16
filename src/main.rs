mod channel;
mod client;
mod replies;
mod server;
mod version;

use server::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    return Server::new("ayame".to_string()).accept().await;
}
