//! `_minecraft._tcp` SRV lookups, behind a trait so tests never touch DNS.

use std::future::Future;

use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::RData;
use tokio::sync::OnceCell;

/// One SRV record: where a service actually listens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SrvRecord {
    pub priority: u16,
    pub weight: u16,
    pub port: u16,
    /// The target host, with or without the trailing dot.
    pub target: String,
}

/// Looks up SRV records. An error or an empty list means "no record".
pub trait SrvResolver: Send + Sync {
    fn lookup_srv(&self, name: &str)
    -> impl Future<Output = Result<Vec<SrvRecord>, String>> + Send;
}

/// The operating system's DNS configuration, built on first use. A failed
/// build is not remembered, so the next lookup tries again (for example once
/// the network is back).
#[derive(Default)]
pub struct SystemSrvResolver {
    resolver: OnceCell<TokioResolver>,
}

impl SystemSrvResolver {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SrvResolver for SystemSrvResolver {
    async fn lookup_srv(&self, name: &str) -> Result<Vec<SrvRecord>, String> {
        let resolver = self
            .resolver
            .get_or_try_init(|| async {
                TokioResolver::builder_tokio()
                    .and_then(|builder| builder.build())
                    .map_err(|error| {
                        tracing::warn!(%error, "no system DNS resolver");
                        format!("no system DNS resolver: {error}")
                    })
            })
            .await?;
        let lookup = resolver
            .srv_lookup(name)
            .await
            .map_err(|error| error.to_string())?;
        Ok(lookup
            .answers()
            .iter()
            .filter_map(|record| match &record.data {
                RData::SRV(srv) => Some(SrvRecord {
                    priority: srv.priority,
                    weight: srv.weight,
                    port: srv.port,
                    target: srv.target.to_utf8(),
                }),
                _ => None,
            })
            .collect())
    }
}

/// The record to use: the lowest priority, then the highest weight. Records
/// whose target is empty or "." mean the service is not offered there.
pub(crate) fn best_target(records: &[SrvRecord]) -> Option<(String, u16)> {
    records
        .iter()
        .filter(|record| !record.target.trim_end_matches('.').is_empty() && record.port != 0)
        .min_by_key(|record| (record.priority, std::cmp::Reverse(record.weight)))
        .map(|record| (record.target.trim_end_matches('.').to_owned(), record.port))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(priority: u16, weight: u16, port: u16, target: &str) -> SrvRecord {
        SrvRecord {
            priority,
            weight,
            port,
            target: target.to_owned(),
        }
    }

    #[test]
    fn picks_the_cinepol_record_without_the_trailing_dot() {
        let records = [record(0, 5, 25565, "mc.policarpiers.com.")];
        assert_eq!(
            best_target(&records),
            Some(("mc.policarpiers.com".to_owned(), 25565))
        );
    }

    #[test]
    fn prefers_low_priority_then_high_weight() {
        let records = [
            record(10, 100, 1, "c."),
            record(0, 1, 2, "b."),
            record(0, 9, 3, "a."),
        ];
        assert_eq!(best_target(&records), Some(("a".to_owned(), 3)));
    }

    #[test]
    fn ignores_records_that_disable_the_service() {
        assert_eq!(best_target(&[record(0, 0, 25565, ".")]), None);
        assert_eq!(best_target(&[]), None);
    }
}
