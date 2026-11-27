// Config\lockfile of a running Riot Client: `name:pid:port:password:protocol` on one line. The file is read for one thing, the launch request the client's own Play button sends, at the moment of that request: the Riot loopback channel of ol-net reads it and is the one caller of `parse_lockfile`. Its presence says the client runs; nothing else ever opens it.

use zeroize::{Zeroize, Zeroizing};

use crate::js;

const LOCKFILE_NAME: &str = "Riot Client";

// The password of one run of the client's local server. No Debug and no copy, and wiped when dropped: it is used once and kept nowhere.
pub struct LockfilePassword(String);

impl LockfilePassword {
    // The one use. What comes back is wiped when it is dropped.
    pub fn take(mut self) -> Zeroizing<String> {
        Zeroizing::new(std::mem::take(&mut self.0))
    }
}

impl Drop for LockfilePassword {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

pub struct Lockfile {
    pub port: u16,
    pub password: LockfilePassword,
}

// Only the client's own https lockfile with a port that can be one counts.
pub fn parse_lockfile(text: &str) -> Option<Lockfile> {
    let parts: Vec<&str> = js::trim(text).split(':').collect();
    let [name, pid, port, password, protocol] = parts.as_slice() else {
        return None;
    };
    if *name != LOCKFILE_NAME
        || *protocol != "https"
        || !js::is_ascii_digits(pid)
        || password.is_empty()
    {
        return None;
    }
    if port.len() > 5 || !js::is_ascii_digits(port) {
        return None;
    }
    let port = port.parse::<u16>().ok().filter(|port| *port >= 1)?;
    Some(Lockfile {
        port,
        password: LockfilePassword((*password).to_owned()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clients_own_lockfile_gives_its_port_and_its_password_once() {
        let lock = parse_lockfile("Riot Client:4242:51234:placeholder-lockfile-password:https\r\n")
            .expect("a lockfile of the client");
        assert_eq!(lock.port, 51234);
        assert_eq!(
            lock.password.take().as_str(),
            "placeholder-lockfile-password"
        );
        for other in [
            "",
            "Riot Client:4242:51234:placeholder-lockfile-password:http",
            "LeagueClient:4242:51234:placeholder-lockfile-password:https",
            "Riot Client:4242:0:placeholder-lockfile-password:https",
            "Riot Client:4242:65536:placeholder-lockfile-password:https",
            "Riot Client:4242:51234::https",
            "Riot Client:x:51234:placeholder-lockfile-password:https",
        ] {
            assert!(parse_lockfile(other).is_none(), "{other}");
        }
    }
}
