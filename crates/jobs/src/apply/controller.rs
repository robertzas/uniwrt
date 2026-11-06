//! The controller's address as the gateway sees its login, for the `uniwrt.<domain>` record
//! render writes when the address is inside one of the networks (features/network.md, DHCP &
//! local DNS).

use std::net::IpAddr;

use uniwrt_node::ExecRequest;

use crate::Remote;

/// The client address of Dropbear's `SSH_CONNECTION` (`<client> <port> <server> <port>`).
/// `None` when it can't be read, which only leaves the record out.
pub(crate) async fn read(remote: &dyn Remote) -> Option<IpAddr> {
    let output = remote.exec(ExecRequest::new(["env"])).await.ok()?;
    let output = output.check("env").ok()?;
    ssh_client(&String::from_utf8_lossy(&output.stdout))
}

/// The client address of an `env` listing's `SSH_CONNECTION`.
fn ssh_client(env: &str) -> Option<IpAddr> {
    env.lines()
        .find_map(|line| line.strip_prefix("SSH_CONNECTION="))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::ssh_client;

    #[test]
    fn the_client_of_ssh_connection() {
        let env = "USER=root\nSSH_CONNECTION=10.0.0.2 50022 10.0.0.1 22\nHOME=/root\n";
        assert_eq!(ssh_client(env), "10.0.0.2".parse().ok());
        let v6 = "SSH_CONNECTION=fd12::2 50022 fd12::1 22\n";
        assert_eq!(ssh_client(v6), "fd12::2".parse().ok());
        assert_eq!(ssh_client("USER=root\n"), None);
        assert_eq!(ssh_client("SSH_CONNECTION=\n"), None);
    }
}
