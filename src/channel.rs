use crate::client::Client;
use crate::replies::NumericReply;

use std::collections::HashSet;
use std::fmt::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::Mutex;

#[derive(Default)]
pub struct ChannelTopic {
    pub text: String,
    pub set_by: String,
    pub set_at: u64,
}

pub struct ChannelModes {
    pub invite_only: bool,
    pub password: String,
    pub limit: usize,
    pub no_external_messages: bool,
    pub secret: bool,
}

pub struct Channel {
    pub name: String,
    pub topic: Mutex<ChannelTopic>,
    pub modes: Mutex<ChannelModes>,
    pub participants: Mutex<HashSet<String>>,
    pub invites: Mutex<HashSet<String>>,
}

impl Channel {
    pub fn new(name: String) -> Channel {
        Channel {
            name: name,
            topic: Mutex::new(ChannelTopic {
                ..Default::default()
            }),
            modes: Mutex::new(ChannelModes {
                invite_only: false,
                password: "".to_string(),
                limit: 0,
                no_external_messages: true,
                secret: false,
            }),
            participants: Mutex::new(HashSet::new()),
            invites: Mutex::new(HashSet::new()),
        }
    }

    pub async fn has_participant(&self, name: &str) -> bool {
        self.participants.lock().await.contains(name)
    }

    pub async fn is_invited(&self, name: &str) -> bool {
        self.invites.lock().await.contains(name)
    }

    pub async fn part(&self, name: String) -> bool {
        if self.has_participant(name.as_str()).await {
            self.participants.lock().await.remove(&name);
            println!("[{}] {} left.", self.name, name.clone());
            return true;
        }

        false
    }

    pub async fn remove(&self, name: String) {
        self.participants.lock().await.remove(&name);
    }

    pub async fn set_topic(&self, sender: String, text: String) {
        let mut topic = self.topic.lock().await;
        topic.text = text;
        topic.set_by = sender;

        let now = SystemTime::now();
        match now.duration_since(UNIX_EPOCH) {
            Ok(duration) => topic.set_at = duration.as_secs(),
            Err(_) => topic.set_at = 0,
        }
    }

    pub async fn get_modes_description(&self, with_params: bool) -> String {
        let mut desc = "+".to_string();
        let modes = self.modes.lock().await;

        if modes.invite_only {
            write!(desc, "i").expect("");
        }

        if modes.password.len() != 0 {
            write!(desc, "k").expect("");
        }

        if modes.limit != 0 {
            write!(desc, "l").expect("");
        }

        if modes.no_external_messages {
            write!(desc, "n").expect("");
        }

        if modes.secret {
            write!(desc, "s").expect("");
        }

        if with_params {
            let mut params = vec![];
            if modes.password.len() != 0 {
                params.push(modes.password.clone());
            }

            if modes.limit != 0 {
                params.push(modes.limit.to_string());
            }

            write!(desc, " {}", params.join(" ")).expect("");
        }

        desc
    }

    pub async fn toggle_modes(&self, client: &Client, params: Vec<String>) -> String {
        if params.len() < 1 {
            panic!("toggle_modes()");
        }

        let chars = params[0].chars();
        let params = params[1..].to_vec();

        let mut flag = false;
        let mut index: usize = 0;

        let mut changes = String::new();
        let mut changes_params = vec![];

        for ch in chars {
            match ch {
                '+' => {
                    flag = true;
                    changes.push('+');
                }
                '-' => {
                    flag = false;
                    changes.push('-');
                }
                'i' => {
                    self.modes.lock().await.invite_only = flag;
                    changes.push('i');
                }
                'k' => {
                    changes.push('k');
                    if flag {
                        if let Some(param) = params.get(index) {
                            if self.modes.lock().await.password.to_string().len() > 0 {
                                client
                                    .send_numeric_reply(
                                        NumericReply::ErrKeySet,
                                        format!("{} :Channel key already set", self.name),
                                    )
                                    .await;
                            } else {
                                self.modes.lock().await.password = param.to_string();
                                changes_params.push(param.to_string());
                            }
                        } else {
                            client
                                .send_numeric_reply(
                                    NumericReply::ErrNeedMoreParams,
                                    "MODE :Not enough parameters".to_string(),
                                )
                                .await;
                        }
                    } else {
                        self.modes.lock().await.password.clear();
                    }
                    index += 1;
                }
                'l' => {
                    changes.push('l');
                    if flag {
                        if let Some(param) = params.get(index) {
                            match param.to_string().parse::<usize>() {
                                Ok(limit) => self.modes.lock().await.limit = limit,
                                Err(_) => self.modes.lock().await.limit = 0,
                            }

                            changes_params.push(self.modes.lock().await.limit.to_string());
                        } else {
                            client
                                .send_numeric_reply(
                                    NumericReply::ErrNeedMoreParams,
                                    "MODE :Not enough parameters".to_string(),
                                )
                                .await;
                        }
                    } else {
                        self.modes.lock().await.limit = 0;
                    }
                    index += 1;
                }
                'n' => {
                    self.modes.lock().await.no_external_messages = flag;
                    changes.push('n');
                }
                's' => {
                    self.modes.lock().await.secret = flag;
                    changes.push('s');
                }
                _ => {
                    client
                        .send_numeric_reply(
                            NumericReply::ErrUnknownMode,
                            format!("{} :Unknown mode", ch),
                        )
                        .await;
                }
            }
        }

        if changes.len() > 0 && changes_params.len() > 0 {
            changes.push(' ');
        }

        changes.push_str(&changes_params.join(" "));
        changes
    }
}
