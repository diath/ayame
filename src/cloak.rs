use crate::client::UserHost;

use crypto::digest::Digest;
use crypto::sha1::Sha1;

pub fn get_cloaked_host(host: UserHost) -> String {
    match host {
        UserHost::IPv4(s) => get_cloaked_host_ipv4(s.to_string()),
        UserHost::IPv6(s) => get_cloaked_host_ipv6(s.to_string()),
        UserHost::VHost(s) => s.to_string(),
    }
}

fn get_cloaked_host_ipv4(host: String) -> String {
    let mut chunks = host.split(".").collect::<Vec<&str>>();
    if chunks.len() != 4 {
        return host;
    }

    chunks.remove(chunks.len() - 1);

    let mut result: Vec<String> = vec![];
    for chunk in chunks {
        let mut hasher = Sha1::new();
        hasher.input_str(chunk);
        result.push(hasher.result_str().to_string()[0..8].to_string());
    }
    result.push("IP".to_string());

    result.join(".")
}

fn get_cloaked_host_ipv6(host: String) -> String {
    let mut chunks = host.split(":").collect::<Vec<&str>>();
    if chunks.len() == 0 {
        return host;
    }

    chunks.remove(chunks.len() - 1);

    let mut result: Vec<String> = vec![];
    for chunk in chunks {
        if chunk.len() == 0 {
            result.push("".to_string());
        }

        let mut hasher = Sha1::new();
        hasher.input_str(chunk);
        result.push(hasher.result_str().to_string()[0..8].to_string());
    }
    result.push("IPv6".to_string());

    result.join(":")
}
