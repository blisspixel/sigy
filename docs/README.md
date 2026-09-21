# Design documents

Last updated: 2026-09-21

Read [Intent](../INTENT.md) first, followed by the [Roadmap](../ROADMAP.md).

Implementation has begun. Read [active work and evidence](development/progress.md) and the [Rust foundation decision](decisions/0001-rust-foundation.md) for current state. The planning chapters retain the broader product contract; they are not a list of shipped features.

Implemented boundaries are recorded in the [local controller](decisions/0002-local-controller.md), [capture journal](decisions/0003-capture-journal.md) and [source authority and HTTP transport](decisions/0004-source-authority-and-http.md) decisions. Focused implementation targets cover the [terminal experience](design/terminal-experience.md), [language coverage and localization](design/languages.md), and [universal signal interpretation](design/signal-interpretation.md).

The [planning index](planning/README.md) records confirmed requirements and the status of the design. Detailed documents cover:

1. [Product and experience](planning/01-product-and-experience.md)
2. [Architecture and data](planning/02-architecture-and-data.md)
3. [Research synthesis](planning/03-source-and-model-research.md)
4. [Assurance and validation](planning/04-assurance-and-validation.md)
5. [Delivery and decisions](planning/05-delivery-and-decisions.md)
6. [Language and stack trade study](planning/06-language-and-stack-trade-study.md)
7. [Providers and cost policy](planning/07-providers-and-cost-policy.md)
8. [Signal extensions and workbench](planning/08-signal-extensions-and-workbench.md)
9. [Security, privacy, and release design](planning/09-security-privacy-and-release.md)
10. [Analysis, knowledge, and multi-stream processing](planning/10-analysis-and-knowledge.md)
11. [Complete CLI, terminal explorer, and radio DVR](planning/11-radio-explorer-and-dvr.md)
12. [Repository organization and engineering](planning/12-repository-and-engineering.md)

[Research](../research/README.md) contains the dated evidence, alternatives, and unresolved experiments for individual topics. Specifications describe proposed behavior; research explains the evidence behind it. Neither implies an implemented capability.

[AGENTS.md](../AGENTS.md) is the canonical concise development guidance. [Naming research](../research/21-naming.md) records the open product-name discussion without changing the working name.
