use crate::client::Client;
use crate::replies::NumericReply;

use std::collections::{HashMap, HashSet};
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

pub struct ChannelUserModes {
    pub owner: bool,
    pub admin: bool,
    pub operator: bool,
    pub half_operator: bool,
    pub voiced: bool,
}

pub struct Channel {
    pub name: String,
    pub topic: Mutex<ChannelTopic>,
    pub modes: Mutex<ChannelModes>,
    pub participants: Mutex<HashMap<String, ChannelUserModes>>,
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
            participants: Mutex::new(HashMap::new()),
            invites: Mutex::new(HashSet::new()),
        }
    }

    pub async fn has_participant(&self, name: &str) -> bool {
        self.participants.lock().await.contains_key(name)
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

        let mut modes = self.modes.lock().await;

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
                /* Channel modes */
                'i' => {
                    if modes.invite_only != flag {
                        modes.invite_only = flag;
                        changes.push('i');
                    }
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
                                modes.password = param.to_string();
                                changes.push('k');
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
                        modes.password.clear();
                        changes.push('k');
                    }
                    index += 1;
                }
                'l' => {
                    if flag {
                        if let Some(param) = params.get(index) {
                            let prev = modes.limit;
                            match param.to_string().parse::<usize>() {
                                Ok(limit) => modes.limit = limit,
                                Err(_) => modes.limit = 0,
                            }

                            if prev != modes.limit {
                                changes.push('l');
                                changes_params.push(modes.limit.to_string());
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
                        modes.limit = 0;
                        changes.push('l');
                    }
                    index += 1;
                }
                'n' => {
                    if modes.no_external_messages != flag {
                        modes.no_external_messages = flag;
                        changes.push('n');
                    }
                }
                's' => {
                    if modes.secret != flag {
                        modes.secret = flag;
                        changes.push('s');
                    }
                }
                /* Channel user modes */
                'q' | 'a' | 'o' | 'h' | 'v' => {
                    if let Some(param) = params.get(index) {
                        if self.toggle_user_mode(&param, ch, flag).await {
                            changes.push(ch);
                            changes_params.push(param.to_string());
                        }
                    }
                    index += 1;
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

    pub async fn toggle_user_mode(&self, nick: &str, mode: char, flag: bool) -> bool {
        let mut participants = self.participants.lock().await;
        if let Some(modes) = participants.get_mut(nick) {
            match mode {
                'q' => {
                    if modes.owner != flag {
                        modes.owner = flag;
                        return true;
                    }
                }
                'a' => {
                    if modes.admin != flag {
                        modes.admin = flag;
                        return true;
                    }
                }
                'o' => {
                    if modes.operator != flag {
                        modes.operator = flag;
                        return true;
                    }
                }
                'h' => {
                    if modes.half_operator != flag {
                        modes.half_operator = flag;
                        return true;
                    }
                }
                'v' => {
                    if modes.voiced != flag {
                        modes.voiced = flag;
                        return true;
                    }
                }
                _ => {
                    return false;
                }
            }
        }

        false
    }
}
