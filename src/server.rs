use crate::client::Client;

use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::Mutex;

pub struct Server {
    pub name: String,
    clients: Mutex<Vec<Arc<Client>>>,
}

impl Server {
    pub fn new() -> Server {
        Server {
            name: "Test".to_string(),
            clients: Mutex::new(vec![]),
        }
    }

    pub async fn accept(self) -> Result<(), Box<dyn std::error::Error>> {
        let server = Arc::new(self);
        let mut acceptor = TcpListener::bind("127.0.0.1:6667").await?;
        loop {
            let (stream, addr) = acceptor.accept().await?;
            let client = Arc::new(Client::new(server.clone(), addr));

            println!("Client connected ({}).", addr);
            let c = Mutex::new(client.clone());
            tokio::spawn(async move {
                c.lock().await.task(stream).await;
            });

            server.clients.lock().await.push(client.clone());
        }
    }
}
