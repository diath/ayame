use ircmsgprs::parser::Message;

pub trait Service {
    fn on_message(&self, message: Message);
}
