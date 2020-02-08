use tokio::net::TcpListener;

use crate::client::Client;

pub struct Server;

impl Server {
    pub fn new() -> Server {
        Server {}
    }

    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut listener = TcpListener::bind("127.0.0.1:6667").await?;
        loop {
            let (stream, address) = listener.accept().await?;
            println!("Client connected ({}).", address);
            let mut client = Client::new(address);
            tokio::spawn(async move {
                client.task(stream).await;
            });
        }
    }
}
