// Hybrid DNS resolution for outbound requests.
//
// PROBLEM
// ───────
// 1. Some users sit behind ISPs that DNS-block our update/license hosts.
// 2. A DoH-only resolver hung indefinitely on Windows (hickory-resolver's
//    DoH bootstrap stalls on some networks even when curl to 1.1.1.1
//    works fine — likely a rustls/TLS-cert init issue on first call).
//
// STRATEGY
// ────────
// System DNS first with a 3 s timeout (fast path for the 99% case).
// Cloudflare DoH fallback (1.1.1.1, hostname-pinned) with a 5 s timeout
// for users whose system DNS is blocked. Hard 8 s ceiling so reqwest
// can never sit on a stuck future.
//
// SAFETY MODEL
// ────────────
// The Tauri updater verifies an Ed25519 minisign signature on the
// downloaded artifact, so DNS is untrusted-by-design here. A hostile
// DoH response can't smuggle a fake update.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use hickory_resolver::config::{ResolverConfig, ResolverOpts, CLOUDFLARE};
use hickory_resolver::net::runtime::TokioRuntimeProvider;
use hickory_resolver::TokioResolver;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::Client;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
const SYSTEM_DNS_TIMEOUT: Duration = Duration::from_secs(3);
const DOH_TIMEOUT: Duration = Duration::from_secs(5);

struct HybridResolver {
    doh: Arc<TokioResolver>,
}

impl HybridResolver {
    fn new() -> Self {
        // Cloudflare's published DoH endpoints, hostname-pinned (1.1.1.1 /
        // 1.0.0.1 with TLS SNI "cloudflare-dns.com"). No bootstrap DNS
        // needed since the IPs are literal.
        let mut builder = TokioResolver::builder_with_config(
            ResolverConfig::https(&CLOUDFLARE),
            TokioRuntimeProvider::default(),
        );
        let opts: &mut ResolverOpts = builder.options_mut();
        opts.timeout = Duration::from_secs(4);
        opts.attempts = 1;
        opts.cache_size = 64;
        let inner = builder
            .build()
            .expect("static Cloudflare DoH resolver configuration is valid");
        Self {
            doh: Arc::new(inner),
        }
    }

    async fn lookup_system(host: &str) -> Result<Vec<SocketAddr>, String> {
        // tokio::net::lookup_host requires a port; we use 0 since reqwest
        // will substitute the real port. Wrapped in a hard timeout so a
        // stalled OS resolver can't block the whole future.
        let target = format!("{}:0", host);
        let result = tokio::time::timeout(SYSTEM_DNS_TIMEOUT, tokio::net::lookup_host(target))
            .await
            .map_err(|_| "System DNS timed out".to_string())?
            .map_err(|e| format!("System DNS error: {}", e))?;
        Ok(result.collect())
    }

    async fn lookup_doh(&self, host: &str) -> Result<Vec<SocketAddr>, String> {
        let lookup = tokio::time::timeout(DOH_TIMEOUT, self.doh.lookup_ip(host))
            .await
            .map_err(|_| "DoH timed out".to_string())?
            .map_err(|e| format!("DoH error: {}", e))?;
        Ok(lookup.iter().map(|ip| SocketAddr::new(ip, 0)).collect())
    }
}

impl Resolve for HybridResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_owned();
        let doh = self.doh.clone();
        Box::pin(async move {
            // Fast path: system DNS. Most users have a working resolver and
            // we don't want to add ~100 ms of DoH round-trip per request.
            match HybridResolver::lookup_system(&host).await {
                Ok(addrs) if !addrs.is_empty() => {
                    let iter: Addrs = Box::new(addrs.into_iter());
                    return Ok(iter);
                }
                _ => {
                    // Fall through to DoH.
                }
            }

            // Slow path: Cloudflare DoH for users whose ISP DNS is broken
            // or actively blocking our hosts.
            let resolver = HybridResolver { doh };
            match resolver.lookup_doh(&host).await {
                Ok(addrs) if !addrs.is_empty() => {
                    let iter: Addrs = Box::new(addrs.into_iter());
                    Ok(iter)
                }
                Ok(_) => Err::<Addrs, Box<dyn std::error::Error + Send + Sync>>(
                    format!("DoH returned no addresses for {}", host).into(),
                ),
                Err(e) => Err::<Addrs, Box<dyn std::error::Error + Send + Sync>>(
                    format!("All resolvers failed for {}: {}", host, e).into(),
                ),
            }
        })
    }
}

/// Build a `reqwest::Client` whose name resolution prefers system DNS but
/// falls back to Cloudflare DoH when the OS resolver fails or is blocked.
pub fn doh_http_client() -> Result<Client, String> {
    Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        .dns_resolver(Arc::new(HybridResolver::new()))
        .build()
        .map_err(|e| format!("Failed to build hybrid HTTP client: {}", e))
}

/// Returns an `Arc<dyn Resolve>` for callers that build their own
/// reqwest client (Tauri updater plugin via its `configure_client` hook).
pub fn doh_resolver() -> Arc<dyn Resolve> {
    Arc::new(HybridResolver::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doh_provider_prefers_hybrid_key_exchange() {
        let provider = hickory_resolver::net::tls::default_provider();
        assert_eq!(
            provider.kx_groups[0].name(),
            rustls::NamedGroup::X25519MLKEM768
        );
        assert!(provider
            .kx_groups
            .iter()
            .any(|group| group.name() == rustls::NamedGroup::X25519));
    }

    #[test]
    fn doh_tls_rejects_an_untrusted_server() {
        use rustls::pki_types::{PrivatePkcs8KeyDer, ServerName};
        use std::net::{TcpListener, TcpStream};

        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let server = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone()],
            PrivatePkcs8KeyDer::from(signing_key.serialize_der()).into(),
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut conn = rustls::ServerConnection::new(Arc::new(server)).unwrap();
            while conn.is_handshaking() && conn.complete_io(&mut socket).is_ok() {}
        });
        let config = hickory_resolver::net::tls::client_config().unwrap();
        let name = ServerName::try_from("localhost").unwrap().to_owned();
        let mut client = rustls::ClientConnection::new(Arc::new(config), name).unwrap();
        let mut socket = TcpStream::connect(address).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut result = Ok((0, 0));
        while client.is_handshaking() && result.is_ok() {
            result = client.complete_io(&mut socket);
        }
        let error = result.expect_err("an untrusted certificate must not be accepted");
        assert!(matches!(
            error
                .get_ref()
                .and_then(|e| e.downcast_ref::<rustls::Error>()),
            Some(rustls::Error::InvalidCertificate(_))
        ));
        drop(socket);
        worker.join().unwrap();
    }

    #[tokio::test]
    #[ignore = "requires network access to Cloudflare DNS"]
    async fn cloudflare_doh_resolves_with_certificate_validation() {
        let addresses = HybridResolver::new()
            .lookup_doh("example.com")
            .await
            .unwrap();
        assert!(!addresses.is_empty());
    }
}
