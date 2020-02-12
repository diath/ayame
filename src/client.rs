use crate::server::Server;

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

use ircmsgprs::parser::{Message, Parser};

pub struct Client {
    pub name: Mutex<String>,
    server: Arc<Server>,
    address: SocketAddr,
    parser: Mutex<Parser>,
}

impl Client {
    pub fn new(server: Arc<Server>, address: SocketAddr) -> Client {
        Client {
            name: Mutex::new(String::new()),
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
                self.on_message(result.unwrap());
            }

            line.clear();
        }

        println!("Client disconnected ({}).", self.address);
    }

    fn on_message(&self, message: Message) {
        println!("Received message: {}", message);
        match message.command {
            _ => {
                println!("Command {} not implemented.", message.command);
            }
        }
    }
}
