use async_trait::async_trait;

use crate::client::Client;

#[async_trait]
pub trait Service {
    async fn on_message(&self, client: &Client, params: Vec<&str>);
}
