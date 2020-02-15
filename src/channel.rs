use std::collections::HashSet;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::Mutex;

#[derive(Default)]
pub struct ChannelTopic {
    pub text: Mutex<String>,
    pub set_by: Mutex<String>,
    pub set_at: Mutex<u64>,
}

pub struct Channel {
    pub name: String,
    pub participants: Mutex<HashSet<String>>,
    pub topic: Mutex<Arc<ChannelTopic>>,
}

impl Channel {
    pub fn new(name: String) -> Channel {
        Channel {
            name: name,
            topic: Mutex::new(Arc::new(ChannelTopic {
                ..Default::default()
            })),
            participants: Mutex::new(HashSet::new()),
        }
    }

    pub async fn has_participant(&self, name: &str) -> bool {
        self.participants.lock().await.contains(name)
    }

    pub async fn join(&self, name: String) -> bool {
        if !self.has_participant(name.as_str()).await {
            self.participants.lock().await.insert(name.clone());
            println!("[{}] {} joined.", self.name, name);
            return true;
        }

        false
    }

    pub async fn part(&self, name: String) -> bool {
        if self.has_participant(name.as_str()).await {
            self.participants.lock().await.remove(&name);
            println!("[{}] {} left.", self.name, name.clone());
            return true;
        }

        false
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
}
