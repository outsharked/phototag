# phototag — design

## Purpose

Automatically add descriptive keyword metadata to photos using a local vision
LLM, so photos become searchable/filterable by content (objects, scenes)
without any cloud service. Keywords are written directly into each image's
own `IPTC:Keywords` / `XMP-dc:Subject` metadata fields, so any downstream
tool (Immich, a file browser, etc.) picks them up for free — no dependency on
a specific photo-management app.

## Non-goals

- Not a photo management app (no albums, no UI, no database of its own).
- Not responsible for choosing/hosting the LLM — it calls an existing local
  OpenAI-compatible LLM gateway and assumes a vision-capable model is
  available through it.
- Not a generic media watcher — scoped to a configured set of photo library
  root paths (one today, but the config supports multiple from the start)
  and a small set of image formats exiftool can reliably write keywords to.

## Architecture

Two components, one Cargo workspace, mirroring `find-anything`'s
`crates/`-per-binary layout:

```
outsharked/phototag (public GitHub repo)
├── crates/
│   ├── common/   — shared API types (tag request/response), config structs
│   ├── server/   — the `phototag-server` HTTP service binary
│   └── watch/    — the `phototag-watch` binary (event watcher + backfill)
├── Dockerfile    — multi-stage build for crates/server
├── docs/specs/   — design specs (this file)
└── docs/plans/   — implementation plans
```

Deployment config (systemd units, container-orchestration files, actual
host addresses and paths) lives in a separate private ops repo, not here.
Any config file committed in this repo — e.g. `phototag-watch.toml` below —
is a generic example with placeholder values, the same convention
`find-anything`'s `client.toml` template uses.

### `phototag-server` (`crates/server`)

- `axum` + `tokio` HTTP service.
- `POST /tag` — accepts raw image bytes, forwards them to a configured
  OpenAI-compatible chat-completions endpoint (`.../v1/chat/completions`)
  as a multimodal chat request with a vision-capable model, asking for a
  short list of concise object/scene keywords. Parses the model's response
  into a clean keyword list and returns it as JSON. Malformed/unparseable
  model output is a `4xx`/`5xx` response, not a crash.
- No filesystem access, no state, no database — purely a stateless
  image-in/keywords-out API. This is what lets it run without any NAS
  mount, on whatever host is convenient for reaching the LLM gateway.
- Deployed as a Docker image (multi-stage Rust build).

### `phototag-watch` (`crates/watch`)

- Runs on the NAS hosting the photo library — a resource-constrained ARM
  device (512MB RAM, no Docker) — as a compiled binary rather than a
  Python/Docker approach, following the precedent of `find-anything`'s
  `find-watch` client, which already runs as a lean compiled binary on this
  class of hardware for the same reason (low idle footprint, no GC, and
  this device's own history of inotify-queue-overflow problems under load
  when the watcher isn't lightweight).
- Uses the `notify` crate to watch a configured list of photo library root
  paths. One root today, but the config and watcher are list-based from the
  start since adding a second library root later should be a config
  change, not a rewrite.
- Reacts on file-close-write events for a configured extension allow-list
  (`.jpg`, `.jpeg`, `.png`, `.tiff`, `.heic` by default — the formats
  exiftool reliably writes IPTC/XMP keywords to), with a short debounce.
- Before tagging, checks the file's existing `IPTC:Keywords` via exiftool;
  skips if already present (idempotent — a stray re-trigger or a re-run
  after a crash is a no-op, no separate progress database needed).
- POSTs the file to the configured `phototag-server`, receives the
  keyword list, then shells out to `exiftool` to write
  `IPTC:Keywords`/`XMP-dc:Subject` in place (no `_original` backup files —
  a one-directional metadata add on files that aren't otherwise being
  edited).
- Same binary also supports a `--backfill` mode: walks each configured root
  once (optionally restricted to a single named root via a flag), applying
  the same tag-and-write logic to every file that doesn't yet have
  keywords, instead of watching. This covers the initial catch-up pass over
  the existing library, and is also the manual recovery path if the
  `phototag-server` service or the LLM gateway behind it was unreachable
  during normal watching (deliberately no automatic retry loop — a
  one-shot manual re-run is simpler to reason about than a background
  retry policy).
- Config file (`phototag-watch.toml`, mirrors `find-anything`'s
  `client.toml` `[[sources]]` shape): a `[[roots]]` array (each with a
  `name` and `path`), plus global `phototag-server` URL, extension allow-list,
  and debounce interval. Example:

  ```toml
  server_url = "http://phototag-server:8080"

  [[roots]]
  name = "pictures"
  path = "/path/to/pictures"

  # [[roots]]
  # name = "second-library"
  # path = "/path/to/other/photos"

  [watch]
  extensions = ["jpg", "jpeg", "png", "tiff", "heic"]
  debounce_ms = 2000
  ```

## Data flow

```
new/changed image under any watched root
        │  (notify, debounced, extension-filtered)
        ▼
phototag-watch: has IPTC:Keywords already? ──yes──▶ skip
        │ no
        ▼
POST image bytes ──▶ phototag-server
        │                   │
        │                   ▼
        │           LLM gateway (may wake a sleeping
        │           GPU host before forwarding)
        │                   │
        │                   ▼
        │           vision-capable model, multimodal chat completion
        │                   │
        │           parse response → keyword list
        ◀───────────────────┘
        ▼
exiftool: write IPTC:Keywords + XMP-dc:Subject in place
```

## Error handling

- `phototag-watch` → `phototag-server` request fails, times out, or
  `phototag-server` → the LLM gateway fails (including a slow-to-wake GPU
  host): log and move on to the next file. No automatic retry. Recovery is
  a manual `--backfill` re-run once the underlying issue is fixed.
- Unparseable/empty model output: log and skip that file — never write a
  garbage or empty keyword list.
- `phototag-watch` crash or restart mid-run: safe by construction, since the
  "already has keywords" check means any file is either fully tagged or
  untouched — no partial-write state to recover.

## Testing

- `crates/server`: integration tests in `crates/server/tests/` using a
  `TestServer`-style harness (matching `find-anything`'s convention),
  hitting `POST /tag` against a mocked LLM-gateway endpoint.
- `crates/watch`: end-to-end test invoking the compiled binary against a
  temp directory and a mock `phototag-server`, covering both watch mode
  and `--backfill` mode, including a config with multiple `[[roots]]`
  pointing at separate temp directories.
- Manual validation before enabling the systemd unit: run `--backfill`
  once against a small real subfolder, inspect the written keywords with
  `exiftool`, confirm Immich (once set up as an external library) surfaces
  them.

## Open items for the implementation plan

- Exact prompt text sent to the vision model for keyword extraction.
- Whether `crates/common` needs a shared HTTP client wrapper or whether
  `crates/watch` and `crates/server`'s test harness can each use `reqwest`
  directly.
- Confirm `exiftool` (e.g. `perl-image-exiftool` via Entware) is installable
  on the target NAS's OS/package manager before relying on it.
