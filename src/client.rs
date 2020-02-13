use crate::replies::NumericReply;
use crate::server::Server;

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

use ircmsgprs::parser::{Message, Parser};

pub struct Client {
    pub nick: Mutex<String>,
    pub user: Mutex<String>,
    pub real_name: Mutex<String>,
    pub registered: Mutex<bool>,
    pub operator: Mutex<bool>,
    server: Arc<Server>,
    address: SocketAddr,
    parser: Mutex<Parser>,
}

impl Client {
    pub fn new(server: Arc<Server>, address: SocketAddr) -> Client {
        Client {
            nick: Mutex::new(String::new()),
            user: Mutex::new(String::new()),
            real_name: Mutex::new(String::new()),
            registered: Mutex::new(false),
            operator: Mutex::new(false),
            server: server,
            address: address,
            parser: Mutex::new(Parser::new()),
        }
    }

    pub async fn task(&self, mut stream: TcpStream) {
        let (reader, _writer) = stream.split();
        let mut line = String::new();
        let mut buf_reader = BufReader::new(reader);

        loop {
            let size = buf_reader.read_line(&mut line).await.unwrap();
            if size == 0 {
                break;
            } else {
                let result = self.parser.lock().await.parse(line.clone());
                if result.is_none() {
                    println!("Client parse error.");
                    break;
                }
                self.on_message(result.unwrap()).await;
            }

            line.clear();
        }

        println!("Client disconnected ({}).", self.address);
    }

    async fn send_numeric_reply(&self, _reply: NumericReply, _message: String) {}

    async fn on_message(&self, message: Message) {
        println!("Received message: {}", message);
        match message.command.as_str() {
            "CAP" => {
                self.on_cap(message);
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
            _ => {
                println!("Command {} not implemented.", message.command);
            }
        }
    }

    fn on_cap(&self, _message: Message) {
        println!("Ignoring CAP command (IRCv3)");
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
            (*self.registered.lock().await) = true;
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
            if self.server.is_operator(name, password).await {
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
}
