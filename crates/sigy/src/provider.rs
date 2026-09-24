use std::io::{self, Write};

use clap::{Args, Subcommand};
use sigy_service::control::{
    Operation, PriceSnapshotView, PriceSpec, ProviderOperation, ProviderPage, ProviderRouteView,
    RateSpec, RouteSpec,
};

use crate::explorer::text::sanitize;

const FIELD_CHARS: usize = 256;

#[derive(Debug, Subcommand)]
pub enum ProviderCommand {
    /// Store and inspect provider routes. Nothing is sent to a provider.
    Route {
        #[command(subcommand)]
        command: RouteCommand,
    },
    /// Store and inspect dated price snapshots. Nothing is fetched.
    Price {
        #[command(subcommand)]
        command: PriceCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum RouteCommand {
    /// Store one immutable route. Fallback is always off; pairs start unvalidated.
    Add {
        /// Unique route ID. Reuse cannot change it.
        id: String,
        /// Provider kind: openrouter or ollama.
        #[arg(long)]
        provider: String,
        /// Endpoint origin only, for example `https://openrouter.ai`.
        #[arg(long)]
        origin: String,
        #[arg(long)]
        model: String,
        /// Permitted upstream provider slug. Repeat for each; required for openrouter.
        #[arg(long = "upstream")]
        upstreams: Vec<String>,
        #[arg(long, default_value = "translate-text")]
        task: String,
        /// Name of the environment variable that will hold the key. The value is never stored.
        #[arg(long)]
        secret_env: Option<String>,
        /// Authorized `source:target` language pair, for example `es:en`. Repeat for each.
        #[arg(long = "pair", required = true)]
        pairs: Vec<String>,
    },
    /// List routes. Secrets are shown by variable name only.
    List {
        #[arg(long)]
        after: Option<String>,
        #[arg(long, default_value_t = 16)]
        limit: u32,
    },
    /// Show one route and its newest price snapshots.
    Show { id: String },
}

#[derive(Debug, Subcommand)]
pub enum PriceCommand {
    /// Store one immutable price snapshot in exact USD per unit.
    Add(Box<PriceAdd>),
    /// Show one price snapshot and whether it is fresh now.
    Show { id: String },
}

#[derive(Debug, Args)]
pub struct PriceAdd {
    /// Unique snapshot ID. Reuse cannot change it.
    id: String,
    #[arg(long)]
    route: String,
    /// RFC 3339 instant the prices were read, for example `2026-09-24T12:00:00Z`.
    #[arg(long)]
    retrieved: String,
    /// Hours the snapshot stays usable, 1 to 720.
    #[arg(long, default_value_t = 24)]
    valid_hours: u32,
    /// USD per prompt token.
    #[arg(long)]
    prompt: String,
    /// USD per completion token.
    #[arg(long)]
    completion: String,
    /// USD per request.
    #[arg(long)]
    request: Option<String>,
    /// USD per internal reasoning token.
    #[arg(long)]
    reasoning: Option<String>,
    /// USD per cached prompt token read.
    #[arg(long)]
    cache_read: Option<String>,
    /// USD per cached prompt token written.
    #[arg(long)]
    cache_write: Option<String>,
    /// USD per image. A nonzero value makes the route ineligible.
    #[arg(long)]
    image: Option<String>,
    /// USD per audio unit. A nonzero value makes the route ineligible.
    #[arg(long)]
    audio: Option<String>,
    /// USD per web search. A nonzero value makes the route ineligible.
    #[arg(long)]
    web_search: Option<String>,
    /// Largest listed charge with an unknown unit. A nonzero value makes the route ineligible.
    #[arg(long)]
    unrecognized: Option<String>,
    /// Where the prices came from, in printable ASCII.
    #[arg(long)]
    note: String,
}

impl ProviderCommand {
    pub fn operation(&self) -> Operation {
        let command = match self {
            Self::Route { command } => match command {
                RouteCommand::Add {
                    id,
                    provider,
                    origin,
                    model,
                    upstreams,
                    task,
                    secret_env,
                    pairs,
                } => ProviderOperation::AddRoute {
                    route: RouteSpec {
                        id: id.clone(),
                        provider: provider.clone(),
                        endpoint_origin: origin.clone(),
                        model: model.clone(),
                        upstream_providers: upstreams.clone(),
                        task: task.clone(),
                        secret_env: secret_env.clone(),
                        language_pairs: pairs.clone(),
                    },
                },
                RouteCommand::List { after, limit } => ProviderOperation::ListRoutes {
                    after: after.clone(),
                    limit: *limit,
                },
                RouteCommand::Show { id } => ProviderOperation::ShowRoute { id: id.clone() },
            },
            Self::Price { command } => match command {
                PriceCommand::Add(add) => ProviderOperation::AddPrice { price: add.spec() },
                PriceCommand::Show { id } => ProviderOperation::ShowPrice { id: id.clone() },
            },
        };
        Operation::Provider { command }
    }
}

impl PriceAdd {
    fn spec(&self) -> PriceSpec {
        let optional = [
            ("request", &self.request),
            ("internal_reasoning", &self.reasoning),
            ("input_cache_read", &self.cache_read),
            ("input_cache_write", &self.cache_write),
            ("image", &self.image),
            ("audio", &self.audio),
            ("web_search", &self.web_search),
            ("unrecognized", &self.unrecognized),
        ];
        let rates = [("prompt", &self.prompt), ("completion", &self.completion)]
            .into_iter()
            .map(|(dimension, usd)| (dimension, Some(usd)))
            .chain(
                optional
                    .into_iter()
                    .map(|(dimension, usd)| (dimension, usd.as_ref())),
            )
            .filter_map(|(dimension, usd)| {
                usd.map(|usd| RateSpec {
                    dimension: dimension.into(),
                    usd: usd.clone(),
                })
            })
            .collect();
        PriceSpec {
            id: self.id.clone(),
            route_id: self.route.clone(),
            retrieved: self.retrieved.clone(),
            valid_hours: self.valid_hours,
            rates,
            source_note: self.note.clone(),
        }
    }
}

fn clean(value: &str) -> String {
    sanitize(value, FIELD_CHARS)
}

pub fn render(writer: &mut impl Write, page: &ProviderPage) -> io::Result<()> {
    for route in &page.routes {
        render_route(writer, route)?;
    }
    for price in &page.prices {
        render_price(writer, price)?;
    }
    if page.routes.is_empty() && page.prices.is_empty() {
        writeln!(writer, "No provider routes.")?;
    }
    if let Some(created) = page.newly_created {
        writeln!(
            writer,
            "{} Provider dispatch is not available.",
            if created { "Stored." } else { "Unchanged." }
        )?;
    }
    if let Some(after) = &page.next_after {
        writeln!(writer, "Next page after {}", clean(after))?;
    }
    Ok(())
}

fn render_route(writer: &mut impl Write, route: &ProviderRouteView) -> io::Result<()> {
    writeln!(
        writer,
        "Route {}: {} {} model {} task {}",
        clean(&route.id),
        clean(&route.provider),
        clean(&route.endpoint_origin),
        clean(&route.model),
        clean(&route.task)
    )?;
    let upstreams = if route.upstream_providers.is_empty() {
        "none".to_owned()
    } else {
        clean(&route.upstream_providers.join(", "))
    };
    writeln!(
        writer,
        "  Upstream providers: {upstreams}. Fallback: {}.",
        if route.allow_fallbacks { "on" } else { "off" }
    )?;
    match &route.secret_env {
        Some(name) => writeln!(
            writer,
            "  Secret: environment variable {} (name only; no value is stored).",
            clean(name)
        )?,
        None => writeln!(writer, "  Secret: none.")?,
    }
    let pairs: Vec<String> = route
        .language_pairs
        .iter()
        .map(|pair| {
            format!(
                "{}:{} {}",
                clean(&pair.source),
                clean(&pair.target),
                clean(&pair.validation)
            )
        })
        .collect();
    writeln!(writer, "  Language pairs: {}.", pairs.join(", "))?;
    writeln!(
        writer,
        "  Dispatch: {}.",
        if route.dispatch_available {
            "available"
        } else {
            "unavailable"
        }
    )
}

fn render_price(writer: &mut impl Write, price: &PriceSnapshotView) -> io::Result<()> {
    writeln!(
        writer,
        "Price {} for route {}: retrieved_ms {} valid_until_ms {} ({})",
        clean(&price.id),
        clean(&price.route_id),
        price.retrieved_ms,
        price.valid_until_ms,
        if price.fresh { "fresh" } else { "stale" }
    )?;
    let rates: Vec<String> = price
        .rates
        .iter()
        .map(|rate| format!("{} {}", clean(&rate.dimension), clean(&rate.usd)))
        .collect();
    writeln!(writer, "  USD per unit: {}.", rates.join(", "))?;
    writeln!(writer, "  Source: {}", clean(&price.source_note))
}
