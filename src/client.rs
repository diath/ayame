use crate::ayame::*;
use crate::replies::NumericReply;
use crate::server::Server;

use std::collections::HashSet;
use std::io::ErrorKind;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::SystemTime;

use chrono::prelude::DateTime;
use chrono::Utc;

use log;

use tokio::io::{split, AsyncBufReadExt, AsyncWriteExt, BufReader, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

use ircmsgprs::parser::{Message, Parser};

pub struct Client {
    pub nick: Mutex<String>,
    pub user: Mutex<String>,
    pub host: Mutex<String>,
    pub real_name: Mutex<String>,
    pub password: Mutex<String>,
    pub registered: Mutex<bool>,
    pub operator: Mutex<bool>,
    pub channels: Mutex<HashSet<String>>,
    pub away_message: Mutex<String>,
    server: Arc<Server>,
    address: SocketAddr,
    writer: Mutex<Option<WriteHalf<TcpStream>>>,
    parser: Mutex<Parser>,
}

impl Client {
    pub fn new(server: Arc<Server>, address: SocketAddr) -> Client {
        Client {
            nick: Mutex::new(String::new()),
            user: Mutex::new(String::new()),
            host: Mutex::new(address.ip().to_string()),
            real_name: Mutex::new(String::new()),
            password: Mutex::new(String::new()),
            registered: Mutex::new(false),
            operator: Mutex::new(false),
            channels: Mutex::new(HashSet::new()),
            away_message: Mutex::new(String::new()),
            server: server,
            address: address,
            writer: Mutex::new(None),
            parser: Mutex::new(Parser::new()),
        }
    }

    pub async fn get_prefix(&self) -> String {
        return format!(
            "{}!{}@{}",
            self.nick.lock().await.to_string(),
            self.user.lock().await.to_string(),
            self.host.lock().await.to_string()
        );
    }

    pub async fn task(&self, stream: TcpStream) {
        let (reader, writer) = split(stream);
        let mut line = String::new();
        let mut buf_reader = BufReader::new(reader);

        (*self.writer.lock().await) = Some(writer);

        loop {
            match buf_reader.read_line(&mut line).await {
                Ok(size) => {
                    if size == 0 {
                        self.server.broadcast_quit(&self, "EOF").await;
                        break;
                    } else {
                        let result = self.parser.lock().await.parse(line.clone());
                        if result.is_none() {
                            log::debug!("Client parse error.");
                            break;
                        }
                        self.on_message(result.unwrap()).await;
                    }
                }
                Err(err) => {
                    if err.kind() != ErrorKind::InvalidData {
                        self.server.broadcast_quit(&self, "Read Error").await;
                        log::debug!("Client read error ({}).", err);
                        break;
                    }
                }
            }

            line.clear();
        }

        self.server.remove_from_channels(&self).await;

        let nick = self.nick.lock().await.to_string();
        if nick.len() != 0 {
            self.server.unmap_nick(nick).await;
        }

        self.server.unmap_client(&self).await;

        log::debug!("Client disconnected ({}).", self.address);
    }

    pub async fn send_raw(&self, message: String) {
        if let Some(writer) = &mut *self.writer.lock().await {
            match writer
                .write_all(format!("{}\r\n", message).as_bytes())
                .await
            {
                Ok(_) => {}
                Err(_) => {
                    log::debug!("Failed to write message ({})", message);
                }
            }
        }
    }

    pub async fn send_numeric_reply(&self, reply: NumericReply, message: String) {
        let nick = self.nick.lock().await.to_string();
        self.send_raw(format!(
            ":{} {:03} {} {}",
            self.server.name, reply as i32, nick, message
        ))
        .await;
    }

    pub async fn complete_registration(&self) {
        (*self.registered.lock().await) = true;

        let prefix = self.get_prefix().await;
        self.send_numeric_reply(
            NumericReply::RplWelcome,
            format!(":Welcome to {} {}", self.server.name, prefix),
        )
        .await;

        self.send_numeric_reply(
            NumericReply::RplYourHost,
            format!(
                ":Your host is {} running version {}-{}",
                self.server.name, IRCD_NAME, IRCD_VERSION
            ),
        )
        .await;

        /* TODO(diath): We should probably store first startup time somewhere. */
        self.send_numeric_reply(
            NumericReply::RplCreated,
            format!(
                ":This server was created {}",
                self.server.created.format("%Y-%m-%d %H:%M:%S.%f"),
            ),
        )
        .await;

        self.server.send_motd(&self).await;
        /* TODO(diath): Send RPL_MYINFO with <servername> <version> <user modes> <server modes>. */
    }

    async fn on_message(&self, message: Message) {
        log::debug!("Received message: {}", message);

        let registered = *self.registered.lock().await;
        if !registered {
            match message.command.as_str() {
                /* Connection Registration */
                "CAP" => {
                    self.on_cap(message);
                }
                "PASS" => {
                    self.on_pass(message).await;
                }
                "NICK" => {
                    self.on_nick(message).await;
                }
                "USER" => {
                    self.on_user(message).await;
                }
                _ => {
                    self.send_numeric_reply(
                        NumericReply::ErrNotRegistered,
                        ":You have not registered".to_string(),
                    )
                    .await;
                }
            }
        } else {
            match message.command.as_str() {
                /* Connection Registration */
                "CAP" => {
                    self.on_cap(message);
                }
                "PASS" => {
                    self.on_pass(message).await;
                }
                "NICK" => {
                    self.on_nick(message).await;
                }
                "USER" => {
                    self.on_user(message).await;
                }
                "OPER" => {
                    self.on_oper(message).await;
                }
                "QUIT" => {
                    self.on_quit(message).await;
                }
                /* Channel Operations */
                "JOIN" => {
                    self.on_join(message).await;
                }
                "PART" => {
                    self.on_part(message).await;
                }
                "TOPIC" => {
                    self.on_topic(message).await;
                }
                "NAMES" => {
                    self.on_names(message).await;
                }
                "LIST" => {
                    self.on_list(message).await;
                }
                "INVITE" => {
                    self.on_invite(message).await;
                }
                "KICK" => {
                    self.on_kick(message).await;
                }
                /* Sending Messages */
                "PRIVMSG" => {
                    self.on_privmsg(message, false).await;
                }
                "NOTICE" => {
                    self.on_privmsg(message, true).await;
                }
                /* Server Queries and Commands */
                "MOTD" => {
                    self.on_motd(message).await;
                }
                "VERSION" => {
                    self.on_version(message).await;
                }
                "STATS" => {
                    self.on_stats(message).await;
                }
                "TIME" => {
                    self.on_time(message).await;
                }
                "REHASH" => {
                    self.on_rehash().await;
                }
                "DIE" | "RESTART" => {
                    self.send_numeric_reply(
                        NumericReply::ErrNoPrivileges,
                        ":Permission Denied- You're not an IRC operator".to_string(),
                    )
                    .await;
                }
                "SUMMON" => {
                    self.send_numeric_reply(
                        NumericReply::ErrSummonDisabled,
                        ":SUMMON has been disabled".to_string(),
                    )
                    .await;
                }
                "USERS" => {
                    self.send_numeric_reply(
                        NumericReply::ErrUsersDisabled,
                        ":USERS has been disabled".to_string(),
                    )
                    .await;
                }
                /* Other */
                "MODE" => {
                    self.on_mode(message).await;
                }
                "PING" => {
                    self.on_ping(message).await;
                }
                "AWAY" => {
                    self.on_away(message).await;
                }
                "USERHOST" => {
                    self.on_userhost(message).await;
                }
                "ISON" => {
                    self.on_ison(message).await;
                }
                _ => {
                    self.send_numeric_reply(
                        NumericReply::ErrUnknownCommand,
                        format!("{} :Unknown command", message.command),
                    )
                    .await;
                    log::debug!("Command {} not implemented.", message.command);
                }
            }
        }
    }

    fn on_cap(&self, _message: Message) {
        log::debug!("Ignoring CAP command (IRCv3)");
    }

    async fn on_pass(&self, message: Message) {
        if message.params.len() < 1 {
            self.send_numeric_reply(
                NumericReply::ErrNeedMoreParams,
                "PASS :Not enough parameters".to_string(),
            )
            .await;
        } else if (*self.nick.lock().await).len() > 0 || *self.registered.lock().await {
            self.send_numeric_reply(
                NumericReply::ErrAlreadyRegistered,
                ":Unauthorized command (already registered)".to_string(),
            )
            .await;
        } else {
            (*self.password.lock().await) = message.params[0].clone();
        }
    }

    async fn on_nick(&self, message: Message) {
        /* TODO(diath): ERR_NICKCOLLISION, ERR_UNAVAILRESOURCE, ERR_RESTRICTED */
        if let Some(nick) = message.params.get(0) {
            if self.server.is_nick_mapped(nick).await {
                self.send_numeric_reply(
                    NumericReply::ErrNicknameInUse,
                    format!("{} :Nickname is already in use", nick),
                )
                .await;
            } else {
                if !Client::is_nick_valid(nick.to_string()) {
                    self.send_numeric_reply(
                        NumericReply::ErrErroneousNickname,
                        format!("{} :Erroneous nickname", nick),
                    )
                    .await;
                } else {
                    let mut send_complete_registration = false;
                    if self.nick.lock().await.len() == 0 {
                        self.server.map_nick(nick.to_string(), &self).await;

                        if !*self.registered.lock().await && self.user.lock().await.len() != 0 {
                            send_complete_registration = true;
                        }
                    } else {
                        self.server
                            .remap_nick(self.nick.lock().await.to_string(), nick.to_string())
                            .await;
                    }
                    (*self.nick.lock().await) = nick.to_string();

                    if send_complete_registration {
                        self.complete_registration().await;
                    }
                }
            }
        } else {
            self.send_numeric_reply(
                NumericReply::ErrNoNicknameGiven,
                ":No nickname given".to_string(),
            )
            .await;
        }
    }

    async fn on_user(&self, message: Message) {
        if *self.registered.lock().await {
            self.send_numeric_reply(
                NumericReply::ErrAlreadyRegistered,
                ":Unauthorized command (already registered)".to_string(),
            )
            .await;
        } else if message.params.len() < 4 {
            self.send_numeric_reply(
                NumericReply::ErrNeedMoreParams,
                "USER :Not enough parameters".to_string(),
            )
            .await;
        } else {
            (*self.user.lock().await) = message.params[0].clone();
            (*self.real_name.lock().await) = message.params[3].clone();

            if self.nick.lock().await.len() != 0 {
                self.complete_registration().await;
            }
        }
    }

    async fn on_oper(&self, message: Message) {
        /* TODO(diath): ERR_NOOPERHOST */
        if *self.operator.lock().await {
            return;
        }

        if message.params.len() < 2 {
            self.send_numeric_reply(
                NumericReply::ErrNeedMoreParams,
                "OPER :Not enough parameters".to_string(),
            )
            .await;
        } else {
            let name = message.params[0].clone();
            let password = message.params[1].clone();
            if self.server.is_operator(&name, &password).await {
                (*self.operator.lock().await) = true;
                self.send_numeric_reply(
                    NumericReply::RplYoureOper,
                    ":You are now an IRC operator".to_string(),
                )
                .await;
            } else {
                self.send_numeric_reply(
                    NumericReply::ErrPasswordMismatch,
                    ":Password incorrect".to_string(),
                )
                .await;
            }
        }
    }

    async fn on_join(&self, message: Message) {
        /* TODO(diath): ERR_TOOMANYTARGETS, ERR_BADCHANMASK, ERR_TOOMANYCHANNELS, ERR_UNAVAILRESOURCE */
        if message.params[0] == "0" {
            for channel in &*self.channels.lock().await {
                self.server.part_channel(self, &channel, "Leaving").await;
            }
            self.channels.lock().await.clear();
        } else {
            let targets = message.params[0].split(",");
            let mut passwords = vec![];
            if message.params.len() > 1 {
                passwords = message.params[1].split(",").collect();
            }

            for (index, target) in targets.enumerate() {
                if target.len() == 0 {
                    continue;
                }

                if !self.server.is_channel_mapped(target).await {
                    self.server.create_channel(target).await;
                }

                let password = if let Some(password) = passwords.get(index) {
                    password.to_string()
                } else {
                    "".to_string()
                };
                if self.server.join_channel(self, target, password).await {
                    self.channels.lock().await.insert(target.to_string());

                    /* NOTE(diath): This cannot be handled in Server::join_channel method or we will end up with a deadlock. */
                    self.server.send_names(self, target.to_string()).await;
                }
            }
        }
    }

    async fn on_part(&self, message: Message) {
        if message.params.len() < 1 {
            self.send_numeric_reply(
                NumericReply::ErrNeedMoreParams,
                "PART :Not enough parameters".to_string(),
            )
            .await;
        } else {
            let nick = self.nick.lock().await.to_string();
            let targets = message.params[0].split(",");
            let part_message = if message.params.len() > 1 {
                message.params[1].clone()
            } else {
                "Leaving".to_string()
            };

            for target in targets {
                if target.len() == 0 {
                    continue;
                }

                if !self.server.is_channel_mapped(target).await {
                    self.send_numeric_reply(
                        NumericReply::ErrNoSuchChannel,
                        format!("{} :No such channel", target).to_string(),
                    )
                    .await;
                    continue;
                }

                if !self.server.has_channel_participant(target, &nick).await {
                    self.send_numeric_reply(
                        NumericReply::ErrNotOnChannel,
                        format!("{} :You're not on that channel", target).to_string(),
                    )
                    .await;
                    continue;
                }

                if self.server.part_channel(self, target, &part_message).await {
                    self.channels.lock().await.remove(target);
                }
            }
        }
    }

    async fn on_topic(&self, message: Message) {
        /* TODO(diath): ERR_NOCHANMODES */
        if message.params.len() < 1 {
            self.send_numeric_reply(
                NumericReply::ErrNeedMoreParams,
                "TOPIC :Not enough parameters".to_string(),
            )
            .await;
        } else if message.params.len() < 2 {
            self.server
                .get_channel_topic(self, &message.params[0])
                .await;
        } else {
            let channel = message.params[0].clone();
            let nick = self.nick.lock().await.to_string();
            if self.server.has_channel_participant(&channel, &nick).await {
                self.server
                    .set_channel_topic(self, &channel, message.params[1].clone())
                    .await;
            } else {
                self.send_numeric_reply(
                    NumericReply::ErrNotOnChannel,
                    format!("{} :You're not on that channel", message.params[0].clone())
                        .to_string(),
                )
                .await;
            }
        }
    }

    async fn on_names(&self, message: Message) {
        /* TODO(diath): ERR_TOOMANYMATCHES */
        if message.params.len() > 1 {
            if message.params[1] != self.server.name {
                self.send_numeric_reply(
                    NumericReply::ErrNoSuchServer,
                    format!("{} :No such server", message.params[1]),
                )
                .await;
                return;
            }
        }

        if message.params.len() > 0 {
            if let Some(_) = message.params[0].find(",") {
                self.send_numeric_reply(
                    NumericReply::ErrTooManyTargets,
                    format!(
                        "{} :Too many targets. The maximum is 1 for NAMES.",
                        message.params[0]
                    ),
                )
                .await;
            } else {
                self.server
                    .send_names(self, message.params[0].clone())
                    .await;
            }
        } else {
            self.send_numeric_reply(
                NumericReply::RplEndOfNames,
                format!("{} :End of /NAMES list.", message.params[0]),
            )
            .await;
        }
    }

    async fn on_list(&self, message: Message) {
        if message.params.len() > 1 {
            if message.params[1] != self.server.name {
                self.send_numeric_reply(
                    NumericReply::ErrNoSuchServer,
                    format!("{} :No such server", message.params[1]),
                )
                .await;
                return;
            }
        }

        if message.params.len() > 0 {
            self.server
                .send_list(self, Some(message.params[0].clone()))
                .await;
        } else {
            self.server.send_list(self, None).await;
        }
    }

    async fn on_invite(&self, message: Message) {
        /* TODO(diath): RPL_AWAY. */
        if message.params.len() < 2 {
            self.send_numeric_reply(
                NumericReply::ErrNeedMoreParams,
                "INVITE :Not enough parameters".to_string(),
            )
            .await;
            return;
        }

        let nick = self.nick.lock().await.to_string();
        let user = message.params[0].clone();
        let target = message.params[1].clone();

        if !self.server.is_channel_mapped(&target).await {
            self.send_numeric_reply(
                NumericReply::ErrNoSuchChannel,
                format!("{} :No such channel", target).to_string(),
            )
            .await;
            return;
        }

        if !self.server.has_channel_participant(&target, &nick).await {
            self.send_numeric_reply(
                NumericReply::ErrNotOnChannel,
                format!("{} :You're not on that channel", target).to_string(),
            )
            .await;
            return;
        }

        if !self.server.is_nick_mapped(&user).await {
            self.send_numeric_reply(
                NumericReply::ErrNoSuchNick,
                format!("{} :No such nick/channel", user).to_string(),
            )
            .await;
            return;
        }

        if self.server.has_channel_participant(&target, &user).await {
            self.send_numeric_reply(
                NumericReply::ErrUserOnChannel,
                format!("{} {} :is already on channel", user, target),
            )
            .await;
            return;
        }

        self.server.invite_channel(self, &target, &user).await;
    }

    async fn on_kick(&self, message: Message) {
        /* TODO(diath): ERR_BADCHANMASK */
        if message.params.len() < 2 {
            self.send_numeric_reply(
                NumericReply::ErrNeedMoreParams,
                "KICK :Not enough parameters".to_string(),
            )
            .await;
            return;
        }

        let nick = self.nick.lock().await.to_string();
        let targets = message.params[0].split(",").collect::<Vec<&str>>();
        let users = message.params[1].split(",").collect::<Vec<&str>>();
        let message = if message.params.len() > 2 {
            message.params[2].clone()
        } else {
            "Kicked".to_string()
        };

        /* NOTE(diath): For the message to be syntactically correct, there MUST be either one channel parameter and multiple user
        parameter, or as many channel parameters as there are user parameters. */
        if targets.len() == 1 && users.len() > 0 {
            let target = targets[0];
            if target.len() == 0 {
                return;
            }

            if !self.server.is_channel_mapped(&target).await {
                self.send_numeric_reply(
                    NumericReply::ErrNoSuchChannel,
                    format!("{} :No such channel", target).to_string(),
                )
                .await;
                return;
            }

            if !self.server.has_channel_participant(&target, &nick).await {
                self.send_numeric_reply(
                    NumericReply::ErrNotOnChannel,
                    format!("{} :You're not on that channel", target).to_string(),
                )
                .await;
                return;
            }

            for user in users {
                if !self.server.has_channel_participant(&target, user).await {
                    self.send_numeric_reply(
                        NumericReply::ErrUserNotInChannel,
                        format!("{} {} :They aren't on that channel", user, target).to_string(),
                    )
                    .await;
                    continue;
                }

                if self
                    .server
                    .kick_channel(self, target, user, message.clone())
                    .await
                {
                    self.channels.lock().await.remove(&nick);
                }
            }
        } else if targets.len() == users.len() {
            for (index, target) in targets.iter().enumerate() {
                if !self.server.is_channel_mapped(&target).await {
                    self.send_numeric_reply(
                        NumericReply::ErrNoSuchChannel,
                        format!("{} :No such channel", target).to_string(),
                    )
                    .await;
                    continue;
                }

                if !self.server.has_channel_participant(&target, &nick).await {
                    self.send_numeric_reply(
                        NumericReply::ErrNotOnChannel,
                        format!("{} :You're not on that channel", target).to_string(),
                    )
                    .await;
                    continue;
                }

                let user = users.get(index).unwrap();
                if !self.server.has_channel_participant(&target, user).await {
                    self.send_numeric_reply(
                        NumericReply::ErrUserNotInChannel,
                        format!("{} {} :They aren't on that channel", user, target).to_string(),
                    )
                    .await;
                    continue;
                }

                if self
                    .server
                    .kick_channel(self, target, user, message.clone())
                    .await
                {
                    self.channels.lock().await.remove(&nick);
                }
            }
        }
    }

    async fn on_privmsg(&self, message: Message, is_notice: bool) {
        /* TODO(diath): ERR_NOTOPLEVEL, ERR_WILDTOPLEVEL, ERR_BADMASK */
        if message.params.len() < 1 {
            self.send_numeric_reply(
                NumericReply::ErrNoRecipient,
                ":No recipient given (PRIVMSG)".to_string(),
            )
            .await;
            return;
        }

        if message.params.len() < 2 {
            self.send_numeric_reply(
                NumericReply::ErrNoTextToSend,
                ":No text to send".to_string(),
            )
            .await;
            return;
        }

        let targets = message.params[0].split(",");
        let text = message.params[1].clone();

        for target in targets {
            if target.len() == 0 {
                continue;
            }

            match &target[0..1] {
                "#" => {
                    self.server
                        .forward_channel_message(is_notice, self, target, text.clone())
                        .await;
                }
                /* NOTE(diath): Technically a channel can be prefixed with either # (network), ! (safe), + (unmoderated) or & (local) but we only support #. */
                "!" | "&" | "+" => {
                    self.send_numeric_reply(
                        NumericReply::ErrNoSuchChannel,
                        format!("{} :No such channel", target).to_string(),
                    )
                    .await;
                }
                _ => {
                    if self.server.is_nick_mapped(target).await {
                        self.server
                            .forward_message(is_notice, self, target, text.clone())
                            .await;
                    } else {
                        self.send_numeric_reply(
                            NumericReply::ErrNoSuchNick,
                            format!("{} :No such nick/channel", target).to_string(),
                        )
                        .await;
                    }
                }
            }
        }
    }

    async fn on_motd(&self, _message: Message) {
        /* TODO: add support for <target> */
        self.server.send_motd(&self).await;
    }

    pub async fn on_version(&self, message: Message) {
        if message.params.len() > 0 {
            if message.params[0] != self.server.name {
                self.send_numeric_reply(
                    NumericReply::ErrNoSuchServer,
                    format!("{} :No such server", message.params[0]),
                )
                .await;
                return;
            }
        }

        self.send_numeric_reply(
            NumericReply::RplVersion,
            format!(
                "{}-{}.0 {} :{}",
                IRCD_NAME, IRCD_VERSION, self.server.name, IRCD_REPOSITORY
            ),
        )
        .await;
    }

    async fn on_stats(&self, message: Message) {
        if message.params.len() < 1 {
            self.send_numeric_reply(
                NumericReply::ErrNeedMoreParams,
                "STATS :Not enough parameters".to_string(),
            )
            .await;

            return;
        }

        let query = message.params[0].as_str();
        match query {
            "u" => {
                let uptime = self.server.uptime().await;

                self.send_numeric_reply(
                    NumericReply::RplStatsUptime,
                    format!(
                        ":Server Up {} days, {:02}:{:02}:{:02}",
                        ((uptime / 86400) as f64).floor() as i64,
                        ((uptime / 3600) as f64).floor() as i64 % 24,
                        ((uptime / 60) as f64).floor() as i64 % 60,
                        uptime % 60,
                    ),
                )
                .await;
            }
            _ => {}
        }

        self.send_numeric_reply(
            NumericReply::RplEndOfStats,
            format!("{} :End of /STATS report", query),
        )
        .await;
    }

    async fn on_time(&self, _message: Message) {
        /* TODO: add support for <target> */
        self.send_numeric_reply(
            NumericReply::RplTime,
            format!(
                "{} :{}",
                self.server.name,
                DateTime::<Utc>::from(SystemTime::now()).format("%Y-%m-%d %H:%M:%S")
            )
            .to_string(),
        )
        .await;
    }

    async fn on_quit(&self, message: Message) {
        let reason = if message.params.len() > 0 {
            message.params[0].to_string()
        } else {
            "Quitting".to_string()
        };

        self.server.broadcast_quit(&self, &reason).await;

        /* TODO(diath): We should probably also shutdown the reader somehow. */
        if let Some(mut writer) = self.writer.lock().await.take() {
            writer.flush();
            writer.shutdown();
        }
    }

    async fn on_rehash(&self) {
        if !*self.operator.lock().await {
            self.send_numeric_reply(
                NumericReply::ErrNoPrivileges,
                ":Permission Denied- You're not an IRC operator".to_string(),
            )
            .await;

            return;
        }

        self.send_numeric_reply(
            NumericReply::RplRehashing,
            "motd.txt :Rehashing".to_string(),
        )
        .await;
        self.server.reload_motd().await;
    }

    async fn on_mode(&self, message: Message) {
        if message.params.len() < 1 {
            self.send_numeric_reply(
                NumericReply::ErrNeedMoreParams,
                "MODE :Not enough parameters".to_string(),
            )
            .await;
            return;
        }

        let target = message.params[0].clone();
        match &target[0..1] {
            "#" => {
                self.server
                    .handle_channel_mode(self, &target, message.params[1..].to_vec())
                    .await;
            }
            /* NOTE(diath): Technically a channel can be prefixed with either # (network), ! (safe), + (unmoderated) or & (local) but we only support #. */
            "!" | "&" | "+" => {
                self.send_numeric_reply(
                    NumericReply::ErrNoSuchChannel,
                    format!("{} :No such channel", target).to_string(),
                )
                .await;
            }
            _ => {
                self.server
                    .handle_user_mode(self, &target, message.params[1..].to_vec())
                    .await;
            }
        }
    }

    async fn on_ping(&self, message: Message) {
        if message.params.len() < 1 {
            self.send_numeric_reply(
                NumericReply::ErrNoOrigin,
                ":No origin specified".to_string(),
            )
            .await;
            return;
        }

        if message.params.len() > 1 {
            if self.server.name != message.params[1] {
                self.send_numeric_reply(
                    NumericReply::ErrNoSuchServer,
                    format!("{} :No such server", message.params[1]),
                )
                .await;
                return;
            }
        }

        self.send_raw(format!(
            ":{} PONG {} :{}",
            self.server.name, self.server.name, message.params[0]
        ))
        .await;
    }

    async fn on_away(&self, message: Message) {
        if message.params.len() > 0 {
            (*self.away_message.lock().await) = message.params[0].to_string();
            self.send_numeric_reply(
                NumericReply::RplNowAway,
                ":You have been marked as being away".to_string(),
            )
            .await;
        } else {
            self.send_numeric_reply(
                NumericReply::RplUnAway,
                ":You are no longer marked as being away".to_string(),
            )
            .await;
        }
    }

    async fn on_userhost(&self, message: Message) {
        if message.params.len() < 1 {
            self.send_numeric_reply(
                NumericReply::ErrNeedMoreParams,
                "USERHOST :Not enough parameters".to_string(),
            )
            .await;
            return;
        }

        self.server.handle_userhost(self, message.params).await;
    }

    async fn on_ison(&self, message: Message) {
        if message.params.len() < 1 {
            self.send_numeric_reply(
                NumericReply::ErrNeedMoreParams,
                "ISON :Not enough parameters".to_string(),
            )
            .await;
            return;
        }

        let mut nicks = vec![];
        for param in message.params {
            if self.server.is_nick_mapped(&param).await {
                nicks.push(param);
            }
        }

        self.send_numeric_reply(NumericReply::RplIsOn, format!(":{}", nicks.join(" ")))
            .await;
    }

    fn is_nick_valid(nick: String) -> bool {
        if nick.len() < 1 {
            return false;
        }

        if nick.len() > 24 {
            return false;
        }

        for ch in nick.chars() {
            if !ch.is_ascii_alphanumeric() && ch != '_' && ch != '-' {
                return false;
            }
        }

        true
    }
}
