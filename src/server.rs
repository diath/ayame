use crate::channel::{Channel, ChannelTopic};
use crate::client::Client;
use crate::replies::NumericReply;

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::Arc;
use std::time::SystemTime;
use std::vec::Vec;

use chrono::prelude::DateTime;
use chrono::Utc;

use tokio::net::TcpListener;
use tokio::sync::Mutex;

pub struct Server {
    pub name: String,
    pub created: String,
    clients: Mutex<HashMap<String, Arc<Client>>>,
    clients_pending: Mutex<Vec<Arc<Client>>>,
    operators: Mutex<HashMap<String, String>>,
    channels: Mutex<HashMap<String, Channel>>,
    motd: Option<Vec<String>>,
}

impl Server {
    pub fn new(name: String) -> Server {
        let dt = DateTime::<Utc>::from(SystemTime::now());

        Server {
            name: name,
            created: dt.format("%Y-%m-%d %H:%M:%S.%f").to_string(),
            clients: Mutex::new(HashMap::new()),
            clients_pending: Mutex::new(vec![]),
            operators: Mutex::new(HashMap::new()),
            channels: Mutex::new(HashMap::new()),
            motd: Server::load_motd("motd.txt"),
        }
    }

    fn load_motd(filename: &str) -> Option<Vec<String>> {
        let file = File::open(filename);
        if !file.is_ok() {
            return None;
        }

        let mut lines = vec![];
        let reader = BufReader::new(file.unwrap());
        for line in reader.lines() {
            lines.push(line.unwrap());
        }

        Some(lines)
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

    pub async fn map_nick(&self, nick: String, client: &Client) {
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
        self.clients.lock().await.insert(nick, c);
    }

    pub async fn remap_nick(&self, old_nick: String, nick: String) {
        if !self.clients.lock().await.contains_key(&old_nick) {
            panic!("remap_nick()");
        }

        if let Some(client) = self.clients.lock().await.remove(&old_nick) {
            self.clients.lock().await.insert(nick, client);
        }
    }

    pub async fn unmap_nick(&self, nick: String) {
        self.clients.lock().await.remove(&nick);
    }

    pub async fn unmap_client(&self, client: &Client) {
        let index = self
            .clients_pending
            .lock()
            .await
            .iter()
            .position(|c| Arc::into_raw(c.clone()) == &*client);
        if index.is_some() {
            self.clients_pending.lock().await.remove(index.unwrap());
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
                .send_raw(format!(":{} PRIVMSG {} :{}", sender, name, message))
                .await;
        }
    }

    pub async fn is_channel_mapped(&self, name: &str) -> bool {
        self.channels
            .lock()
            .await
            .contains_key(name.to_string().to_lowercase().as_str())
    }

    pub async fn create_channel(&self, name: &str) {
        self.channels.lock().await.insert(
            name.to_string().to_lowercase(),
            Channel::new(name.to_string().to_lowercase()),
        );
    }

    pub async fn has_channel_participant(&self, name: &str, nick: &str) -> bool {
        if let Some(channel) = self
            .channels
            .lock()
            .await
            .get(name.to_string().to_lowercase().as_str())
        {
            return channel.has_participant(nick).await;
        }

        false
    }

    pub async fn join_channel(&self, name: &str, nick: &str) -> bool {
        /* TODO(diath): This should broadcast user prefix and not nick. */
        if let Some(channel) = self
            .channels
            .lock()
            .await
            .get(name.to_string().to_lowercase().as_str())
        {
            let message = format!(":{} JOIN {}", nick, name);
            if channel.join(nick.to_string()).await {
                for target in &*channel.participants.lock().await {
                    if let Some(client) = self.clients.lock().await.get(target) {
                        client.send_raw(message.clone()).await;
                    }
                }

                if let Some(client) = self.clients.lock().await.get(nick) {
                    let topic = channel.topic.lock().await;
                    let text = topic.text.lock().await;
                    if text.len() == 0 {
                        client
                            .send_numeric_reply(
                                NumericReply::RplNoTopic,
                                format!("{} :No topic is set", name).to_string(),
                            )
                            .await;
                    } else {
                        client
                            .send_numeric_reply(
                                NumericReply::RplTopic,
                                format!("{} :{}", name, text).to_string(),
                            )
                            .await;

                        let set_by = topic.set_by.lock().await;
                        let set_at = topic.set_at.lock().await;
                        client
                            .send_numeric_reply(
                                NumericReply::RplTopicSet,
                                format!("{} {} {}", name, set_by, set_at).to_string(),
                            )
                            .await;
                    }
                }

                return true;
            }
        }

        false
    }

    pub async fn part_channel(&self, name: &str, nick: &str, part_message: &str) -> bool {
        /* TODO(diath): This should broadcast user prefix and not nick. */
        if let Some(channel) = self
            .channels
            .lock()
            .await
            .get(name.to_string().to_lowercase().as_str())
        {
            let message = format!(":{} PART {} :{}.", nick, name, part_message);
            if channel.part(nick.to_string()).await {
                for target in &*channel.participants.lock().await {
                    if let Some(client) = self.clients.lock().await.get(target) {
                        client.send_raw(message.clone()).await;
                    }
                }

                /* NOTE(diath): We need to send the confirmation to the sending client separately as they are no longer in the channel participant list. */
                if let Some(client) = self.clients.lock().await.get(nick) {
                    client.send_raw(message.clone()).await;
                }

                return true;
            }
        }

        false
    }

    pub async fn forward_channel_message(&self, sender: String, name: &str, message: String) {
        /* TODO(diath): This should broadcast user prefix and not nick. */
        if let Some(channel) = self
            .channels
            .lock()
            .await
            .get(name.to_string().to_lowercase().as_str())
        {
            println!("[{}] {}: {}", name, sender, message);

            let message = format!(":{} PRIVMSG {} :{}", sender, name, message);
            for target in &*channel.participants.lock().await {
                if let Some(client) = self.clients.lock().await.get(target) {
                    if client.get_prefix().await != sender {
                        client.send_raw(message.clone()).await;
                    }
                }
            }
        }
    }

    pub async fn get_channel_topic(&self, name: &str) -> Option<Arc<ChannelTopic>> {
        if let Some(channel) = self
            .channels
            .lock()
            .await
            .get(name.to_string().to_lowercase().as_str())
        {
            return Some(channel.topic.lock().await.clone());
        }

        None
    }

    pub async fn set_channel_topic(&self, sender: String, name: &str, topic: String) {
        /* TODO(diath): This should broadcast user prefix and not nick. */
        if let Some(channel) = self
            .channels
            .lock()
            .await
            .get(name.to_string().to_lowercase().as_str())
        {
            println!("[{}] {} changed topic to {}", name, sender, topic);
            // NOTE(diath): The topic sender should be just the name, not the prefix.
            channel.set_topic(sender.clone(), topic.clone()).await;

            let message = format!(":{} TOPIC {} :{}", sender, name, topic);
            for target in &*channel.participants.lock().await {
                if let Some(client) = self.clients.lock().await.get(target) {
                    if client.get_prefix().await != sender {
                        client.send_raw(message.clone()).await;
                    }
                }
            }
        }
    }

    pub async fn remove_from_channels(&self, client: &Client) {
        let nick = client.nick.lock().await;
        for channel_name in &*client.channels.lock().await {
            if let Some(channel) = self.channels.lock().await.get(channel_name) {
                channel.remove(nick.to_string()).await;
            }
        }
    }

    pub async fn send_motd(&self, client: &Client) {
        if let Some(motd) = &self.motd {
            client
                .send_numeric_reply(
                    NumericReply::RplMotdStart,
                    format!(":- {} Message of the day - ", self.name).to_string(),
                )
                .await;

            for line in motd {
                client
                    .send_numeric_reply(NumericReply::RplMotd, format!(":- {}", line).to_string())
                    .await;
            }

            client
                .send_numeric_reply(
                    NumericReply::RplEndOfMotd,
                    ":End of MOTD command".to_string(),
                )
                .await;
        } else {
            client
                .send_numeric_reply(NumericReply::ErrNoMotd, ":MOTD File is missing".to_string())
                .await;
        }
    }

    pub async fn broadcast_quit(&self, client: &Client, reason: &str) {
        let message = format!(":{} QUIT :{}", client.get_prefix().await, reason);
        let mut targets = HashSet::new();

        for channel_name in &*client.channels.lock().await {
            if let Some(channel) = self.channels.lock().await.get(channel_name) {
                for target in &*channel.participants.lock().await {
                    targets.insert(target.clone());
                }
            }
        }

        for target in targets {
            if let Some(client) = self.clients.lock().await.get(&target) {
                client.send_raw(message.clone()).await;
            }
        }
    }
}
