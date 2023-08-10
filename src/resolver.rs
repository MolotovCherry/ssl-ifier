use std::net::{SocketAddr, ToSocketAddrs};

#[derive(Debug, Default)]
pub struct Addresses {
    pub ipv4: Option<SocketAddr>,
    pub ipv6: Option<SocketAddr>,
}

/// Convert a address like `localhost:1234`, or `localhost`,
/// to an socket address with port, like `127.0.0.1:1234` or `127.0.0.1`
/// returns both ipv4 and ipv6 (if there is one)
pub fn get_addresses(addr: &str) -> anyhow::Result<Addresses> {
    let mut addresses = Addresses::default();
    addr.to_socket_addrs()?.for_each(|s| match s {
        a @ SocketAddr::V4(_) => {
            addresses.ipv4 = Some(a);
        }

        a @ SocketAddr::V6(_) => {
            addresses.ipv6 = Some(a);
        }
    });

    Ok(addresses)
}

pub fn get_port(addr: &str) -> Option<&str> {
    if let Some(index) = addr.rfind(':') {
        let port = &addr[index + 1..];
        // validate it is actually a port number
        port.parse::<u16>().ok()?;
        Some(port)
    } else {
        None
    }
}
