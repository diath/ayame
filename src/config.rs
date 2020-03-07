use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub oper: Option<Vec<OperConfig>>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ServerConfig {
    pub name: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub motd_path: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct OperConfig {
    pub name: Option<String>,
    pub password: Option<String>,
}
