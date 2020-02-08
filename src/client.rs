use std::net::SocketAddr;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpStream;

use ircmsgprs::parser::{Message, Parser};

pub struct Client {
    address: SocketAddr,
    parser: Parser,
}

impl Client {
    pub fn new(address: SocketAddr) -> Client {
        Client {
            address: address,
            parser: Parser::new(),
        }
    }

    pub async fn task(&mut self, mut stream: TcpStream) {
        let (reader, _writer) = stream.split();
        let mut buf_reader = BufReader::new(reader);
        let mut buffer = vec![];

        loop {
            // TODO(diath): Should it read until \r\n?
            match buf_reader.read_until(b'\n', &mut buffer).await {
                Ok(size) => {
                    if size == 0 {
                        break;
                    } else {
                        let line = String::from_utf8(buffer.clone());
                        if line.is_ok() {
                            let result = self.parser.parse(line.unwrap());
                            if result.is_none() {
                                println!("Client parse error.");
                                return;
                            }
                            self.on_message(result.unwrap());
                        }
                    }

                    buffer.clear();
                }
                Err(error) => {
                    eprintln!("Client error: {}.", error);
                    break;
                }
            }
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
