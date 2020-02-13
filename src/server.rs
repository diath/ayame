use crate::client::Client;

use std::sync::Arc;

use std::collections::HashMap;
use std::vec::Vec;

use tokio::net::TcpListener;
use tokio::sync::Mutex;

pub struct Server {
    clients: Mutex<HashMap<String, Arc<Client>>>,
    clients_pending: Mutex<Vec<Arc<Client>>>,
    operators: Mutex<HashMap<String, String>>,
}

impl Server {
    pub fn new() -> Server {
        Server {
            clients: Mutex::new(HashMap::new()),
            clients_pending: Mutex::new(vec![]),
            operators: Mutex::new(HashMap::new()),
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

            server.clients_pending.lock().await.push(client.clone());
        }
    }

    pub async fn is_nick_mapped(&self, name: &str) -> bool {
        self.clients.lock().await.contains_key(name)
    }

    pub async fn map_nick(&self, name: String, client: &Client) {
        let index = self
            .clients_pending
            .lock()
            .await
            .iter()
            .position(|c| Arc::into_raw(c.clone()) == &*client);
        if index.is_none() {
            panic!("map nick");
        }
        let c = self.clients_pending.lock().await.remove(index.unwrap());
        self.clients.lock().await.insert(name, c);
    }

    pub async fn remap_nick(&self, old_name: String, name: String) {
        if !self.clients.lock().await.contains_key(&old_name) {
            panic!("remap_nick()");
        }

        if let Some(client) = self.clients.lock().await.remove(&old_name) {
            self.clients.lock().await.insert(name, client);
        }
    }

    pub async fn is_operator(&self, name: String, password: String) -> bool {
        if let Some(entry) = self.operators.lock().await.get(name.as_str()) {
            entry == &password
        } else {
            false
        }
    }
}
