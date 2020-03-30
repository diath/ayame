use std::collections::HashMap;

use async_trait::async_trait;

use tokio::sync::Mutex;

use crate::client::Client;
use crate::service::Service;

pub struct NickServ {
    pub nicks: Mutex<HashMap<String, String>>,
}

impl NickServ {
    async fn reply(&self, client: &Client, message: &str) {
        let nick = client.nick.lock().await;
        client
            .send_raw(format!(":NickServ@services NOTICE {} :{}", nick, message))
            .await;
    }
}

#[async_trait]
impl Service for NickServ {
    async fn on_message(&self, client: &Client, params: Vec<&str>) {
        if params.len() < 1 {
            return;
        }

        match params[0].to_ascii_lowercase().as_str() {
            "register" => {
                if params.len() < 3 {
                    self.reply(client, "Not enough params").await;
                } else {
                    if self.nicks.lock().await.contains_key(params[1]) {
                        self.reply(client, "Nick already taken").await;
                    } else {
                        self.nicks
                            .lock()
                            .await
                            .insert(params[1].to_string(), params[2].to_string());
                        self.reply(client, "Nick successfully registered").await;
                    }
                }
            }
            "identify" => {
                if let Some(password) = self.nicks.lock().await.get(params[1]) {
                    if password == params[2] {
                        self.reply(client, "You are now identified for this nick")
                            .await;
                    } else {
                        self.reply(client, "Wrong password").await;
                    }
                } else {
                    self.reply(client, "Nick not registered").await;
                }
            }
            _ => {
                self.reply(client, "Unknown command").await;
            }
        }
    }
}
