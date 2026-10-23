# 0111: One Atom feed document

Date: 2026-10-23

Status: implemented parse of one Atom document on the existing podcast refresh. Catalog schema stays v53. Local IPC stays v56. Issue 13 stays open. A news command, article text, and a stage exit are not claimed.

## Decision

`podcast refresh` already reads one RSS 2.0 document. An Atom document failed before this increment, both as a content type and as a root element. This increment accepts one feed in the namespace `http://www.w3.org/2005/Atom` through the same subscribe, refresh, snapshot, and episode rows.

The parser stays the existing hand parser. Depth, entities, size, and expansion limits are unchanged. RSS 2.0 and this Atom root are the only successes. An `entry` becomes an episode only when its parent is that feed. An entry inside an RSS channel, and an RSS item inside an Atom feed, are ignored. The feed-level `id` is not an episode. An entry `id` is a publisher guid. The 2,048-character guid limit still fails the document.

`published` fills the existing publication time when it parses. `updated` fills that time only when `published` is absent. Both are the RFC 3339 subset `YYYY-MM-DDTHH:MM:SS` with `Z` or `±HH:MM`, ASCII case ignored for `T` and `Z`, and an optional fraction whose first three digits are milliseconds. A bad value leaves the publication time empty and does not fail a document that still has an id. A date alone, a missing zone, a leap second, and a named zone do not parse.

A direct child `link` of an entry is an enclosure only when `rel` is exactly `enclosure` or exactly `http://www.iana.org/assignments/relation/enclosure`. A missing `rel` means alternate and is not an enclosure. `href` is resolved against the fetched document, never `xml:base`. The first enclosure wins. `title` text is stored, including text nested inside the title, and is not rendered as HTML. `summary`, `content`, `author`, `category`, and `rights` are ignored. Links inside them are not enclosures and are not requested. An Atom `id` that is an HTTP URI is identity text. It is not fetched.

Podcasting 2.0 `transcript`, `chapters`, and `liveItem` keep their existing meaning on an Atom entry. They are still not fetched. An entry with no id and no enclosure-plus-parsed-time is skipped. The first copy of an id wins. Entry 501 marks the snapshot truncated. An empty feed commits zero episodes and is success. Atom 0.3 and an un-namespaced `feed` fail.

`application/atom+xml` is an accepted feed type, with `application/rss+xml`, `application/xml`, and `text/xml`. Any of those four types may carry either RSS 2.0 or Atom. The parser decides. A missing type, two types, `text/html`, `application/xhtml+xml`, `application/json`, and a charset other than `utf-8` or `us-ascii` are rejected. The acquisition error is `expected an RSS or Atom document`. The parser error is `feed document is not RSS 2.0 or Atom`.

No catalog migration and no IPC message change. Episodes stay `publisher_guid` or `derived_enclosure`. `published` and `updated` collapse into `published_ms`. Summary and content are not stored. This amends [Decision 0018](0018-rss-feed-refresh.md). It does not exit operation 11 again and does not exit stage 9.

## What this does not do

This slice does not add `sigy news`, OPML, a news table, or an MCP news tool. It does not scrape, read a sitemap, or fetch `robots.txt`. It does not store summary or content, and it does not keep `published` and `updated` as separate columns. It does not poll automatically, resume a range, or send a conditional GET. It does not translate feed text, place a feed on the globe, or match a monitor term to an article. It does not add a score. It does not close issue 13 or issue 14. It does not close S-03B. It does not exit a roadmap stage.

## Interface

`podcast subscribe` and `podcast refresh` stay the commands. Help text says one RSS 2.0 or Atom document. Refresh still does not download an enclosure, a transcript, a chapter, a summary, or a content link. `podcast download` remains the only enclosure fetch, and only for a URL already stored on an episode.

## Evidence

`cargo verify` exited 0 in 423.33 seconds. The `sigy` binary tests passed 384, with 7 ignored, in 27.02 seconds. The `sigy-service` library tests passed 770, with 2 ignored, in 183.76 seconds. `cargo audit --deny warnings` loaded 1351 advisories and scanned 382 dependencies. `cargo verify-media` exited 0 in 140.26 seconds: 20 ignored service fixtures in 115.56 seconds and the direct live PCM check in 0.07 seconds. FFmpeg is `N-113513-gaa3cfd4b5a-20240202`. Coverage and `cargo visual-qa` were not run. External spend is USD 0. This receipt was written after those commands.

## Limits

Issue 13 phase 1 stays unchecked. Scraping, sitemaps, robots, conditional GET, item revisions beyond the existing episode row, language evidence, globe markers, and cross-source findings stay later. Issue 14 stays open. S-03B stays open. Process exit before a completion hold commits can still drop that outcome. No acoustic, Omarchy, language, or platform qualification is claimed. External spend is USD 0. This slice changes HTTP acquisition, so `cargo verify` and `cargo verify-media` are both required. Fixtures contact only 127.0.0.1.
