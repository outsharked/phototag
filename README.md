# phototag

Local-LLM image keyword tagging. Two binaries:

- **`phototag-server`** — stateless HTTP service. `POST /tag` with raw image
  bytes, get back `{"keywords": [...]}`. Calls a configured
  OpenAI-compatible vision model; writes nothing to disk itself.
- **`phototag-watch`** — watches configured photo library root paths (or,
  with `--backfill`, walks them once) and for any image without existing
  `IPTC:Keywords`, calls `phototag-server` and writes the returned keywords
  into the file's own `IPTC:Keywords`/`XMP-dc:Subject` metadata via
  `exiftool`.

See `AGENTS.md` for architecture and conventions, and
`docs/specs/2026-07-21-phototag-design.md` for the full design.

## Building

```bash
cargo build --release
```

`phototag-watch` also needs `exiftool` on `PATH` at runtime.

## Running `phototag-server`

```bash
PHOTOTAG_GATEWAY_URL=http://your-llm-gateway:8080/v1 \
PHOTOTAG_GATEWAY_MODEL=your-vision-model \
cargo run -p phototag-server
```

## Running `phototag-watch`

Copy `phototag-watch.example.toml`, fill in real paths and your
`phototag-server` URL, then:

```bash
cargo run -p phototag-watch -- --config your-config.toml           # watch mode
cargo run -p phototag-watch -- --config your-config.toml --backfill # one-shot catch-up
```
