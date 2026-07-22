# image-tagger — design

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
  LLM gateway (`reveillm`) and assumes a vision-capable model is available
  there.
- Not a generic media watcher — scoped to a single photo library path and a
  small set of image formats exiftool can reliably write keywords to.

## Architecture

Two components, one Cargo workspace, mirroring `find-anything`'s
`crates/`-per-binary layout:

```
outsharked/image-tagger (public GitHub repo)
├── crates/
│   ├── common/   — shared API types (tag request/response), config structs
│   ├── server/   — the `image-tagger` HTTP service binary
│   └── watch/    — the `phototag-watch` binary (event watcher + backfill)
├── Dockerfile    — multi-stage build for crates/server, pushed to the
│                   private registry for docker-lxc deployment
└── docs/superpowers/specs/ — this file
```

Real deployment topology (host IPs, the `reveillm` URL, the actual watch
path, systemd units, docker-compose stack) lives only in the private
`homelab` repo — same split already used for `reveillm`, `mediawatcher`, and
`find-watch`. This repo's own committed config is a placeholder-only example
(mirroring `reveillm`'s `config.example.yaml` and `find-anything`'s
`client.toml` template), so the public repo never encodes real network
topology.

### `image-tagger` server (`crates/server`)

- `axum` + `tokio` HTTP service.
- `POST /tag` — accepts raw image bytes, forwards to `reveillm`'s
  OpenAI-compatible endpoint (`.../v1/chat/completions`) as a multimodal
  chat request with a vision-capable model, asking for a short list of
  concise object/scene keywords. Parses the model's response into a clean
  keyword list and returns it as JSON. Malformed/unparseable model output
  is a `4xx`/`5xx` response, not a crash.
- No filesystem access, no state, no database — purely a stateless
  image-in/keywords-out API. This is what lets it run on docker-lxc without
  any NAS mount.
- Deployed as a Docker image (multi-stage Rust build) pushed to the private
  registry, run as a new stack on docker-lxc alongside `reveillm`.

### `phototag-watch` (`crates/watch`)

- Runs on synology1 (DS218j, 512MB RAM, ARMv7, no Docker) as a compiled
  binary — matches `find-watch`'s existing precedent on this exact host,
  chosen over a Python/Docker approach specifically because of the weak
  hardware and this host's documented history of inotify-queue-overflow
  problems under load.
- Uses the `notify` crate to watch a single configured photo library path
  (not the whole `/volume1/data` share `find-watch` covers).
- Reacts on file-close-write events for a configured extension allow-list
  (`.jpg`, `.jpeg`, `.png`, `.tiff`, `.heic` by default — the formats
  exiftool reliably writes IPTC/XMP keywords to), with a short debounce.
- Before tagging, checks the file's existing `IPTC:Keywords` via exiftool;
  skips if already present (idempotent — a stray re-trigger or a re-run
  after a crash is a no-op, no separate progress database needed).
- POSTs the file to the configured `image-tagger` server, receives the
  keyword list, then shells out to `exiftool` to write
  `IPTC:Keywords`/`XMP-dc:Subject` in place (no `_original` backup files —
  a one-directional metadata add on files that aren't otherwise being
  edited).
- Same binary also supports a `--backfill` mode: walks the configured path
  once, applying the same tag-and-write logic to every file that doesn't
  yet have keywords, instead of watching. This covers the initial catch-up
  pass over the existing library, and is also the manual recovery path if
  `image-tagger`/`reveillm`/MUSIC3 was unreachable during normal watching
  (no automatic retry loop — matches the manual-rescan philosophy already
  established by `mediawatcher` on synology2).
- Config file (`phototag-watch.toml`, mirrors `find-anything`'s
  `client.toml` shape): watch path, `image-tagger` URL, extension
  allow-list, debounce interval.

## Data flow

```
new/changed image under watched path
        │  (notify, debounced, extension-filtered)
        ▼
phototag-watch: has IPTC:Keywords already? ──yes──▶ skip
        │ no
        ▼
POST image bytes ──▶ image-tagger (docker-lxc)
        │                   │
        │                   ▼
        │           reveillm (WoL-wakes MUSIC3 if asleep)
        │                   │
        │                   ▼
        │           Ollama qwen3-vl:30b, multimodal chat completion
        │                   │
        │           parse response → keyword list
        ◀───────────────────┘
        ▼
exiftool: write IPTC:Keywords + XMP-dc:Subject in place
```

## Error handling

- `phototag-watch` → `image-tagger` request fails, times out, or
  `image-tagger` → `reveillm` fails (including a MUSIC3 wake timeout): log
  and move on to the next file. No automatic retry. Recovery is a manual
  `--backfill` re-run once the underlying issue is fixed.
- Unparseable/empty model output: log and skip that file — never write a
  garbage or empty keyword list.
- `phototag-watch` crash or restart mid-run: safe by construction, since the
  "already has keywords" check means any file is either fully tagged or
  untouched — no partial-write state to recover.

## Testing

- `crates/server`: integration tests in `crates/server/tests/` using a
  `TestServer`-style harness (matching `find-anything`'s convention),
  hitting `POST /tag` against a mocked `reveillm` endpoint.
- `crates/watch`: end-to-end test invoking the compiled binary against a
  temp directory and a mock `image-tagger` server, covering both watch mode
  and `--backfill` mode.
- Manual validation before enabling the systemd unit: run `--backfill`
  once against a small real subfolder, inspect the written keywords with
  `exiftool`, confirm Immich (once set up as an external library) surfaces
  them.

## Open items for the implementation plan

- Exact prompt text sent to the vision model for keyword extraction.
- Whether `crates/common` needs a shared HTTP client wrapper or whether
  `crates/watch` and `crates/server`'s test harness can each use `reqwest`
  directly.
- Confirm `exiftool` (`perl-image-exiftool` via Entware) is installable on
  synology1's DSM version before relying on it.
