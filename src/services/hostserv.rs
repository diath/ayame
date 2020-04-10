use std::collections::HashMap;
use std::net::SocketAddr;

use async_trait::async_trait;

use tokio::sync::Mutex;

use crate::client::{Client, UserHost};
use crate::cloak::get_cloaked_host;
use crate::service::Service;

pub struct HostServ {
    pub require_activation: bool,
    pub hosts: Mutex<HashMap<String, String>>,
    pub pending: Mutex<HashMap<String, String>>,
}

impl HostServ {
    pub fn new() -> HostServ {
        HostServ {
            require_activation: false,
            hosts: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
        }
    }

    async fn reply(&self, client: &Client, message: &str) {
        let nick = client.nick.lock().await;
        client
            .send_raw(format!(":HostServ@services NOTICE {} :{}", nick, message))
            .await;
    }
}

fn is_vhost_valid(vhost: String) -> bool {
    for chunk in vhost.split(".") {
        if chunk.len() == 0 || !chunk.chars().all(|ch| ch.is_ascii_alphabetic()) {
            return false;
        }
    }

    true
}

#[async_trait]
impl Service for HostServ {
    async fn on_message(&self, client: &Client, params: Vec<&str>) {
        if params.len() < 1 {
            return;
        }

        match params[0].to_ascii_lowercase().as_str() {
            "on" => {
                if *client.identified.lock().await {
                    let nick = client.nick.lock().await.to_string();
                    if let Some(vhost) = self.hosts.lock().await.get(&nick) {
                        (*client.host.lock().await) = UserHost::VHost(vhost.to_string());

                        self.reply(client, &format!("Your vhost of {} is now activated", vhost))
                            .await;
                    } else if let Some(_) = self.pending.lock().await.get(&nick) {
                        self.reply(client, "Your vhost is pending activation").await;
                    } else {
                        self.reply(client, "There is no vhost for your nick").await;
                    }
                } else {
                    self.reply(client, "You are not identified for that nick")
                        .await;
                }
            }
            "off" => {
                if *client.identified.lock().await {
                    let host = match client.address {
                        SocketAddr::V4(addr) => UserHost::IPv4(addr.ip().to_string()),
                        SocketAddr::V6(addr) => UserHost::IPv6(addr.ip().to_string()),
                    };
                    (*client.host.lock().await) = UserHost::VHost(get_cloaked_host(host));
                } else {
                    self.reply(client, "You are not identified for that nick")
                        .await;
                }
            }
            "request" => {
                if params.len() < 2 {
                    self.reply(client, "Not enough params").await;
                } else if *client.identified.lock().await {
                    if !is_vhost_valid(params[1].to_string()) {
                        self.reply(client, "Invalid vhost format specified").await;
                    } else {
                        let nick = client.nick.lock().await.to_string();
                        if self.require_activation {
                            let result = self
                                .pending
                                .lock()
                                .await
                                .insert(nick, params[1].to_string());

                            self.reply(
                                client,
                                "Your vhost has been requested and awaiting activation",
                            )
                            .await;

                            if result.is_some() {
                                self.reply(client, "Your old vhost has been removed").await;
                            }
                        } else {
                            let result =
                                self.hosts.lock().await.insert(nick, params[1].to_string());

                            self.reply(client, "Your vhost has been activated and is ready to use")
                                .await;

                            if result.is_some() {
                                self.reply(client, "Your old vhost has been removed").await;
                            }
                        }
                    }
                } else {
                    self.reply(client, "You are not identified for that nick")
                        .await;
                }
            }
            "activate" => {
                if params.len() < 2 {
                    self.reply(client, "Not enough params").await;
                } else if *client.operator.lock().await {
                    // NOTE(diath): This is a little goofy to prevent a deadlock.
                    let mut vhost = None;
                    if let Some(value) = self.pending.lock().await.get(params[1]) {
                        vhost = Some(value.to_string());
                    }

                    if vhost.is_some() {
                        self.pending.lock().await.remove(params[1]);
                        self.hosts
                            .lock()
                            .await
                            .insert(params[1].to_string(), vhost.unwrap());
                        self.reply(client, "You have activated the requested vhost")
                            .await;
                    } else {
                        self.reply(
                            client,
                            &format!("No pending vhost for nick {} found", params[1]),
                        )
                        .await;
                    }
                } else {
                    self.reply(client, "You are not an IRC operator").await;
                }
            }
            "reject" => {
                if params.len() < 2 {
                    self.reply(client, "Not enough params").await;
                } else if *client.operator.lock().await {
                    if !self.pending.lock().await.contains_key(params[1]) {
                        self.reply(
                            client,
                            &format!("No pending vhost for nick {} found", params[1]),
                        )
                        .await;
                        return;
                    }

                    self.pending.lock().await.remove(params[1]);
                    self.reply(
                        client,
                        &format!(
                            "You have rejected the requested vhost for nick {}",
                            params[1]
                        ),
                    )
                    .await;
                } else {
                    self.reply(client, "You are not an IRC operator").await;
                }
            }
            "waiting" => {
                if *client.operator.lock().await {
                    self.reply(client, "List of pending vhosts:").await;
                    for (nick, vhost) in self.pending.lock().await.iter() {
                        self.reply(client, &format!("{} - {}", nick, vhost)).await;
                    }
                } else {
                    self.reply(client, "You are not an IRC operator").await;
                }
            }
            "del" => {
                if params.len() < 2 {
                    self.reply(client, "Not enough params").await;
                } else if *client.operator.lock().await {
                    if !self.hosts.lock().await.contains_key(params[1]) {
                        self.reply(client, &format!("No vhost for nick {} found", params[1]))
                            .await;
                        return;
                    }

                    self.hosts.lock().await.remove(params[1]);
                    self.reply(
                        client,
                        &format!("You have removed the vhost for nick {}", params[1]),
                    )
                    .await;
                } else {
                    self.reply(client, "You are not an IRC operator").await;
                }
            }
            "help" => {}
            _ => {
                self.reply(client, "Unknown command, try HELP").await;
            }
        }
    }
}
