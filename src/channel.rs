use crate::client::Client;
use crate::replies::NumericReply;

use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::{Mutex, RwLock};

#[derive(Default)]
pub struct ChannelTopic {
    pub text: String,
    pub set_by: String,
    pub set_at: u64,
}

pub struct ChannelModes {
    pub moderated: bool,
    pub invite_only: bool,
    pub password: String,
    pub limit: usize,
    pub no_external_messages: bool,
    pub secret: bool,
    pub restrict_topic: bool,
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
    pub participants: RwLock<HashMap<String, ChannelUserModes>>,
    pub invites: Mutex<HashSet<String>>,
    pub bans: Mutex<HashSet<String>>,
    pub ban_exceptions: Mutex<HashSet<String>>,
}

impl ChannelUserModes {
    pub fn is_owner(&self) -> bool {
        return self.owner;
    }

    pub fn is_admin(&self, explicit: bool) -> bool {
        if explicit {
            return self.admin;
        }

        return self.owner || self.admin;
    }

    pub fn is_operator(&self, explicit: bool) -> bool {
        if explicit {
            return self.operator;
        }

        return self.owner || self.admin || self.operator;
    }

    pub fn is_half_operator(self: &ChannelUserModes, explicit: bool) -> bool {
        if explicit {
            return self.half_operator;
        }

        return self.owner || self.admin || self.operator || self.half_operator;
    }

    pub fn is_voiced(&self, explicit: bool) -> bool {
        if explicit {
            return self.voiced;
        }

        return self.owner || self.admin || self.operator || self.half_operator || self.voiced;
    }

    pub fn get_prefix(&self) -> &str {
        if self.is_owner() {
            return "~";
        } else if self.is_admin(false) {
            return "&";
        } else if self.is_operator(false) {
            return "@";
        } else if self.is_half_operator(false) {
            return "%";
        } else if self.is_voiced(false) {
            return "+";
        }

        return "";
    }
}

impl Channel {
    pub fn new(name: String) -> Channel {
        Channel {
            name: name,
            topic: Mutex::new(ChannelTopic {
                ..Default::default()
            }),
            modes: Mutex::new(ChannelModes {
                moderated: false,
                invite_only: false,
                password: "".to_string(),
                limit: 0,
                no_external_messages: true,
                secret: false,
                restrict_topic: true,
            }),
            participants: RwLock::new(HashMap::new()),
            invites: Mutex::new(HashSet::new()),
            bans: Mutex::new(HashSet::new()),
            ban_exceptions: Mutex::new(HashSet::new()),
        }
    }

    pub async fn has_participant(&self, name: &str) -> bool {
        self.participants.read().await.contains_key(name)
    }

    pub async fn is_invited(&self, name: &str) -> bool {
        self.invites.lock().await.contains(name)
    }

    pub async fn is_banned(&self, prefix: &str) -> bool {
        self.bans.lock().await.contains(prefix)
    }

    pub async fn is_ban_exempt(&self, prefix: &str) -> bool {
        self.ban_exceptions.lock().await.contains(prefix)
    }

    pub async fn part(&self, name: String) -> bool {
        if self.has_participant(name.as_str()).await {
            self.participants.write().await.remove(&name);
            println!("[{}] {} left.", self.name, name.clone());
            return true;
        }

        false
    }

    pub async fn remove(&self, name: String) {
        self.participants.write().await.remove(&name);
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

        if modes.moderated {
            write!(desc, "m").expect("");
        }

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

        if modes.restrict_topic {
            write!(desc, "t").expect("");
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
                'm' => {
                    if modes.moderated != flag {
                        modes.moderated = flag;
                        changes.push('m');
                    }
                }
                'i' => {
                    if modes.invite_only != flag {
                        modes.invite_only = flag;
                        changes.push('i');
                    }
                }
                'k' => {
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
                't' => {
                    if modes.restrict_topic != flag {
                        modes.restrict_topic = flag;
                        changes.push('t');
                    }
                }
                'b' => {
                    if let Some(param) = params.get(index) {
                        if flag {
                            if self.bans.lock().await.insert(param.to_string()) {
                                changes.push('b');
                                changes_params.push(param.to_string());
                            }
                        } else {
                            if self.bans.lock().await.remove(param) {
                                changes.push('b');
                                changes_params.push(param.to_string());
                            }
                        }
                    }
                    index += 1;
                }
                'e' => {
                    if let Some(param) = params.get(index) {
                        if flag {
                            if self.ban_exceptions.lock().await.insert(param.to_string()) {
                                changes.push('e');
                                changes_params.push(param.to_string());
                            }
                        } else {
                            if self.ban_exceptions.lock().await.remove(param) {
                                changes.push('e');
                                changes_params.push(param.to_string());
                            }
                        }
                    }
                    index += 1;
                }
                /* Channel user modes */
                'q' | 'a' | 'o' | 'h' | 'v' => {
                    if let Some(param) = params.get(index) {
                        let nick = client.nick.lock().await.to_string();
                        let oper = *client.operator.lock().await;
                        if oper || self.can_toggle_user_mode(&nick, ch, flag).await {
                            if self.toggle_user_mode(&param, ch, flag).await {
                                changes.push(ch);
                                changes_params.push(param.to_string());
                            }
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

    pub async fn can_toggle_user_mode(&self, set_by: &str, mode: char, flag: bool) -> bool {
        if let Some(modes) = self.participants.read().await.get(set_by) {
            match mode {
                'q' => {
                    return modes.is_owner();
                }
                'a' => {
                    return modes.is_admin(false);
                }
                'o' => {
                    return modes.is_operator(false);
                }
                'h' => {
                    return modes.is_half_operator(false);
                }
                'v' => {
                    if flag {
                        return modes.is_half_operator(false);
                    } else {
                        return modes.is_voiced(false);
                    }
                }
                _ => {
                    return false;
                }
            }
        }

        false
    }

    pub async fn toggle_user_mode(&self, nick: &str, mode: char, flag: bool) -> bool {
        if let Some(modes) = self.participants.write().await.get_mut(nick) {
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

    pub async fn has_access(&self, nick: &str, other: &str) -> bool {
        if let Some(modes) = self.participants.read().await.get(nick) {
            if let Some(modes_other) = self.participants.read().await.get(other) {
                if modes.is_owner() {
                    return true;
                }

                if modes.is_admin(false) {
                    return !modes_other.is_owner();
                }

                if modes.is_operator(false) {
                    return !modes_other.is_admin(false);
                }

                if modes.is_half_operator(false) {
                    return !modes_other.is_operator(false);
                }
            }

            return false;
        }

        false
    }

    pub async fn is_operator(&self, nick: &str) -> bool {
        if let Some(modes) = self.participants.read().await.get(nick) {
            return modes.is_operator(false);
        }

        false
    }

    pub async fn is_voiced(&self, nick: &str) -> bool {
        if let Some(modes) = self.participants.read().await.get(nick) {
            return modes.is_voiced(false);
        }

        false
    }
}
