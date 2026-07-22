# image-tagger Development Guide

## Project Overview

Local-LLM image keyword tagging. Two Rust binaries in one Cargo workspace:

- **`image-tagger`** (`crates/server`) — stateless HTTP service. `POST /tag`
  takes image bytes, forwards them to a local vision LLM via an
  OpenAI-compatible gateway, and returns a parsed keyword list. No
  filesystem access, no database.
- **`phototag-watch`** (`crates/watch`) — filesystem watcher + backfill tool.
  Watches a configured photo library path, and for new/changed images with
  no existing `IPTC:Keywords`, POSTs them to `image-tagger`, then writes the
  returned keywords into the file's own `IPTC:Keywords`/`XMP-dc:Subject`
  metadata via `exiftool`. Also runs in `--backfill` mode: a one-shot walk
  of the whole path instead of watching, used for the initial catch-up pass
  and as the manual recovery path after an outage (no automatic retry).

Keywords are written directly into each image's own metadata, so any
downstream tool (photo library, file browser, search index) picks them up
without depending on this project.

- **Language:** Rust
- **Async runtime:** `tokio`
- **HTTP server:** `axum` (`crates/server`)
- **HTTP client:** `reqwest` (`crates/watch` → `crates/server`)
- **Filesystem watching:** `notify` (`crates/watch`)
- **Config format:** TOML (`serde` + `toml`)
- **Logging:** `tracing` + `tracing-subscriber`

## Workspace Layout

```
crates/
├── common/   — shared API types (tag request/response), config structs
├── server/   — `image-tagger` binary
└── watch/    — `phototag-watch` binary (watch mode + --backfill)
```

## Deployment

This repo holds source only — no real network topology (host IPs, gateway
URLs, actual watch paths) is committed here. Real deployment config lives in
the private `homelab` repo:

- `image-tagger` is built as a Docker image and deployed as a docker-compose
  stack on the docker LXC host.
- `phototag-watch` is cross-compiled for `armv7-unknown-linux-gnueabihf` and
  deployed to the NAS as a systemd service, alongside `find-watch`
  (`find-anything`'s watcher, same host).

Any config file committed to this repo is a placeholder-only example (real
values never checked in), matching the convention used by `reveillm` and
`find-anything`.

## Documentation

- Design specs: `docs/specs/`
- Implementation plans: `docs/plans/`

Specs and plans are stored directly under `docs/specs/` and `docs/plans/`,
**not** in a `superpowers/` (or other tool-specific) subfolder — this
overrides any skill or tool default that suggests otherwise.
