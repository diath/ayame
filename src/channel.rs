use std::collections::HashSet;
use std::fmt::Write;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::Mutex;

#[derive(Default)]
pub struct ChannelTopic {
    pub text: Mutex<String>,
    pub set_by: Mutex<String>,
    pub set_at: Mutex<u64>,
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
    pub topic: Mutex<Arc<ChannelTopic>>,
    pub modes: ChannelModes,
    pub participants: Mutex<HashSet<String>>,
    pub invites: Mutex<HashSet<String>>,
}

impl Channel {
    pub fn new(name: String) -> Channel {
        Channel {
            name: name,
            topic: Mutex::new(Arc::new(ChannelTopic {
                ..Default::default()
            })),
            modes: ChannelModes {
                invite_only: false,
                password: "".to_string(),
                limit: 0,
                no_external_messages: true,
                secret: false,
            },
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
        let topic = self.topic.lock().await;
        (*topic.text.lock().await) = text;
        (*topic.set_by.lock().await) = sender;

        let now = SystemTime::now();
        match now.duration_since(UNIX_EPOCH) {
            Ok(duration) => (*topic.set_at.lock().await) = duration.as_secs(),
            Err(_) => (*topic.set_at.lock().await) = 0,
        }
    }

    pub fn get_modes_description(&self) -> String {
        let mut desc = "[+".to_string();

        if self.modes.invite_only {
            write!(desc, "i").expect("");
        }

        if self.modes.password.len() != 0 {
            write!(desc, "k").expect("");
        }

        if self.modes.limit != 0 {
            write!(desc, "l").expect("");
        }

        if self.modes.no_external_messages {
            write!(desc, "n").expect("");
        }

        if self.modes.secret {
            write!(desc, "s").expect("");
        }

        write!(desc, "]").expect("");
        desc
    }
}
