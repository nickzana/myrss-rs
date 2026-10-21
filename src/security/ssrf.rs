use reqwest::dns::{Addrs, Resolve, Resolving};
use std::{
    net::{IpAddr, ToSocketAddrs},
    sync::Arc,
};
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum UrlGuardError {
    #[error("URL must be absolute")]
    NotAbsolute,
    #[error("only http and https feeds are supported")]
    Scheme,
    #[error("URL credentials are not allowed")]
    Credentials,
    #[error("feed port is not allowed")]
    Port,
    #[error("host cannot be resolved")]
    Resolve,
    #[error("destination is on a non-public network")]
    PrivateNetwork,
}

pub fn validate_url(
    raw: &str,
    allow_private: bool,
    allowed_hosts: &[String],
) -> Result<Url, UrlGuardError> {
    let url = Url::parse(raw).map_err(|_| UrlGuardError::NotAbsolute)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(UrlGuardError::Scheme);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(UrlGuardError::Credentials);
    }
    let port = url.port_or_known_default().ok_or(UrlGuardError::Port)?;
    let host = url.host_str().ok_or(UrlGuardError::Resolve)?;
    let explicitly_allowed = allowed_hosts.iter().any(|h| h.eq_ignore_ascii_case(host));
    if !(port == 80 || port == 443 || allow_private || explicitly_allowed) {
        return Err(UrlGuardError::Port);
    }
    if allow_private || explicitly_allowed {
        return Ok(url);
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        if !is_public_ip(ip) {
            return Err(UrlGuardError::PrivateNetwork);
        }
        return Ok(url);
    }
    let mut resolved = (host, port)
        .to_socket_addrs()
        .map_err(|_| UrlGuardError::Resolve)?;
    let mut found = false;
    for addr in &mut resolved {
        found = true;
        if !is_public_ip(addr.ip()) {
            return Err(UrlGuardError::PrivateNetwork);
        }
    }
    if !found {
        return Err(UrlGuardError::Resolve);
    }
    Ok(url)
}

pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_multicast()
                || v4.is_unspecified()
                || v4.octets()[0] == 0
                || (v4.octets()[0] == 100 && (64..=127).contains(&v4.octets()[1])))
        }
        IpAddr::V6(v6) => {
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || v6.is_unique_local()
                || v6.is_unicast_link_local())
        }
    }
}

#[derive(Debug, Clone)]
pub struct PublicDnsResolver {
    allow_private: bool,
    allowed_hosts: Arc<[String]>,
}

impl PublicDnsResolver {
    pub fn new(allow_private: bool, allowed_hosts: Vec<String>) -> Self {
        Self {
            allow_private,
            allowed_hosts: allowed_hosts.into(),
        }
    }
}

impl Resolve for PublicDnsResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> Resolving {
        let host = name.as_str().to_owned();
        let allow_private = self.allow_private;
        let explicitly_allowed = self
            .allowed_hosts
            .iter()
            .any(|h| h.eq_ignore_ascii_case(&host));
        Box::pin(async move {
            let addrs = tokio::net::lookup_host((host.as_str(), 0))
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?
                .collect::<Vec<_>>();
            if addrs.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "host did not resolve",
                )
                .into());
            }
            if !allow_private
                && !explicitly_allowed
                && addrs.iter().any(|addr| !is_public_ip(addr.ip()))
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "destination is on a non-public network",
                )
                .into());
            }
            let out: Addrs = Box::new(addrs.into_iter());
            Ok(out)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_local_and_non_http() {
        assert!(matches!(
            validate_url("http://127.0.0.1/feed", false, &[]),
            Err(UrlGuardError::PrivateNetwork)
        ));
        assert!(matches!(
            validate_url("file:///tmp/feed", false, &[]),
            Err(UrlGuardError::Scheme)
        ));
        assert!(validate_url("http://example.com:8080/feed", false, &[]).is_err());
    }
}
