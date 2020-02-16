use crate::replies::NumericReply;
use crate::server::Server;

use std::collections::HashSet;
use std::io::ErrorKind;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::SystemTime;

use chrono::prelude::DateTime;
use chrono::Utc;

use tokio::io::{split, AsyncBufReadExt, AsyncWriteExt, BufReader, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

use ircmsgprs::parser::{Message, Parser};

pub struct Client {
    pub nick: Mutex<String>,
    pub user: Mutex<String>,
    pub real_name: Mutex<String>,
    pub password: Mutex<String>,
    pub registered: Mutex<bool>,
    pub operator: Mutex<bool>,
    pub channels: Mutex<HashSet<String>>,
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
            real_name: Mutex::new(String::new()),
            password: Mutex::new(String::new()),
            registered: Mutex::new(false),
            operator: Mutex::new(false),
            channels: Mutex::new(HashSet::new()),
            server: server,
            address: address,
            writer: Mutex::new(None),
            parser: Mutex::new(Parser::new()),
        }
    }

    pub async fn get_prefix(&self) -> String {
        return format!(
            "{}!{}@{}",
            self.nick.lock().await,
            self.user.lock().await,
            self.address
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
                            println!("Client parse error.");
                            break;
                        }
                        self.on_message(result.unwrap()).await;
                    }
                }
                Err(err) => {
                    if err.kind() != ErrorKind::InvalidData {
                        self.server.broadcast_quit(&self, "Read Error").await;
                        eprintln!("Client read error ({}).", err);
                        break;
                    }
                }
            }

            line.clear();
        }

        self.server.remove_from_channels(&self).await;

        let nick = self.nick.lock().await;
        if nick.len() != 0 {
            self.server.unmap_nick(nick.to_string()).await;
        }

        self.server.unmap_client(&self).await;

        println!("Client disconnected ({}).", self.address);
    }

    pub async fn send_raw(&self, message: String) {
        if let Some(writer) = &mut *self.writer.lock().await {
            match writer
                .write_all(format!("{}\r\n", message).as_bytes())
                .await
            {
                Ok(_) => {}
                Err(_) => {
                    println!("Failed to write message ({})", message);
                }
            }
        }
    }

    pub async fn send_numeric_reply(&self, reply: NumericReply, message: String) {
        let nick = self.nick.lock().await;
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
            format!("Welcome to {} {}", self.server.name, prefix),
        )
        .await;

        /* TODO(diath): We should probably store software version somewhere. */
        self.send_numeric_reply(
            NumericReply::RplYourHost,
            format!(
                "Your host is {} running version {}",
                self.server.name, "ayame 0.1.0"
            ),
        )
        .await;

        /* TODO(diath): We should probably store first startup time somewhere. */
        self.send_numeric_reply(
            NumericReply::RplCreated,
            format!("This server was created {}", self.server.created),
        )
        .await;

        self.server.send_motd(&self).await;
        /* TODO(diath): Send RPL_MYINFO with <servername> <version> <user modes> <server modes>. */
    }

    async fn on_message(&self, message: Message) {
        println!("Received message: {}", message);
        match message.command.as_str() {
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
            "PRIVMSG" => {
                self.on_privmsg(message).await;
            }
            "JOIN" => {
                self.on_join(message).await;
            }
            "PART" => {
                self.on_part(message).await;
            }
            "TOPIC" => {
                self.on_topic(message).await;
            }
            "MOTD" => {
                self.on_motd(message).await;
            }
            "TIME" => {
                self.on_time(message).await;
            }
            "REHASH" => {
                self.on_rehash().await;
            }
            "DIE" | "RESTART" => {
                if *self.registered.lock().await {
                    self.send_numeric_reply(
                        NumericReply::ErrNoPrivileges,
                        ":Permission Denied- You're not an IRC operator".to_string(),
                    )
                    .await;
                }
            }
            "SUMMON" => {
                if *self.registered.lock().await {
                    self.send_numeric_reply(
                        NumericReply::ErrSummonDisabled,
                        ":SUMMON has been disabled".to_string(),
                    )
                    .await;
                }
            }
            _ => {
                if *self.registered.lock().await {
                    self.send_numeric_reply(
                        NumericReply::ErrUnknownCommand,
                        format!("{} :Unknown command", message.command),
                    )
                    .await;
                }
                println!("Command {} not implemented.", message.command);
            }
        }
    }

    fn on_cap(&self, _message: Message) {
        println!("Ignoring CAP command (IRCv3)");
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
                if let Some(_) = nick.chars().position(|c| !c.is_ascii_alphanumeric()) {
                    self.send_numeric_reply(
                        NumericReply::ErrErroneousNickname,
                        format!("{} :Erroneous nickname", nick),
                    )
                    .await;
                } else {
                    if self.nick.lock().await.len() == 0 {
                        self.server.map_nick(nick.to_string(), &self).await;

                        if !*self.registered.lock().await && self.user.lock().await.len() != 0 {
                            self.complete_registration().await;
                        }
                    } else {
                        self.server
                            .remap_nick(self.nick.lock().await.to_string(), nick.to_string())
                            .await;
                    }
                    (*self.nick.lock().await) = nick.to_string();
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
        if !*self.registered.lock().await || *self.operator.lock().await {
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

    async fn on_privmsg(&self, message: Message) {
        /* TODO(diath): ERR_NOTOPLEVEL, ERR_WILDTOPLEVEL, ERR_BADMASK */
        if !*self.registered.lock().await {
            return;
        }

        if message.params.len() < 1 {
            self.send_numeric_reply(
                NumericReply::ErrNoRecipient,
                ":No recipient given (PRIVMSG)".to_string(),
            )
            .await;
        } else if message.params.len() < 2 {
            self.send_numeric_reply(
                NumericReply::ErrNoTextToSend,
                ":No text to send".to_string(),
            )
            .await;
        } else {
            let targets = message.params[0].split(",");
            let text = message.params[1].clone();

            for target in targets {
                if target.len() == 0 {
                    continue;
                }

                match &target[0..1] {
                    "#" => {
                        if self.server.is_channel_mapped(target).await {
                            let nick = self.nick.lock().await.to_string();
                            if self.server.has_channel_participant(target, &nick).await {
                                self.server
                                    .forward_channel_message(
                                        self.get_prefix().await,
                                        target,
                                        text.clone(),
                                    )
                                    .await;
                            } else {
                                /* TODO(diath): Check for channel +n mode (if user not on a channel) or channel +m mode (if user not +v) */
                                self.send_numeric_reply(
                                    NumericReply::ErrCannotSendToChan,
                                    format!("{} :Cannot send to channel", target).to_string(),
                                )
                                .await;
                            }
                        } else {
                            self.send_numeric_reply(
                                NumericReply::ErrNoSuchChannel,
                                format!("{} :No such channel", target).to_string(),
                            )
                            .await;
                        }
                    }
                    //* NOTE(diath): Technically a channel can be prefixed with either # (network), ! (safe), + (unmoderated) or & (local) but we only support #. */
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
                                .forward_message(self.get_prefix().await, target, text.clone())
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
    }

    async fn on_join(&self, message: Message) {
        /* TODO(diath): ERR_INVITEONLYCHAN, ERR_CHANNELISFULL, ERR_TOOMANYTARGETS, ERR_BANNEDFROMCHAN, ERR_BADCHANNELKEY, ERR_BADCHANMASK, ERR_TOOMANYCHANNELS, ERR_UNAVAILRESOURCE */
        /* TODO(diath): Channel keys */
        if !*self.registered.lock().await {
            return;
        }

        let nick = self.nick.lock().await.to_string();
        if message.params[0] == "0" {
            let nick = self.nick.lock().await;
            for channel in &*self.channels.lock().await {
                self.server.part_channel(&channel, &nick, "Leaving").await;
            }
            self.channels.lock().await.clear();
        } else {
            let targets = message.params[0].split(",");
            for target in targets {
                if target.len() == 0 {
                    continue;
                }

                if !self.server.is_channel_mapped(target).await {
                    self.server.create_channel(target).await;
                }

                if self.server.join_channel(target, &nick).await {
                    self.channels.lock().await.insert(target.to_string());
                }
            }
        }
    }

    async fn on_part(&self, message: Message) {
        if !*self.registered.lock().await {
            return;
        }

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

                if self.server.part_channel(target, &nick, &part_message).await {
                    self.channels.lock().await.remove(target);
                }
            }
        }
    }

    async fn on_topic(&self, message: Message) {
        /* TODO(diath): ERR_CHANOPRIVSNEEDED, ERR_NOCHANMODES */
        if !*self.registered.lock().await {
            return;
        }

        if message.params.len() < 1 {
            self.send_numeric_reply(
                NumericReply::ErrNeedMoreParams,
                "TOPIC :Not enough parameters".to_string(),
            )
            .await;
        } else if message.params.len() < 2 {
            let channel = message.params[0].clone();
            if self
                .server
                .is_channel_mapped(channel.clone().as_str())
                .await
            {
                if let Some(topic) = self.server.get_channel_topic(&channel).await {
                    let text = topic.text.lock().await;
                    if text.len() == 0 {
                        self.send_numeric_reply(
                            NumericReply::RplNoTopic,
                            format!("{} :No topic is set", channel).to_string(),
                        )
                        .await;
                    } else {
                        self.send_numeric_reply(
                            NumericReply::RplTopic,
                            format!("{} :{}", channel, text).to_string(),
                        )
                        .await;

                        let set_by = topic.set_by.lock().await;
                        let set_at = topic.set_at.lock().await;
                        self.send_numeric_reply(
                            NumericReply::RplTopicSet,
                            format!("{} {} {}", channel, set_by, set_at).to_string(),
                        )
                        .await;
                    }
                }
            } else {
                self.send_numeric_reply(
                    NumericReply::ErrNoSuchChannel,
                    format!("{} :No such channel", channel).to_string(),
                )
                .await;
            }
        } else {
            let channel = message.params[0].clone();
            let nick = self.nick.lock().await.to_string();
            if self.server.has_channel_participant(&channel, &nick).await {
                self.server
                    .set_channel_topic(nick, &channel, message.params[1].clone())
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

    async fn on_motd(&self, _message: Message) {
        if !*self.registered.lock().await {
            return;
        }

        /* TODO: add support for <target> */
        self.server.send_motd(&self).await;
    }

    async fn on_time(&self, _message: Message) {
        if !*self.registered.lock().await {
            return;
        }

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
        if !*self.registered.lock().await {
            return;
        }

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
}
