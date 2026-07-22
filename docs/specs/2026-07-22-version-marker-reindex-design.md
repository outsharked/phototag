# phototag — version marker & reindex — design

## Purpose

Track which photos `phototag` has already indexed, and with which version of
`phototag-watch`, so that:

- Already-tagged photos are never re-processed on ordinary watch/backfill
  runs (avoiding redundant LLM calls).
- A photo tagged by an older version of `phototag-watch` can be deliberately
  re-tagged later, once tagging quality has improved — but only when
  explicitly requested, never automatically.

## Non-goals

- Not a general "tag arbitrary file/folder on demand" CLI. That's a
  separate, later piece of work — see "Explicitly out of scope" below.
- Not tracking a separate version for `phototag-server`'s prompt/model. Only
  `phototag-watch`'s own Cargo package version is tracked.
- Not a migration concern. Nothing has been deployed against real photos
  yet, so there's no existing on-disk state to migrate.

## Marker format & detection

A sentinel keyword, `phototag:v{version}` (e.g. `phototag:v0.1.0`), is added
to the same `IPTC:Keywords`/`XMP-dc:Subject` list that holds the LLM-derived
content keywords — no separate XMP namespace or exiftool config file needed.
`{version}` is `phototag-watch`'s own Cargo package version
(`CARGO_PKG_VERSION`), compared using the `semver` crate rather than naive
string comparison (so `0.9.0` correctly sorts before `0.10.0`).

`crates/watch/src/exif.rs` replaces its current boolean-only
`has_keywords(path) -> Result<bool>` with:

```rust
pub async fn read_keywords(path: &Path) -> Result<Vec<String>>
```

returning the actual current keyword list. A small pure helper — living in
`pipeline.rs`, not `exif.rs`, since it does no subprocess/I/O work and
`exif.rs` stays scoped strictly to wrapping `exiftool` calls — scans that
list for an entry matching the `phototag:v...` pattern and parses out its
version:

```rust
fn find_phototag_marker(keywords: &[String]) -> Option<semver::Version>
```

`write_keywords` is unchanged — still a full-list writer. All "don't delete
existing keywords" behavior lives in the pipeline, which computes the
complete desired list before calling it.

## Tag-check & reindex flow

`pipeline::tag_one_file` gains a new parameter:

```rust
pub async fn tag_one_file(
    path: &Path,
    client: &TaggerClient,
    reindex_outdated: bool,
) -> Result<TagOutcome>
```

Behavior, based on `read_keywords` and `find_phototag_marker`:

| Marker state | `reindex_outdated` | Action |
|---|---|---|
| No marker | (either) | First-time tagging. Call `phototag-server`, write back the *union* of existing keywords + new content keywords + marker (case-insensitive dedup — same convention as the reindex row below). Nothing already on the file is removed. |
| Marker, version ≥ current | (either) | `AlreadyTagged`, no LLM call. |
| Marker, version < current | `false` (default) | `AlreadyTagged`, no LLM call. An outdated marker alone never triggers work. |
| Marker, version < current | `true` | Reindex: call `phototag-server` for a fresh keyword list, add any not already present (case-insensitive dedup, matching `phototag-server`'s existing dedup convention), update the marker to the current version, write the full computed list. Existing keywords — including anything added by hand since the original tagging — are never removed. |

`TagOutcome` keeps its existing two variants (`Tagged(Vec<String>)` /
`AlreadyTagged`) — both first-time tagging and reindexing report as
`Tagged`, since the caller-facing effect (some keywords were written) is
the same in both cases.

## CLI surface

`--backfill` gains a new flag: `--reindex-outdated`. When set, backfill
passes `reindex_outdated: true` into `tag_one_file` for every file it
processes; unset (default) passes `false`.

This flag is only meaningful with `--backfill`. Continuous watch mode
always passes `false` — it only ever reacts to newly-created/modified
files, which by definition have no marker yet, so "outdated marker"
doesn't apply there.

Recap of the existing CLI surface this builds on: `phototag-watch` is one
binary; `main.rs` dispatches on `--backfill` to either `watcher::run_watch`
(default, continuous) or `backfill::run_backfill` (one-shot walk over
configured `[[roots]]`). Both bottom out in `pipeline::tag_one_file` per
file — this design only changes that shared function's signature and the
`--backfill` flag set, not the overall dispatch shape.

## Explicitly out of scope

A future "tag this specific file or folder on demand" CLI tool — one that
accepts an arbitrary path directly rather than only operating on configured
`[[roots]]`. Not designed here. The reason it's mentioned at all: because
`tag_one_file(path, client, reindex_outdated)` takes a plain path and has no
dependency on how that path was discovered, that future tool would be able
to call it directly, unchanged, once it exists.

## Testing

- `crates/watch/src/exif.rs`: existing tests built around the old
  `has_keywords` boolean get rewritten around `read_keywords`.
- `crates/watch/src/pipeline.rs`: new tests for all four branches in the
  table above, including one that specifically verifies a manually-added
  keyword (not written by phototag) survives a reindex.
- `crates/watch/src/backfill.rs`: new test exercising `--reindex-outdated`
  end-to-end — an outdated-marker file is left alone without the flag, and
  reindexed with it.
