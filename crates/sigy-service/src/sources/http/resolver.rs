use std::{io, net::SocketAddr, sync::Arc};

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use tokio::sync::Semaphore;

use crate::sources::NetworkScope;

#[derive(Debug)]
pub(super) struct CheckedResolver {
    scope: NetworkScope,
    slots: Arc<Semaphore>,
}

impl CheckedResolver {
    pub(super) const fn new(scope: NetworkScope, slots: Arc<Semaphore>) -> Self {
        Self { scope, slots }
    }
}

impl Resolve for CheckedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let scope = self.scope;
        let slots = self.slots.clone();
        Box::pin(async move {
            let addresses = if let NetworkScope::PinnedAddress { address } = scope {
                vec![SocketAddr::new(address, 0)]
            } else {
                let _permit = slots
                    .try_acquire_owned()
                    .map_err(|_| io::Error::other("DNS capacity reached"))?;
                // Use the configured system nameservers without a blocking OS
                // lookup or search-domain expansion. Dropping the future cancels it.
                let resolver = hickory_resolver::Resolver::builder_tokio()?.build()?;
                let hostname = format!("{}.", name.as_str().trim_end_matches('.'));
                resolver
                    .lookup_ip(hostname)
                    .await?
                    .iter()
                    .take(17)
                    .map(|address| SocketAddr::new(address, 0))
                    .collect()
            };
            validate_addresses(scope, &addresses)?;
            Ok(Box::new(addresses.into_iter()) as Addrs)
        })
    }
}

fn validate_addresses(scope: NetworkScope, addresses: &[SocketAddr]) -> io::Result<()> {
    if addresses.is_empty()
        || addresses.len() > 16
        || addresses.iter().any(|address| !scope.permits(address.ip()))
    {
        return Err(io::Error::other("destination denied"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_or_excessive_dns_answers_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
        let scope = NetworkScope::PublicInternet {};
        let public: SocketAddr = "8.8.8.8:0".parse()?;
        assert!(validate_addresses(scope, &[public]).is_ok());
        for denied in [
            "127.0.0.1:0",
            "10.1.2.3:0",
            "[::ffff:127.0.0.1]:0",
            "[fe80::1]:0",
        ] {
            assert!(validate_addresses(scope, &[public, denied.parse()?]).is_err());
        }
        assert!(validate_addresses(scope, &[]).is_err());
        assert!(validate_addresses(scope, &[public; 17]).is_err());
        Ok(())
    }
}
