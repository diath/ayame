use crate::ayame::*;
use crate::channel::{Channel, ChannelUserModes};
use crate::client::Client;
use crate::config::Config;
use crate::replies::NumericReply;

use std::collections::{HashMap, HashSet};
use std::fs::{read_to_string, File};
use std::io::{BufRead, BufReader};
use std::net::SocketAddr;
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
    address: SocketAddr,
    clients: Mutex<HashMap<String, Arc<Client>>>,
    clients_pending: Mutex<Vec<Arc<Client>>>,
    operators: Mutex<HashMap<String, String>>,
    channels: Mutex<HashMap<String, Channel>>,
    motd: Mutex<Option<Vec<String>>>,
}

impl Server {
    pub fn new() -> Server {
        let dt = DateTime::<Utc>::from(SystemTime::now());
        let config = Server::load_config();

        let name = config.server.name.unwrap_or(IRCD_NAME.to_string());
        let motd_path = config.server.motd_path.unwrap_or(IRCD_MOTD.to_string());
        let host = config.server.host.unwrap_or("127.0.0.1".to_string());
        let port = config.server.port.unwrap_or(6667);

        println!("Server: {}", name);
        println!("Address: {}:{}", host, port);

        Server {
            name: name,
            created: dt.format("%Y-%m-%d %H:%M:%S.%f").to_string(),
            address: format!("{}:{}", host, port).parse().unwrap(),
            clients: Mutex::new(HashMap::new()),
            clients_pending: Mutex::new(vec![]),
            operators: Mutex::new(HashMap::new()),
            channels: Mutex::new(HashMap::new()),
            motd: Mutex::new(Server::load_motd(&motd_path)),
        }
    }

    fn load_config() -> Config {
        match read_to_string(IRCD_CONFIG) {
            Ok(s) => match toml::from_str(&s) {
                Ok(config) => config,
                Err(error) => {
                    eprintln!("Config parse error: {}", error);
                    Config {
                        ..Default::default()
                    }
                }
            },
            Err(_) => Config {
                ..Default::default()
            },
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

    pub async fn reload_motd(&self) {
        (*self.motd.lock().await) = Server::load_motd("motd.txt");
    }

    pub async fn accept(self) -> Result<(), Box<dyn std::error::Error>> {
        let server = Arc::new(self);
        let mut acceptor = TcpListener::bind(server.address).await?;
        println!("Listening...");

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
            panic!("map_nick()");
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

    pub async fn join_channel(&self, name: &str, password: String, client: &Client) -> bool {
        /* TODO(diath): This should broadcast user prefix and not nick. */
        if let Some(channel) = self
            .channels
            .lock()
            .await
            .get(name.to_string().to_lowercase().as_str())
        {
            let oper = *client.operator.lock().await;
            let nick = client.nick.lock().await.to_string();
            if channel.has_participant(&nick).await {
                return false;
            }

            let mut participants = channel.participants.write().await;

            // NOTE(diath): Operators are exempt from join limits.
            if !oper {
                let modes = channel.modes.lock().await;
                if modes.limit != 0 && participants.len() >= modes.limit {
                    client
                        .send_numeric_reply(
                            NumericReply::ErrChannelIsFull,
                            format!("{} :Cannot join channel (+l)", name).to_string(),
                        )
                        .await;
                    return false;
                }

                if modes.password.len() != 0 && modes.password != password {
                    client
                        .send_numeric_reply(
                            NumericReply::ErrBadChannelKey,
                            format!("{} :Cannot join channel (+k)", name).to_string(),
                        )
                        .await;
                    return false;
                }

                if modes.invite_only && !channel.is_invited(&nick).await {
                    client
                        .send_numeric_reply(
                            NumericReply::ErrInviteOnlyChan,
                            format!("{} :Cannot join channel (+i)", name).to_string(),
                        )
                        .await;
                    return false;
                }
            }

            let mut operator = false;
            if participants.len() == 0 {
                operator = true;
            }

            participants.insert(
                nick.clone(),
                ChannelUserModes {
                    owner: false,
                    admin: false,
                    operator: operator,
                    half_operator: false,
                    voiced: false,
                },
            );

            let message = format!(":{} JOIN {}", nick, name);
            for target in participants.keys() {
                if let Some(client) = self.clients.lock().await.get(target) {
                    client.send_raw(message.clone()).await;
                }
            }

            if let Some(client) = self.clients.lock().await.get(&nick) {
                let topic = channel.topic.lock().await;
                let text = &topic.text;
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

                    client
                        .send_numeric_reply(
                            NumericReply::RplTopicSet,
                            format!("{} {} {}", name, topic.set_by, topic.set_at).to_string(),
                        )
                        .await;
                }

                println!("[{}] {} joined.", name, nick);
                return true;
            }
        }

        false
    }

    pub async fn part_channel(&self, name: &str, nick: &str, part_message: &str) -> bool {
        /* TODO(diath): This should broadcast user prefix and not nick. */
        let mut result = false;
        let mut remove = false;

        if let Some(channel) = self
            .channels
            .lock()
            .await
            .get(name.to_string().to_lowercase().as_str())
        {
            if channel.part(nick.to_string()).await {
                let message = format!(":{} PART {} :{}", nick, name, part_message);

                for target in channel.participants.read().await.keys() {
                    if let Some(client) = self.clients.lock().await.get(target) {
                        client.send_raw(message.clone()).await;
                    }
                }

                /* NOTE(diath): We need to send the confirmation to the sending client separately as they are no longer in the channel participant list. */
                if let Some(client) = self.clients.lock().await.get(nick) {
                    client.send_raw(message.clone()).await;
                }

                if channel.participants.read().await.len() == 0 {
                    remove = true;
                }

                result = true;
            }
        }

        if remove {
            self.channels
                .lock()
                .await
                .remove(name.to_string().to_lowercase().as_str());
        }

        result
    }

    pub async fn invite_channel(&self, client: &Client, channel_name: &str, invited: &str) -> bool {
        if let Some(channel) = self
            .channels
            .lock()
            .await
            .get(channel_name.to_string().to_lowercase().as_str())
        {
            let nick = client.nick.lock().await.to_string();
            let oper = *client.operator.lock().await;
            if !oper && !channel.is_operator(&nick).await {
                client
                    .send_numeric_reply(
                        NumericReply::ErrChanOpPrivsNeeded,
                        format!("{} :You're not channel operator", channel_name).to_string(),
                    )
                    .await;
                return false;
            }

            channel.invites.lock().await.insert(invited.to_string());
            return true;
        }

        false
    }

    pub async fn kick_channel(
        &self,
        client: &Client,
        channel_name: &str,
        kicked: &str,
        kick_message: String,
    ) -> bool {
        /* TODO(diath): This should broadcast user prefix and not nick. */
        let mut result = false;
        let mut remove = false;

        if let Some(channel) = self
            .channels
            .lock()
            .await
            .get(channel_name.to_string().to_lowercase().as_str())
        {
            let nick = client.nick.lock().await.to_string();
            let oper = *client.operator.lock().await;
            if !oper && !channel.is_operator(&nick).await {
                client
                    .send_numeric_reply(
                        NumericReply::ErrChanOpPrivsNeeded,
                        format!("{} :You're not channel operator", channel_name).to_string(),
                    )
                    .await;
                return false;
            }

            if oper || nick == kicked.to_string() || channel.has_access(&nick, kicked).await {
                let message = format!(
                    ":{} KICK {} {} :{}",
                    nick, channel_name, kicked, kick_message
                );
                for target in channel.participants.read().await.keys() {
                    if let Some(client) = self.clients.lock().await.get(target) {
                        client.send_raw(message.clone()).await;
                    }
                }

                channel.remove(kicked.to_string()).await;
                if channel.participants.read().await.len() == 0 {
                    remove = true;
                }

                result = true;
            }
        }

        if remove {
            self.channels
                .lock()
                .await
                .remove(channel_name.to_string().to_lowercase().as_str());
        }

        result
    }

    pub async fn forward_channel_message(&self, client: &Client, name: &str, message: String) {
        /* TODO(diath): This should broadcast user prefix and not nick. */
        if let Some(channel) = self
            .channels
            .lock()
            .await
            .get(name.to_string().to_lowercase().as_str())
        {
            let prefix = client.get_prefix().await;
            let nick = client.nick.lock().await.to_string();

            // NOTE(diath): Operators can always send messages to any channel.
            if !*client.operator.lock().await {
                let modes = channel.modes.lock().await;
                if modes.no_external_messages && !channel.has_participant(&nick).await {
                    client
                        .send_numeric_reply(
                            NumericReply::ErrCannotSendToChan,
                            format!("{} :No external messages allowed ({})", &name, &name)
                                .to_string(),
                        )
                        .await;
                    return;
                }

                if modes.moderated && !channel.is_voiced(&nick).await {
                    client
                        .send_numeric_reply(
                            NumericReply::ErrCannotSendToChan,
                            format!("{} :You need voice (+v) ({})", &name, &name).to_string(),
                        )
                        .await;
                    return;
                }
            }

            println!("[{}] {}: {}", name, prefix, message);

            let message = format!(":{} PRIVMSG {} :{}", prefix, name, message);
            for target in channel.participants.read().await.keys() {
                if let Some(client) = self.clients.lock().await.get(target) {
                    if client.get_prefix().await != prefix {
                        client.send_raw(message.clone()).await;
                    }
                }
            }
        } else {
            client
                .send_numeric_reply(
                    NumericReply::ErrNoSuchChannel,
                    format!("{} :No such nick/channel", &name).to_string(),
                )
                .await;
        }
    }

    pub async fn get_channel_topic(&self, client: &Client, channel_name: &str) {
        if let Some(channel) = self.channels.lock().await.get(channel_name) {
            let topic = channel.topic.lock().await;
            let text = &topic.text;
            if text.len() == 0 {
                client
                    .send_numeric_reply(
                        NumericReply::RplNoTopic,
                        format!("{} :No topic is set", channel_name).to_string(),
                    )
                    .await;
            } else {
                client
                    .send_numeric_reply(
                        NumericReply::RplTopic,
                        format!("{} :{}", channel_name, text).to_string(),
                    )
                    .await;

                client
                    .send_numeric_reply(
                        NumericReply::RplTopicSet,
                        format!("{} {} {}", channel_name, topic.set_by, topic.set_at).to_string(),
                    )
                    .await;
            }
        } else {
            client
                .send_numeric_reply(
                    NumericReply::ErrNoSuchChannel,
                    format!("{} :No such channel", channel_name).to_string(),
                )
                .await;
        }
    }

    pub async fn set_channel_topic(&self, client: &Client, channel_name: &str, topic: String) {
        /* TODO(diath): This should broadcast user prefix and not nick. */
        if let Some(channel) = self
            .channels
            .lock()
            .await
            .get(channel_name.to_string().to_lowercase().as_str())
        {
            let nick = client.nick.lock().await.to_string();
            let oper = *client.operator.lock().await;
            if !oper
                && channel.modes.lock().await.restrict_topic
                && !channel.is_operator(&nick).await
            {
                client
                    .send_numeric_reply(
                        NumericReply::ErrChanOpPrivsNeeded,
                        format!("{} :You're not channel operator", channel_name).to_string(),
                    )
                    .await;
                return;
            }

            println!("[{}] {} changed topic to {}", channel_name, nick, topic);
            // NOTE(diath): The topic sender should be just the name, not the prefix.
            channel.set_topic(nick.to_string(), topic.clone()).await;

            let message = format!(":{} TOPIC {} :{}", nick, channel_name, topic);
            for target in channel.participants.read().await.keys() {
                if let Some(client) = self.clients.lock().await.get(target) {
                    if client.get_prefix().await != nick.to_string() {
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

    pub async fn send_names(&self, client: &Client, channel_name: String) {
        let nick = client.nick.lock().await.to_string();
        let is_operator = *client.operator.lock().await;

        if !self.is_channel_mapped(&channel_name).await {
            return;
        }

        if let Some(channel) = self.channels.lock().await.get(&channel_name) {
            let has_participant = channel.has_participant(&nick).await;
            if is_operator || has_participant {
                let mut names = vec![];
                for (name, modes) in &*channel.participants.read().await {
                    names.push(format!("{}{}", modes.get_prefix(), name));
                }

                client
                    .send_numeric_reply(
                        NumericReply::RplNamReply,
                        format!("= {} :{}", channel_name, names.join(" ")),
                    )
                    .await;
            }
        }

        client
            .send_numeric_reply(
                NumericReply::RplEndOfNames,
                format!("{} :End of /NAMES list.", channel_name),
            )
            .await;
    }

    pub async fn send_list(&self, client: &Client, channels: Option<String>) {
        client
            .send_numeric_reply(NumericReply::RplListStart, format!("Channel :Users  Name"))
            .await;

        if let Some(names) = channels {
            for channel_name in names.split(",") {
                if channel_name.len() == 0 {
                    continue;
                }

                if let Some(channel) = self.channels.lock().await.get(channel_name) {
                    let oper = *client.operator.lock().await;
                    let nick = client.nick.lock().await.to_string();
                    let topic = channel.topic.lock().await;
                    let participants = channel.participants.read().await;

                    if channel.modes.lock().await.secret
                        && !oper
                        && !participants.contains_key(&nick)
                    {
                        continue;
                    }

                    client
                        .send_numeric_reply(
                            NumericReply::RplList,
                            format!(
                                "{} {} :[{}] {}",
                                channel.name,
                                participants.len(),
                                channel.get_modes_description(oper).await,
                                topic.text
                            ),
                        )
                        .await;
                }
            }
        } else {
            for (_, channel) in &*self.channels.lock().await {
                let oper = *client.operator.lock().await;
                let nick = client.nick.lock().await.to_string();
                let topic = channel.topic.lock().await;
                let participants = channel.participants.read().await;

                if channel.modes.lock().await.secret && !oper && !participants.contains_key(&nick) {
                    continue;
                }

                client
                    .send_numeric_reply(
                        NumericReply::RplList,
                        format!(
                            "{} {} :{} {}",
                            channel.name,
                            participants.len(),
                            channel.get_modes_description(oper).await,
                            topic.text
                        ),
                    )
                    .await;
            }
        }

        client
            .send_numeric_reply(NumericReply::RplListEnd, format!(":End of /LIST"))
            .await;
    }

    pub async fn send_motd(&self, client: &Client) {
        if let Some(motd) = &*self.motd.lock().await {
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
        let mut targets = HashSet::new();

        for channel_name in &*client.channels.lock().await {
            if let Some(channel) = self.channels.lock().await.get(channel_name) {
                for target in channel.participants.read().await.keys() {
                    targets.insert(target.clone());
                }
            }
        }

        let message = format!(":{} QUIT :{}", client.get_prefix().await, reason);
        for target in targets {
            if let Some(client) = self.clients.lock().await.get(&target) {
                client.send_raw(message.clone()).await;
            }
        }
    }

    pub async fn broadcast_invite(&self, client: &Client, channel_name: &str, user: &str) {
        if let Some(channel) = self.channels.lock().await.get(channel_name) {
            let nick = client.nick.lock().await.to_string();
            let message = format!(
                ":{} NOTICE @{} :{} invited {} into the channel.",
                self.name, channel_name, nick, user
            );
            for target in channel.participants.read().await.keys() {
                if let Some(client) = self.clients.lock().await.get(target) {
                    client.send_raw(message.clone()).await;
                }
            }

            if let Some(invited) = self.clients.lock().await.get(user) {
                invited
                    .send_raw(format!(
                        ":{} INVITE {} :{}",
                        client.get_prefix().await,
                        nick,
                        channel_name
                    ))
                    .await;
            }
        }
    }

    pub async fn handle_channel_mode(
        &self,
        client: &Client,
        channel_name: &str,
        params: Vec<String>,
    ) {
        if let Some(channel) = self.channels.lock().await.get(channel_name) {
            let nick = client.nick.lock().await.to_string();
            let oper = *client.operator.lock().await;
            let has_participant = channel.has_participant(&nick).await;

            if params.len() < 1 {
                if !oper {
                    if channel.modes.lock().await.secret && !has_participant {
                        client
                            .send_numeric_reply(
                                NumericReply::ErrNoSuchChannel,
                                format!("{} :No such channel", channel_name).to_string(),
                            )
                            .await;
                        return;
                    }
                }

                client
                    .send_numeric_reply(
                        NumericReply::RplChannelModeIs,
                        format!(
                            "{} {}",
                            channel_name,
                            channel.get_modes_description(oper || has_participant).await
                        ),
                    )
                    .await;
            } else {
                if oper || has_participant {
                    if !oper && !channel.is_operator(&nick).await {
                        client
                            .send_numeric_reply(
                                NumericReply::ErrChanOpPrivsNeeded,
                                format!("{} :You're not channel operator", channel_name)
                                    .to_string(),
                            )
                            .await;
                        return;
                    }

                    let changes = channel.toggle_modes(client, params).await;
                    if changes.len() > 0 {
                        let mut targets = HashSet::new();

                        for target in channel.participants.read().await.keys() {
                            targets.insert(target.clone());
                        }

                        let message = format!(
                            ":{} MODE {} {}",
                            client.get_prefix().await,
                            channel_name,
                            changes
                        );
                        for target in targets {
                            if let Some(client) = self.clients.lock().await.get(&target) {
                                client.send_raw(message.clone()).await;
                            }
                        }
                    }
                } else {
                    client
                        .send_numeric_reply(
                            NumericReply::ErrUserNotInChannel,
                            format!("{} {} :They aren't on that channel", nick, channel_name)
                                .to_string(),
                        )
                        .await;
                }
            }
        } else {
            client
                .send_numeric_reply(
                    NumericReply::ErrNoSuchChannel,
                    format!("{} :No such channel", channel_name).to_string(),
                )
                .await;
        }
    }

    pub async fn handle_user_mode(&self, client: &Client, target_nick: &str, params: Vec<String>) {
        if self.is_nick_mapped(&target_nick).await {
            let nick = client.nick.lock().await.to_string();
            if &nick == target_nick {
                if params.len() > 0 {
                    /* TODO(diath): Set user mode. */
                } else {
                    /* TODO(diath): Send user mode. */
                    client
                        .send_numeric_reply(NumericReply::RplUModeIs, "".to_string())
                        .await;
                }
            } else {
                client
                    .send_numeric_reply(
                        NumericReply::ErrUsersDontMatch,
                        ":Cant change mode for other users".to_string(),
                    )
                    .await;
            }
        } else {
            client
                .send_numeric_reply(
                    NumericReply::ErrNoSuchNick,
                    format!("{} :No such nick/channel", target_nick).to_string(),
                )
                .await;
        }
    }
}
