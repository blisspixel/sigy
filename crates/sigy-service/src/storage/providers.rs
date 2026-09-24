//! Immutable provider routes and dated price snapshots. No credential value is stored.

use super::{Store, validate_key};
use crate::{
    Error, Result,
    providers::{LanguagePair, PriceDraft, ProviderKind, Rates, RouteDraft, Task},
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

#[cfg(test)]
pub(crate) mod attempts;
#[cfg(test)]
mod tests;

pub(crate) const MAX_PAGE: u32 = 64;
const MAX_ROUTE_PRICES: u32 = 16;
const PRICE_COLUMNS: &str = "id, route_id, retrieved_ms, valid_until_ms, prompt, completion, request, internal_reasoning, input_cache_read, input_cache_write, image, audio, web_search, unrecognized, source_note, created_ms";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProviderRoute {
    pub(crate) route: RouteDraft,
    pub(crate) created_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PriceRecord {
    pub(crate) price: PriceDraft,
    pub(crate) created_ms: i64,
}

impl Store {
    /// Store one immutable route. An identical replay is unchanged.
    /// # Errors
    /// Refuses a changed route under an existing ID, the route limit, or catalog errors.
    pub(crate) fn add_provider_route(
        &mut self,
        route: &RouteDraft,
        now_ms: i64,
    ) -> Result<(ProviderRoute, bool)> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) = read_route(&tx, &route.id)? {
            if existing.route != *route {
                return Err(Error::IdempotencyConflict);
            }
            return Ok((existing, false));
        }
        let count: i64 =
            tx.query_row("SELECT count(*) FROM provider_routes", [], |row| row.get(0))?;
        if count >= 256 {
            return Err(Error::InvalidInput("provider route count"));
        }
        tx.execute(
            "INSERT INTO provider_routes(id, provider, endpoint_origin, model, upstreams_json, task, secret_env, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                route.id,
                route.kind.as_str(),
                route.origin,
                route.model,
                serde_json::to_string(&route.upstreams)?,
                route.task.as_str(),
                route.secret_env,
                now_ms
            ],
        )?;
        for pair in &route.pairs {
            tx.execute(
                "INSERT INTO provider_route_pairs(route_id, source_language, target_language) VALUES (?1, ?2, ?3)",
                params![route.id, pair.source, pair.target],
            )?;
        }
        tx.commit()?;
        Ok((
            ProviderRoute {
                route: route.clone(),
                created_ms: now_ms,
            },
            true,
        ))
    }

    /// # Errors
    /// Refuses a malformed ID or an invalid stored row.
    pub(crate) fn provider_route(&self, id: &str) -> Result<Option<ProviderRoute>> {
        validate_key(id, "provider route ID")?;
        read_route(&self.connection, id)
    }

    /// Routes ordered by ID after an optional cursor.
    /// # Errors
    /// Refuses a malformed cursor, an out-of-range limit, or invalid stored rows.
    pub(crate) fn provider_routes(
        &self,
        after: Option<&str>,
        limit: u32,
    ) -> Result<Vec<ProviderRoute>> {
        if !(1..=MAX_PAGE).contains(&limit) {
            return Err(Error::InvalidInput("page limit"));
        }
        if let Some(after) = after {
            validate_key(after, "provider route cursor")?;
        }
        let mut query = self.connection.prepare(
            "SELECT id FROM provider_routes WHERE ?1 IS NULL OR id > ?1 ORDER BY id LIMIT ?2",
        )?;
        let ids = query
            .query_map(params![after, limit], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ids.iter()
            .map(|id| read_route(&self.connection, id)?.ok_or(Error::StorageIntegrity))
            .collect()
    }

    /// Store one immutable price snapshot for a hosted route. An identical replay is unchanged.
    /// # Errors
    /// Refuses an unknown or local route, a changed snapshot under an existing ID, or catalog errors.
    pub(crate) fn add_price_snapshot(
        &mut self,
        price: &PriceDraft,
        now_ms: i64,
    ) -> Result<(PriceRecord, bool)> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) = read_price(&tx, &price.id)? {
            if existing.price != *price {
                return Err(Error::IdempotencyConflict);
            }
            return Ok((existing, false));
        }
        let route = read_route(&tx, &price.route_id)?.ok_or(Error::NotFound)?;
        if route.route.kind != ProviderKind::OpenRouter {
            return Err(Error::InvalidInput(
                "only a hosted route has a price snapshot",
            ));
        }
        let count: i64 = tx.query_row(
            "SELECT count(*) FROM provider_price_snapshots WHERE route_id = ?1",
            [&price.route_id],
            |row| row.get(0),
        )?;
        if count >= 64 {
            return Err(Error::InvalidInput("price snapshot count"));
        }
        let rates = price.rates.values().map(|rate| rate.to_string());
        let [
            prompt,
            completion,
            request,
            reasoning,
            cache_read,
            cache_write,
            image,
            audio,
            search,
            unknown,
        ] = &rates;
        tx.execute(
            &format!("INSERT INTO provider_price_snapshots({PRICE_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)"),
            params![
                price.id, price.route_id, price.retrieved_ms, price.valid_until_ms,
                prompt, completion, request, reasoning, cache_read, cache_write,
                image, audio, search, unknown, price.note, now_ms
            ],
        )?;
        tx.commit()?;
        Ok((
            PriceRecord {
                price: price.clone(),
                created_ms: now_ms,
            },
            true,
        ))
    }

    /// # Errors
    /// Refuses a malformed ID or an invalid stored row.
    pub(crate) fn price_snapshot(&self, id: &str) -> Result<Option<PriceRecord>> {
        validate_key(id, "price snapshot ID")?;
        read_price(&self.connection, id)
    }

    /// The newest snapshots of one route, at most 16.
    /// # Errors
    /// Refuses invalid stored rows.
    pub(crate) fn route_price_snapshots(&self, route_id: &str) -> Result<Vec<PriceRecord>> {
        let mut query = self.connection.prepare(
            "SELECT id FROM provider_price_snapshots WHERE route_id = ?1 ORDER BY retrieved_ms DESC, id LIMIT ?2",
        )?;
        let ids = query
            .query_map(params![route_id, MAX_ROUTE_PRICES], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ids.iter()
            .map(|id| read_price(&self.connection, id)?.ok_or(Error::StorageIntegrity))
            .collect()
    }

    /// Checks route pairs and the order of attempt outcomes and ledger transitions.
    /// # Errors
    /// Fails closed on any inconsistency.
    pub(crate) fn audit_providers(&self) -> Result<()> {
        let inconsistent: i64 = self.connection.query_row(
            "SELECT (SELECT count(*) FROM provider_routes r WHERE NOT EXISTS
                (SELECT 1 FROM provider_route_pairs p WHERE p.route_id = r.id))
             + (SELECT count(*) FROM provider_attempts a JOIN requests q ON q.id = a.request_id
                WHERE (q.state = 'settled' AND a.outcome IS NULL)
                   OR (q.state = 'released' AND a.outcome IS NOT NULL)
                   OR (q.state = 'settled' AND q.evidence IS NOT a.generation_id))
             + (SELECT count(*) FROM provider_attempts a JOIN provider_price_snapshots s ON s.id = a.snapshot_id
                WHERE s.route_id != a.route_id)",
            [],
            |row| row.get(0),
        )?;
        if inconsistent != 0 {
            return Err(Error::LedgerIntegrity);
        }
        Ok(())
    }
}

fn read_route(connection: &Connection, id: &str) -> Result<Option<ProviderRoute>> {
    let row = connection
        .query_row(
            "SELECT provider, endpoint_origin, model, upstreams_json, task, secret_env, allow_fallbacks, created_ms FROM provider_routes WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, bool>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            },
        )
        .optional()?;
    let Some((kind, origin, model, upstreams, task, secret_env, fallbacks, created_ms)) = row
    else {
        return Ok(None);
    };
    if fallbacks {
        return Err(Error::StorageIntegrity);
    }
    let mut query = connection.prepare(
        "SELECT source_language, target_language, validation FROM provider_route_pairs WHERE route_id = ?1 ORDER BY source_language, target_language LIMIT 17",
    )?;
    let pairs = query
        .query_map([id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if pairs.is_empty()
        || pairs.len() > 16
        || pairs.iter().any(|(_, _, state)| state != "unvalidated")
    {
        return Err(Error::StorageIntegrity);
    }
    let route = RouteDraft {
        id: id.to_owned(),
        kind: ProviderKind::parse(&kind).map_err(|_| Error::StorageIntegrity)?,
        origin,
        model,
        upstreams: serde_json::from_str(&upstreams).map_err(|_| Error::StorageIntegrity)?,
        task: Task::parse(&task).map_err(|_| Error::StorageIntegrity)?,
        secret_env,
        pairs: pairs
            .into_iter()
            .map(|(source, target, _)| LanguagePair { source, target })
            .collect(),
    };
    Ok(Some(ProviderRoute { route, created_ms }))
}

fn read_price(connection: &Connection, id: &str) -> Result<Option<PriceRecord>> {
    let row = connection
        .query_row(
            &format!("SELECT {PRICE_COLUMNS} FROM provider_price_snapshots WHERE id = ?1"),
            [id],
            |row| {
                let mut rates = Vec::with_capacity(Rates::NAMES.len());
                for (offset, name) in Rates::NAMES.iter().enumerate() {
                    rates.push(((*name).to_owned(), row.get::<_, String>(4 + offset)?));
                }
                Ok((
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    rates,
                    row.get::<_, String>(14)?,
                    row.get::<_, i64>(15)?,
                ))
            },
        )
        .optional()?;
    let Some((route_id, retrieved_ms, valid_until_ms, rates, note, created_ms)) = row else {
        return Ok(None);
    };
    let parsed = Rates::from_named(&rates).map_err(|_| Error::StorageIntegrity)?;
    // Stored text must be the canonical exact decimal of the rate it holds.
    if parsed
        .values()
        .iter()
        .zip(&rates)
        .any(|(rate, (_, text))| rate.to_string() != *text)
    {
        return Err(Error::StorageIntegrity);
    }
    Ok(Some(PriceRecord {
        price: PriceDraft {
            id: id.to_owned(),
            route_id,
            retrieved_ms,
            valid_until_ms,
            rates: parsed,
            note,
        },
        created_ms,
    }))
}
