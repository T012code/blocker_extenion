//! Is there a network? Any adapter that's up with a usable address (not loopback,
//! not link-local, not a virtual switch for VMs or containers).

use std::net::IpAddr;

const VIRTUAL: &[&str] = &[
    "vethernet",
    "virtualbox",
    "vmware",
    "hyper-v",
    "loopback",
    "wsl",
    "docker",
    "vboxnet",
];

pub fn online() -> bool {
    match if_addrs::get_if_addrs() {
        Ok(list) => list.iter().any(|i| {
            let name = i.name.to_ascii_lowercase();
            !i.is_loopback() && usable(i.ip()) && !VIRTUAL.iter().any(|v| name.contains(v))
        }),
        // Can't tell: assume online (downloads notice a lost connection themselves).
        Err(_) => true,
    }
}

fn usable(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => !v4.is_loopback() && !v4.is_link_local() && !v4.is_unspecified(),
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            !v6.is_loopback() && !v6.is_unspecified() && (first & 0xffc0) != 0xfe80
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_local_and_loopback_dont_count() {
        assert!(!usable("127.0.0.1".parse().unwrap()));
        assert!(!usable("169.254.10.2".parse().unwrap()));
        assert!(!usable("fe80::1".parse().unwrap()));
        assert!(!usable("::1".parse().unwrap()));
        assert!(usable("192.168.1.20".parse().unwrap()));
        assert!(usable("2a01:4f8::1".parse().unwrap()));
    }
}
