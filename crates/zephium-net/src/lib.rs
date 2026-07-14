//! Outbound http(s) fetches on a dedicated worker thread. Everything else in
//! the workspace stays network-free.

use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs};
use std::sync::mpsc::{self, SyncSender};
use std::thread;
use std::time::Duration;

use zephium_core::ports::net::{Fetched, Net};

const MAX_FETCH_URL_BYTES: usize = 8 * 1024;
const MAX_FETCH_BYTES: usize = 1024 * 1024;
const MAX_REDIRECTS: usize = 3;
const MAX_RESOLVED_ADDRESSES: usize = 32;
const MAX_CONTENT_TYPE_BYTES: usize = 128;

struct Job {
    url: String,
    max_bytes: usize,
    done: Box<dyn FnOnce(Option<Fetched>) + Send>,
}

pub struct HttpNet {
    tx: SyncSender<Job>,
}

impl Default for HttpNet {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpNet {
    pub fn new() -> Self {
        // Native fetches are page-influenced and deliberately low priority.
        // Bound retained URLs and callbacks even if the network stalls.
        let (tx, rx) = mpsc::sync_channel::<Job>(16);
        if let Err(error) = thread::Builder::new()
            .name("zephium-net".into())
            .spawn(move || {
                let agent = ureq::AgentBuilder::new()
                    .timeout(Duration::from_secs(10))
                    // Follow redirects manually so every hop receives the
                    // same credential/origin/downgrade policy.
                    .redirects(0)
                    // Page-selected resources must never inherit a developer's
                    // HTTP_PROXY and turn it into a route to a private network.
                    .try_proxy_from_env(false)
                    // Resolve every hop through the same policy. Returning the
                    // vetted addresses also closes the DNS-rebinding gap between
                    // a preflight lookup and the socket connection.
                    .resolver(PublicResolver)
                    .user_agent("Zephium/0.1")
                    .build();
                for job in rx {
                    (job.done)(fetch(&agent, &job.url, job.max_bytes));
                }
            })
        {
            // The receiver is dropped with the rejected closure, so this
            // object remains a deterministic fail-closed network adapter.
            // Resource exhaustion at startup must not become a process abort.
            eprintln!("network: cannot start the native fetch actor: {error}");
        }
        Self { tx }
    }
}

#[derive(Debug)]
struct PublicResolver;

impl ureq::Resolver for PublicResolver {
    fn resolve(&self, netloc: &str) -> std::io::Result<Vec<SocketAddr>> {
        vetted_addresses(netloc.to_socket_addrs()?)
    }
}

fn vetted_addresses(
    addresses: impl IntoIterator<Item = SocketAddr>,
) -> std::io::Result<Vec<SocketAddr>> {
    let mut vetted = Vec::with_capacity(MAX_RESOLVED_ADDRESSES);
    for address in addresses {
        // Do not collect an attacker-influenced resolver iterator before
        // applying a bound. Rejecting the whole answer is safer than silently
        // truncating it and giving rebinding/failover behavior a different set
        // of destinations than the resolver returned.
        if vetted.len() == MAX_RESOLVED_ADDRESSES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "resolver returned too many destinations",
            ));
        }
        if !is_public_ip(address.ip()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "refusing a non-public network destination",
            ));
        }
        vetted.push(address);
    }
    if vetted.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "resolver returned no destinations",
        ));
    }
    Ok(vetted)
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(a == 0
                || a == 10
                || a == 127
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && b == 0 && c == 0)
                || (a == 192 && b == 0 && c == 2)
                || (a == 192 && b == 88 && c == 99)
                || (a == 192 && b == 168)
                || (a == 198 && (b == 18 || b == 19))
                || (a == 198 && b == 51 && c == 100)
                || (a == 203 && b == 0 && c == 113)
                || a >= 224)
        }
        IpAddr::V6(ip) => {
            // OS transition mechanisms can route an apparently global IPv6
            // address to an embedded IPv4 destination. Apply the IPv4 policy
            // to mapped/compatible, well-known NAT64, and 6to4 forms too.
            if let Some(ip) = embedded_ipv4(ip) {
                return is_public_ip(IpAddr::V4(ip));
            }
            let s = ip.segments();
            !(ip.is_unspecified()
                || ip.is_loopback()
                || ip.is_multicast()
                // RFC 8215 local-use NAT64. Unlike the well-known /96 above,
                // this prefix is not globally routed and may translate to a
                // private IPv4 destination.
                || (s[0] == 0x0064 && s[1] == 0xff9b && s[2] == 0x0001)
                // RFC 6666 discard-only and non-public transition/benchmark/
                // identifier ranges must not become native-fetch routes.
                || (s[0] == 0x0100 && s[1] == 0 && s[2] == 0 && s[3] == 0)
                || (s[0] == 0x2001 && s[1] == 0)
                || (s[0] == 0x2001 && s[1] == 2 && s[2] == 0)
                || (s[0] == 0x2001 && (s[1] & 0xfff0) == 0x0010)
                || (s[0] == 0x2001 && (s[1] & 0xfff0) == 0x0020)
                || (s[0] & 0xfe00) == 0xfc00
                || (s[0] & 0xffc0) == 0xfe80
                || (s[0] & 0xffc0) == 0xfec0
                || (s[0] == 0x2001 && s[1] == 0x0db8)
                || (s[0] == 0x3fff && (s[1] & 0xf000) == 0))
        }
    }
}

fn embedded_ipv4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    if let Some(ip) = ip.to_ipv4() {
        return Some(ip);
    }
    let bytes = ip.octets();
    // RFC 6052 well-known NAT64 prefix 64:ff9b::/96.
    if bytes[..12] == [0x00, 0x64, 0xff, 0x9b, 0, 0, 0, 0, 0, 0, 0, 0] {
        return Some(Ipv4Addr::new(bytes[12], bytes[13], bytes[14], bytes[15]));
    }
    // RFC 3056 6to4 prefix 2002::/16 embeds IPv4 immediately afterwards.
    if bytes[..2] == [0x20, 0x02] {
        return Some(Ipv4Addr::new(bytes[2], bytes[3], bytes[4], bytes[5]));
    }
    None
}

impl Net for HttpNet {
    fn fetch(
        &self,
        url: String,
        max_bytes: usize,
        done: Box<dyn FnOnce(Option<Fetched>) + Send>,
    ) -> bool {
        // Validate before admission so a dormant or stalled worker cannot be
        // used as a bounded-count but unbounded-byte retention queue.
        if !request_in_bounds(&url, max_bytes) {
            return false;
        }
        self.tx
            .try_send(Job {
                url,
                max_bytes,
                done,
            })
            .is_ok()
    }
}

fn fetch(agent: &ureq::Agent, url: &str, max_bytes: usize) -> Option<Fetched> {
    if !request_in_bounds(url, max_bytes) {
        return None;
    }
    let mut current = url::Url::parse(url).ok()?;
    if !valid_fetch_url(&current) {
        return None;
    }
    let required_origin = current.origin();

    for redirect_count in 0..=MAX_REDIRECTS {
        let response = agent.get(current.as_str()).call().ok()?;
        if (300..400).contains(&response.status()) {
            if redirect_count == MAX_REDIRECTS {
                return None;
            }
            let location = response.header("Location")?;
            current = checked_redirect(&current, &required_origin, location)?;
            continue;
        }
        if !(200..300).contains(&response.status()) {
            return None;
        }
        if response
            .header("Content-Length")
            .and_then(|value| value.parse::<usize>().ok())
            .is_some_and(|length| length > max_bytes)
        {
            return None;
        }
        let content_type = bounded_content_type(response.content_type());
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(max_bytes as u64 + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.is_empty() || bytes.len() > max_bytes {
            return None;
        }
        return Some(Fetched {
            content_type,
            bytes,
        });
    }
    None
}

fn bounded_content_type(value: &str) -> Option<String> {
    (!value.is_empty()
        && value.len() <= MAX_CONTENT_TYPE_BYTES
        && value
            .bytes()
            .all(|byte| byte == b'\t' || (0x20..=0x7e).contains(&byte)))
    .then(|| value.to_owned())
}

fn request_in_bounds(url: &str, max_bytes: usize) -> bool {
    !url.is_empty()
        && url.len() <= MAX_FETCH_URL_BYTES
        && (1..=MAX_FETCH_BYTES).contains(&max_bytes)
        && url::Url::parse(url).is_ok_and(|url| valid_fetch_url(&url))
}

fn valid_fetch_url(url: &url::Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
}

fn checked_redirect(
    current: &url::Url,
    required_origin: &url::Origin,
    location: &str,
) -> Option<url::Url> {
    if location.len() > MAX_FETCH_URL_BYTES {
        return None;
    }
    let next = current.join(location).ok()?;
    // Same-origin also rejects HTTPS-to-HTTP downgrade. This small native
    // client must not become a page-controlled cross-origin request primitive
    // that bypasses browser policy.
    (valid_fetch_url(&next)
        && next.as_str().len() <= MAX_FETCH_URL_BYTES
        && next.origin() == *required_origin)
        .then_some(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_driven_fetches_reject_private_and_special_networks() {
        for ip in [
            "0.0.0.0",
            "10.0.0.1",
            "100.64.0.1",
            "127.0.0.1",
            "169.254.169.254",
            "172.16.0.1",
            "192.168.1.1",
            "198.18.0.1",
            "224.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "::ffff:127.0.0.1",
            "::127.0.0.1",
            "64:ff9b::7f00:1",
            "64:ff9b:1::7f00:1",
            "100::1",
            "2001::1",
            "2001:2::1",
            "2001:10::1",
            "2001:20::1",
            "2002:7f00:1::",
            "3fff::1",
            "fc00::1",
            "fe80::1",
            "ff02::1",
            "2001:db8::1",
        ] {
            let ip: IpAddr = ip.parse().unwrap();
            assert!(!is_public_ip(ip), "accepted special address {ip}");
        }
    }

    #[test]
    fn resolver_answers_are_validated_while_bounded() {
        let public: SocketAddr = "1.1.1.1:443".parse().unwrap();
        let private: SocketAddr = "127.0.0.1:443".parse().unwrap();
        assert_eq!(
            vetted_addresses(std::iter::repeat_n(public, MAX_RESOLVED_ADDRESSES)).unwrap(),
            vec![public; MAX_RESOLVED_ADDRESSES]
        );
        assert!(vetted_addresses(std::iter::empty()).is_err());
        assert!(vetted_addresses([public, private]).is_err());
        assert!(vetted_addresses(std::iter::repeat_n(public, MAX_RESOLVED_ADDRESSES + 1)).is_err());
    }

    #[test]
    fn response_metadata_is_bounded_before_copying() {
        assert_eq!(
            bounded_content_type("image/png").as_deref(),
            Some("image/png")
        );
        assert_eq!(bounded_content_type("text/plain\r\nInjected: yes"), None);
        assert_eq!(
            bounded_content_type(&"x".repeat(MAX_CONTENT_TYPE_BYTES + 1)),
            None
        );
    }

    #[test]
    fn page_driven_fetches_allow_public_networks() {
        for ip in ["1.1.1.1", "8.8.8.8", "2606:4700:4700::1111"] {
            let ip: IpAddr = ip.parse().unwrap();
            assert!(is_public_ip(ip), "rejected public address {ip}");
        }
    }

    #[test]
    fn page_driven_fetch_urls_reject_credentials_and_non_web_schemes() {
        for url in [
            "https://user@example.com/icon.png",
            "https://user:secret@example.com/icon.png",
            "file:///tmp/icon.png",
            "data:image/png;base64,AA==",
        ] {
            assert!(!valid_fetch_url(&url::Url::parse(url).unwrap()), "{url}");
        }
        assert!(valid_fetch_url(
            &url::Url::parse("https://example.com/icon.png").unwrap()
        ));
        assert!(!request_in_bounds(
            "https://example.com/icon.png",
            MAX_FETCH_BYTES + 1
        ));
        assert!(!request_in_bounds(
            &format!("https://example.com/{}", "x".repeat(MAX_FETCH_URL_BYTES)),
            1
        ));
    }

    #[test]
    fn redirects_cannot_change_origin_downgrade_or_add_credentials() {
        let current = url::Url::parse("https://example.com/icons/a.png").unwrap();
        let origin = current.origin();
        assert_eq!(
            checked_redirect(&current, &origin, "../favicon.ico")
                .unwrap()
                .as_str(),
            "https://example.com/favicon.ico"
        );
        for location in [
            "http://example.com/favicon.ico",
            "https://cdn.example/favicon.ico",
            "https://user@example.com/favicon.ico",
            "file:///tmp/favicon.ico",
        ] {
            assert!(
                checked_redirect(&current, &origin, location).is_none(),
                "accepted redirect {location}"
            );
        }
    }
}
