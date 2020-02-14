use crate::channel::Channel;
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
    channels: Mutex<HashMap<String, Channel>>,
}

impl Server {
    pub fn new() -> Server {
        Server {
            clients: Mutex::new(HashMap::new()),
            clients_pending: Mutex::new(vec![]),
            operators: Mutex::new(HashMap::new()),
            channels: Mutex::new(HashMap::new()),
        }
    }

    pub async fn accept(self) -> Result<(), Box<dyn std::error::Error>> {
        let server = Arc::new(self);
        server
            .channels
            .lock()
            .await
            .insert("#lobby".to_string(), Channel::new("#lobby".to_string()));

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

    pub async fn is_operator(&self, name: &str, password: &str) -> bool {
        if let Some(entry) = self.operators.lock().await.get(name) {
            entry == password
        } else {
            false
        }
    }

    pub async fn forward_message(&self, sender: String, name: &str, message: String) {
        if !self.clients.lock().await.contains_key(name) {
            panic!("forward_message()");
        }

        if let Some(client) = self.clients.lock().await.get(name) {
            client
                .send_raw(format!(":{} PRIVMSG {} :{}\r\n", sender, name, message))
                .await;
        }
    }

    pub async fn is_channel_mapped(&self, name: &str) -> bool {
        self.channels.lock().await.contains_key(name)
    }

    pub async fn has_channel_participant(&self, name: &str, nick: &str) -> bool {
        if let Some(channel) = self.channels.lock().await.get(name) {
            return channel.has_participant(nick.to_string()).await;
        }

        false
    }

    pub async fn join_channel(&self, name: &str, nick: &str) {
        /* TODO(diath): This should broadcast user prefix and not nick. */
        let message = format!(":{} JOIN {}", nick, name);
        if let Some(channel) = self.channels.lock().await.get(name) {
            if channel.join(nick.to_string()).await {
                for target in &*channel.participants.lock().await {
                    if let Some(client) = self.clients.lock().await.get(target) {
                        client.send_raw(message.clone()).await;
                    }
                }
            }
        }
    }

    pub async fn part_channel(&self, name: &str, nick: &str, part_message: &str) {
        /* TODO(diath): This should broadcast user prefix and not nick. */
        let message = format!(":{} PART :{}.", nick, part_message);
        if let Some(channel) = self.channels.lock().await.get(name) {
            if channel.part(nick.to_string()).await {
                for target in &*channel.participants.lock().await {
                    if let Some(client) = self.clients.lock().await.get(target) {
                        client.send_raw(message.clone()).await;
                    }
                }
            }
        }
    }

    pub async fn forward_channel_message(&self, sender: String, name: &str, message: String) {
        /* TODO(diath): This should broadcast user prefix and not nick. */
        let message = format!(":{} PRIVMSG {} :{}\r\n", sender, name, message);
        if let Some(channel) = self.channels.lock().await.get(name) {
            println!("[{}] {}: {}", name, sender, message);

            for target in &*channel.participants.lock().await {
                if let Some(client) = self.clients.lock().await.get(target) {
                    client.send_raw(message.clone()).await;
                }
            }
        }
    }
}
