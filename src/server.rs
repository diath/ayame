use crate::ayame::*;
use crate::channel::{Channel, ChannelUserModes};
use crate::client::Client;
use crate::config::Config;
use crate::replies::NumericReply;
use crate::service::Service;
use crate::services::hostserv::HostServ;
use crate::services::nickserv::NickServ;

use std::cmp;
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
use tokio::sync::{Mutex, RwLock};

use log;

pub struct NickHistory {
    pub nick: String,
    pub user: String,
    pub host: String,
    pub real_name: String,
    pub timestamp: i64,
}

pub struct Server {
    pub name: String,
    pub created: DateTime<Utc>,
    pub sent_packets: RwLock<u64>,
    pub recv_packets: RwLock<u64>,
    pub sent_bytes: RwLock<u64>,
    pub recv_bytes: RwLock<u64>,
    address: SocketAddr,
    clients: Mutex<HashMap<String, Arc<Client>>>,
    clients_pending: Mutex<Vec<Arc<Client>>>,
    operator_credentials: Mutex<HashMap<String, String>>,
    operators: Mutex<HashSet<String>>,
    channels: Mutex<HashMap<String, Channel>>,
    motd: Mutex<Option<Vec<String>>>,
    nick_history: Mutex<HashMap<String, Vec<NickHistory>>>,
    services: Mutex<HashMap<String, Box<dyn Service + Send + Sync>>>,
}

impl Server {
    pub fn new() -> Server {
        let config = Server::load_config();

        let name = config.server.name.unwrap_or(IRCD_NAME.to_string());
        let motd_path = config.server.motd_path.unwrap_or(IRCD_MOTD.to_string());
        let host = config.server.host.unwrap_or("127.0.0.1".to_string());
        let port = config.server.port.unwrap_or(6667);

        log::info!("Server: {}", name);
        log::info!("Address: {}:{}", host, port);

        let mut operators = HashMap::new();
        if let Some(opers) = config.oper {
            for oper in opers {
                if oper.name.is_none() || oper.password.is_none() {
                    continue;
                }

                operators.insert(oper.name.unwrap(), oper.password.unwrap());
            }
        }
        log::info!("Loaded {} operators.", operators.len());

        let mut services: HashMap<String, Box<dyn Service + Send + Sync>> = HashMap::new();
        services.insert("nickserv".to_string(), Box::new(NickServ::new()));
        services.insert("hostserv".to_string(), Box::new(HostServ::new()));

        Server {
            name: name,
            created: DateTime::<Utc>::from(SystemTime::now()),
            sent_packets: RwLock::new(0),
            recv_packets: RwLock::new(0),
            sent_bytes: RwLock::new(0),
            recv_bytes: RwLock::new(0),
            address: format!("{}:{}", host, port).parse().unwrap(),
            clients: Mutex::new(HashMap::new()),
            clients_pending: Mutex::new(vec![]),
            operator_credentials: Mutex::new(operators),
            operators: Mutex::new(HashSet::new()),
            channels: Mutex::new(HashMap::new()),
            motd: Mutex::new(Server::load_motd(&motd_path)),
            nick_history: Mutex::new(HashMap::new()),
            services: Mutex::new(services),
        }
    }

    fn load_config() -> Config {
        match read_to_string(IRCD_CONFIG) {
            Ok(s) => match toml::from_str(&s) {
                Ok(config) => config,
                Err(error) => {
                    log::warn!("Config parse error: {}", error);
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
        log::info!("Listening...");

        loop {
            let (stream, addr) = acceptor.accept().await?;
            let client = Arc::new(Client::new(server.clone(), addr));

            log::debug!("Client connected ({}).", addr);
            let c = Mutex::new(client.clone());
            tokio::spawn(async move {
                c.lock().await.task(stream).await;
            });

            let c2 = Mutex::new(client.clone());
            tokio::spawn(async move {
                c2.lock().await.task_ping().await;
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

        // NOTE(diath): We cannot use the let Some idiom here or we will end up with a deadlock.
        let client = self.clients.lock().await.remove(&old_nick);
        if client.is_some() {
            self.clients.lock().await.insert(nick, client.unwrap());
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

    pub async fn add_operator(&self, nick: String) {
        self.operators.lock().await.insert(nick);
    }

    pub async fn remove_operator(&self, nick: &str) {
        self.operators.lock().await.remove(nick);
    }

    pub async fn verify_operator(&self, name: &str, password: &str) -> bool {
        if let Some(entry) = self.operator_credentials.lock().await.get(name) {
            return entry == password;
        }

        false
    }

    pub async fn forward_message(
        &self,
        is_notice: bool,
        sender: &Client,
        name: &str,
        message: String,
    ) {
        if let Some(service) = self.services.lock().await.get(name) {
            service
                .on_message(sender, message.split(" ").collect::<Vec<&str>>())
                .await;
        } else if let Some(client) = self.clients.lock().await.get(name) {
            if is_notice {
                log::debug!(
                    "[NOTICE {} -> {}] {}",
                    sender.nick.lock().await.to_string(),
                    name,
                    message
                );
            } else {
                log::debug!(
                    "[PRIVMSG {} -> {}] {}",
                    sender.nick.lock().await.to_string(),
                    name,
                    message
                );
            }

            let message = if is_notice {
                format!(
                    ":{} NOTICE {} :{}",
                    sender.get_prefix().await,
                    name,
                    message
                )
            } else {
                format!(
                    ":{} PRIVMSG {} :{}",
                    sender.get_prefix().await,
                    name,
                    message
                )
            };

            client.update_idle_time().await;
            client.send_raw(message).await;

            if !is_notice {
                let away = client.away_message.lock().await.to_string();
                if away.len() > 0 {
                    sender
                        .send_numeric_reply(NumericReply::RplAway, format!("{}: {}", name, away))
                        .await;
                }
            }
        } else {
            sender
                .send_numeric_reply(
                    NumericReply::ErrNoSuchNick,
                    format!("{} :No such nick/channel", name).to_string(),
                )
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
        log::debug!("[{}] Channel created.", name);

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

    pub async fn join_channel(
        &self,
        client: &Client,
        channel_name: &str,
        password: String,
    ) -> bool {
        if let Some(channel) = self
            .channels
            .lock()
            .await
            .get(channel_name.to_string().to_lowercase().as_str())
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
                            format!("{} :Cannot join channel (+l)", channel_name).to_string(),
                        )
                        .await;
                    return false;
                }

                if modes.password.len() != 0 && modes.password != password {
                    client
                        .send_numeric_reply(
                            NumericReply::ErrBadChannelKey,
                            format!("{} :Cannot join channel (+k)", channel_name).to_string(),
                        )
                        .await;
                    return false;
                }

                let prefix = client.get_prefix().await;
                if modes.invite_only
                    && !channel.is_invited(&nick).await
                    && !channel.is_invite_exempt(&prefix).await
                {
                    client
                        .send_numeric_reply(
                            NumericReply::ErrInviteOnlyChan,
                            format!("{} :Cannot join channel (+i)", channel_name).to_string(),
                        )
                        .await;
                    return false;
                }

                if channel.is_banned(&prefix).await && !channel.is_ban_exempt(&prefix).await {
                    client
                        .send_numeric_reply(
                            NumericReply::ErrBannedFromChan,
                            format!("{} :Cannot join channel (+b)", channel_name).to_string(),
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

            let message = format!(":{} JOIN {}", client.get_prefix().await, channel_name);
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
                            format!("{} {} {}", channel_name, topic.set_by, topic.set_at)
                                .to_string(),
                        )
                        .await;
                }

                log::debug!("[{}] {} joined.", channel_name, nick);
                return true;
            }
        }

        false
    }

    pub async fn part_channel(
        &self,
        client: &Client,
        channel_name: &str,
        part_message: &str,
    ) -> bool {
        let mut result = false;
        let mut remove = false;

        if let Some(channel) = self
            .channels
            .lock()
            .await
            .get(channel_name.to_string().to_lowercase().as_str())
        {
            let nick = client.nick.lock().await.to_string();
            if channel.part(nick).await {
                let message = format!(
                    ":{} PART {} :{}",
                    client.get_prefix().await,
                    channel_name,
                    part_message
                );

                for target in channel.participants.read().await.keys() {
                    if let Some(client) = self.clients.lock().await.get(target) {
                        client.send_raw(message.clone()).await;
                    }
                }

                /* NOTE(diath): We need to send the confirmation to the sending client separately as they are no longer in the channel participant list. */
                client.send_raw(message.clone()).await;

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

    pub async fn invite_channel(&self, client: &Client, channel_name: &str, invited_nick: &str) {
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
                return;
            }

            channel
                .invites
                .lock()
                .await
                .insert(invited_nick.to_string());

            self.broadcast_invite(client, channel, &invited_nick).await;

            client
                .send_numeric_reply(
                    NumericReply::RplInviting,
                    format!("{} {}", invited_nick, channel_name).to_string(),
                )
                .await;

            if let Some(invited) = self.clients.lock().await.get(invited_nick) {
                let away = invited.away_message.lock().await.to_string();
                if away.len() > 0 {
                    client
                        .send_numeric_reply(
                            NumericReply::RplAway,
                            format!("{}: {}", invited_nick, away),
                        )
                        .await;
                }
            }
        }
    }

    pub async fn kick_channel(
        &self,
        client: &Client,
        channel_name: &str,
        kicked: &str,
        kick_message: String,
    ) -> bool {
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
            if !oper && !channel.is_half_operator(&nick).await {
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
                    client.get_prefix().await,
                    channel_name,
                    kicked,
                    kick_message
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

    pub async fn forward_channel_message(
        &self,
        is_notice: bool,
        client: &Client,
        name: &str,
        message: String,
    ) {
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

            log::debug!("[{}] {}: {}", name, prefix, message);

            let message = if is_notice {
                format!(":{} NOTICE {} :{}", prefix, name, message)
            } else {
                format!(":{} PRIVMSG {} :{}", prefix, name, message)
            };

            for target in channel.participants.read().await.keys() {
                if let Some(client) = self.clients.lock().await.get(target) {
                    if client.get_prefix().await != prefix {
                        client.send_raw(message.clone()).await;
                    }
                }
            }

            client.update_idle_time().await;
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

            log::debug!("[{}] {} changed topic to {}", channel_name, nick, topic);
            // NOTE(diath): The topic sender should be just the name, not the prefix.
            channel.set_topic(nick.to_string(), topic.clone()).await;

            let message = format!(
                ":{} TOPIC {} :{}",
                client.get_prefix().await,
                channel_name,
                topic
            );
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
        let nick = client.nick.lock().await.to_string();
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

    pub async fn send_who_entry(
        &self,
        channel: Option<&Channel>,
        channel_name: String,
        client: &Client,
        participant: &Client,
    ) {
        let user = participant.user.lock().await.to_string();
        let host = participant.get_host().await;
        let nick = participant.nick.lock().await.to_string();
        let real_name = participant.real_name.lock().await.to_string();

        /* NOTE(diath): Who flags:
            - H and G indicate away status (H for here, G for gone).
            - * indicates server operator.
            - @ and + indicate channel operator and voice respectively.
        */
        let mut flags = "".to_string();
        let away_message = participant.away_message.lock().await.to_string();
        if away_message.len() == 0 {
            flags.push('H');
        } else {
            flags.push('G');
        }

        if *participant.operator.lock().await {
            flags.push('*');
        }

        if let Some(channel) = channel {
            if channel.is_operator(&nick).await {
                flags.push('@');
            } else if channel.is_voiced(&nick).await {
                flags.push('+');
            }
        }

        if flags.len() > 0 {
            flags.push(' ');
        }

        client
            .send_numeric_reply(
                NumericReply::RplWhoReply,
                format!(
                    "{} {} {} {} {} {}:0 {}",
                    channel_name, user, host, self.name, nick, flags, real_name
                ),
            )
            .await;
    }

    pub async fn send_who(&self, client: &Client, channel_name: String, operators_only: bool) {
        if let Some(channel) = self.channels.lock().await.get(&channel_name) {
            let nick = client.nick.lock().await.to_string();
            let oper = *client.operator.lock().await;
            if oper || channel.has_participant(&nick).await {
                for target in channel.participants.read().await.keys() {
                    if let Some(participant) = self.clients.lock().await.get(target) {
                        let is_operator = channel.is_operator(&target).await;
                        if operators_only && !is_operator {
                            continue;
                        }

                        self.send_who_entry(
                            Some(&channel),
                            channel_name.to_string(),
                            &client,
                            &participant,
                        )
                        .await;
                    }
                }
            }
        }

        client
            .send_numeric_reply(
                NumericReply::RplEndOfWho,
                format!("{} :End of /WHO list.", channel_name),
            )
            .await;
    }

    pub async fn send_whois(&self, client: &Client, target_nick: &str) {
        if let Some(target) = self.clients.lock().await.get(target_nick) {
            let nick = target.nick.lock().await.to_string();
            let user = target.user.lock().await.to_string();
            let host = target.get_host().await;
            let real_name = target.real_name.lock().await.to_string();

            client
                .send_numeric_reply(
                    NumericReply::RplWhoisUser,
                    format!("{} {} {} * :{}", nick, user, host, real_name),
                )
                .await;

            if *target.identified.lock().await {
                client
                    .send_numeric_reply(
                        NumericReply::RplUserIsRegNick,
                        format!("{} :is identified for this nick", nick),
                    )
                    .await;
            }

            client
                .send_numeric_reply(
                    NumericReply::RplWhoisServer,
                    format!("{} {} :{}", nick, self.name, IRCD_NAME),
                )
                .await;

            if *target.operator.lock().await {
                client
                    .send_numeric_reply(
                        NumericReply::RplWhoisOperator,
                        format!("{} :is an IRC operator", nick),
                    )
                    .await;
            }

            let away_message = target.away_message.lock().await.to_string();
            if away_message.len() > 0 {
                client
                    .send_numeric_reply(
                        NumericReply::RplAway,
                        format!("{} :{}", nick, away_message),
                    )
                    .await;
            }

            if *client.operator.lock().await {
                let mut channels = vec![];
                for channel_name in &*target.channels.lock().await {
                    if let Some(channel) = self.channels.lock().await.get(channel_name) {
                        channels.push(format!(
                            "{}{}",
                            channel.get_participant_prefix(&nick).await,
                            channel_name
                        ));
                    }
                }

                if channels.len() > 0 {
                    client
                        .send_numeric_reply(
                            NumericReply::RplWhoisChannels,
                            format!("{}", channels.join(" ")),
                        )
                        .await;
                }
            }

            client
                .send_numeric_reply(
                    NumericReply::RplWhoisIdle,
                    format!("{} {} :seconds idle", nick, target.get_idle_time().await),
                )
                .await;

            client
                .send_numeric_reply(
                    NumericReply::RplEndOfWhois,
                    format!("{} :End of WHOIS list", nick),
                )
                .await;
        } else {
            client
                .send_numeric_reply(
                    NumericReply::ErrNoSuchNick,
                    format!("{} :No such nick/channel", target_nick).to_string(),
                )
                .await;
        }
    }

    pub async fn send_whowas(&self, client: &Client, target_nick: &str, limit: u32) {
        if let Some(history) = self.nick_history.lock().await.get(target_nick) {
            let mut start = 0;
            if limit != 0 {
                let offset = cmp::min(history.len(), limit as usize);
                start = history.len() - offset;
            }

            for entry in history[start..].iter() {
                client
                    .send_numeric_reply(
                        NumericReply::RplWhoWasUser,
                        format!(
                            "{} {} {} * :{}",
                            entry.nick, entry.user, entry.host, entry.real_name
                        ),
                    )
                    .await;

                client
                    .send_numeric_reply(
                        NumericReply::RplWhoisServer,
                        format!("{} {} :{}", target_nick, self.name, entry.timestamp),
                    )
                    .await;
            }

            client
                .send_numeric_reply(
                    NumericReply::RplEndOfWhowas,
                    format!("{} :End of WHOWAS", target_nick),
                )
                .await;
        } else {
            client
                .send_numeric_reply(
                    NumericReply::ErrWasNoSuchNick,
                    format!("{} :There was no such nickname", target_nick),
                )
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

    pub async fn broadcast_invite(&self, client: &Client, channel: &Channel, user: &str) {
        let nick = client.nick.lock().await.to_string();
        let message = format!(
            ":{} NOTICE @{} :{} invited {} into the channel.",
            self.name, channel.name, nick, user
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
                    channel.name
                ))
                .await;
        }
    }

    pub async fn broadcast_oper_notice(&self, message: String) {
        for nick in &*self.operators.lock().await {
            if let Some(client) = self.clients.lock().await.get(nick) {
                client
                    .send_raw(format!(
                        ":{} NOTICE {} :{}",
                        self.name,
                        self.name,
                        message.to_string(),
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
                    let changes = channel.toggle_modes(client, params).await;
                    if changes.len() > 0 {
                        let mut targets = HashSet::new();

                        for target in channel.participants.read().await.keys() {
                            targets.insert(target.clone());
                        }

                        log::debug!("[{}] Mode {}.", channel_name, changes);

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
                    let changes = client.toggle_modes(params).await;
                    if changes.len() > 0 {
                        client
                            .send_raw(format!(
                                ":{} MODE {} :{}",
                                self.name,
                                client.nick.lock().await.to_string(),
                                changes
                            ))
                            .await;
                    }
                } else {
                    client
                        .send_numeric_reply(
                            NumericReply::RplUModeIs,
                            client.get_modes_description().await,
                        )
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

    pub async fn handle_userhost(&self, client: &Client, params: Vec<String>) {
        let mut result = vec![];
        for param in params {
            if let Some(other) = self.clients.lock().await.get(&param) {
                let mut parts = vec![];

                let nick = other.nick.lock().await.to_string();
                parts.push(nick.clone());

                if *other.operator.lock().await {
                    parts.push("*".to_string());
                }

                parts.push("=".to_string());

                if other.away_message.lock().await.to_string().len() > 0 {
                    parts.push("+".to_string());
                } else {
                    parts.push("-".to_string());
                }

                parts.push(format!("{}@{}", &nick, other.get_host().await));
                result.push(parts.join(""));
            }
        }

        client
            .send_numeric_reply(NumericReply::RplUserHost, format!(":{}", result.join(" ")))
            .await;
    }

    pub async fn uptime(&self) -> i64 {
        return (DateTime::<Utc>::from(SystemTime::now()) - self.created).num_seconds();
    }

    pub async fn append_nick_history(&self, nick: String, client: &Client) {
        let entry = NickHistory {
            nick: nick.to_string(),
            user: client.user.lock().await.to_string(),
            host: client.get_host().await,
            real_name: client.real_name.lock().await.to_string(),
            timestamp: Utc::now().timestamp(),
        };

        if !self.nick_history.lock().await.contains_key(&nick) {
            self.nick_history
                .lock()
                .await
                .insert(nick.to_string(), vec![]);
        }

        if let Some(entries) = self.nick_history.lock().await.get_mut(&nick) {
            entries.push(entry);
        }
    }
}
