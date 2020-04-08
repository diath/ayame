use std::collections::HashMap;

use async_trait::async_trait;

use tokio::sync::Mutex;

use crate::client::Client;
use crate::service::Service;

pub struct NickServ {
    pub nicks: Mutex<HashMap<String, String>>,
}

impl NickServ {
    pub fn new() -> NickServ {
        NickServ {
            nicks: Mutex::new(HashMap::new()),
        }
    }

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
                        let nick = client.nick.lock().await.to_string();
                        if nick == params[1] {
                            self.nicks
                                .lock()
                                .await
                                .insert(params[1].to_string(), params[2].to_string());
                            self.reply(client, "Nick successfully registered").await;
                        } else {
                            self.reply(client, "You can only register your current nick")
                                .await;
                        }
                    }
                }
            }
            "identify" => {
                if params.len() < 3 {
                    self.reply(client, "Not enough params").await;
                } else if *client.identified.lock().await {
                    self.reply(client, "You are already identified").await;
                } else if let Some(password) = self.nicks.lock().await.get(params[1]) {
                    if password == params[2] {
                        (*client.identified.lock().await) = true;
                        self.reply(client, "You are now identified for this nick")
                            .await;
                    } else {
                        self.reply(client, "Wrong password").await;
                    }
                } else {
                    self.reply(client, "Nick not registered").await;
                }
            }
            "logout" => {
                let identified = *client.identified.lock().await;
                if identified {
                    (*client.identified.lock().await) = false;
                    self.reply(client, "You are no longer identified").await;
                } else {
                    self.reply(client, "You are not identified").await;
                }
            }
            "drop" => {
                if params.len() < 3 {
                    self.reply(client, "Not enough params").await;
                } else if *client.identified.lock().await {
                    self.reply(client, "You must logout before dropping a nick")
                        .await;
                } else {
                    let mut password = None;
                    if let Some(_password) = self.nicks.lock().await.get(params[1]) {
                        password = Some(_password.clone());
                    }

                    if let Some(password) = password {
                        if password == params[2] {
                            self.nicks.lock().await.remove(params[1]);
                            self.reply(client, "The nick registration has been released")
                                .await;
                        } else {
                            self.reply(client, "Wrong password").await;
                        }
                    } else {
                        self.reply(client, "Nick not registered").await;
                    }
                }
            }
            "help" => {
                self.reply(client, "NickServ commands:").await;
                self.reply(client, "REGISTER <nick> <password>").await;
                self.reply(client, "IDENTIFY <nick> <password>").await;
                self.reply(client, "LOGOUT").await;
                self.reply(client, "DROP <nick> <password>").await;
                self.reply(client, "HELP").await;
            }
            _ => {
                self.reply(client, "Unknown command, try HELP").await;
            }
        }
    }
}
