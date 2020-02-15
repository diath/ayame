use std::collections::HashSet;
use tokio::sync::Mutex;

pub struct Channel {
    pub name: String,
    pub participants: Mutex<HashSet<String>>,
}

impl Channel {
    pub fn new(name: String) -> Channel {
        Channel {
            name: name,
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
}
