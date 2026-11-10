# Zayden platform

The shared application layer of the Zayden Discord bot: command and event
routing, configuration and entitlements, graphics, and the bot runtime.
Serenity for Discord, Postgres via `sqlx`.

## Layout

| Path               | What it is                                                      |
| ------------------ | --------------------------------------------------------------- |
| `zayden-core/`     | command/event routing traits shared by every module             |
| `zayden-app/`      | application layer: config, entitlements, shared state           |
| `zayden-graphics/` | image rendering (`resvg`, `png`)                                |
| `bot-runtime/`     | shared bot runtime — boot, handler, registry, cron loop         |
| `.sqlx/`           | offline query cache; committed, regenerated on any query change |

The migrations live in the `schema` repo (`https://github.com/zayden-bots/schema`),
a git dependency declared in the root `Cargo.toml`. `Cargo.lock` pins the commit.
This repo depends on nothing else in the org. The `modules`, `bots` and
`dashboard` repos depend on it.

## Commands

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo fmt --all
```

`--all-targets` defaults to _off_ and silently skips tests, benches and
examples. Always pass it.

Clippy never codegens or links, so it reports green while the build is broken.
Only a real build exercises linking:

```bash
cargo build --workspace --all-targets
```

`bacon.toml` defines these as watch jobs:

```bash
bacon                # clippy, the default job
bacon test           # cargo test
bacon fmt            # cargo fmt --all --check
bacon build          # link check
bacon check-offline  # cargo check with SQLX_OFFLINE=true
bacon nextest        # needs `cargo install cargo-nextest`
```

Keep the flags identical across this README, `bacon.toml` and CI. A local
command weaker than CI is how drift starts.

## Conventions

- Lints are declared once in the root `Cargo.toml` under `[workspace.lints]`;
  every crate opts in with `[lints] workspace = true`.
- Dependency versions are pinned once in `[workspace.dependencies]`; member
  crates say `foo.workspace = true` and may _add_ features on top, but never
  restate a version. Unused workspace entries cost nothing — cargo only
  resolves a dependency once a member opts in.
- `unwrap`/`expect`/`panic`/`todo`/`dbg!` are **denied** outside tests;
  `clippy.toml` re-permits them under `#[cfg(test)]`.
- Tests live in `tests/`, not in inline `#[cfg(test)] mod tests` blocks.
- Commands are registered and routed manually through Serenity traits. `poise`
  is not used.
- `rustfmt.toml` uses unstable options, so `rust-toolchain.toml` pins nightly
  for local work. Never write `cargo +nightly` — the prefix is redundant.

## Database

Tests that need a database use `#[sqlx::test(migrator = "schema::MIGRATOR")]`.
The migrations are embedded in the `schema` crate, so tests need only
`DATABASE_URL`.

**Only ever run mutating `sqlx` commands against a throwaway database.**
Stand up a disposable server first:

```bash
docker run --rm -d --name zayden-prepare -p 55432:5432 -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB=zayden_prepare postgres:18-alpine
export DATABASE_URL="postgres://postgres:postgres@localhost:55432/zayden_prepare"
```

Apply the migrations from the sibling `schema` checkout:

```bash
sqlx migrate run --source ../schema/migrations
```

Or apply exactly what the lockfile pins (what CI does):

```bash
dir="$(cargo metadata --format-version 1 --locked \
  | jq -r '.packages[] | select(.name == "schema") | .manifest_path | rtrimstr("/Cargo.toml")')"
sqlx migrate run --source "$dir/migrations"
```

`.sqlx/` must be committed: CI sets `SQLX_OFFLINE=true` and never connects to a
database, so that cache is the only thing the query macros have to check
against. It holds only this repo's own queries. A git dependency's queries are
checked against the `.sqlx/` inside that dependency's checkout.

Regenerate it against an **empty, freshly-migrated** database — `LEFT JOIN`
nullability inference is plan-sensitive, and a cache built against a populated
DB will fail CI's `prepare --check`. The first command builds the git
dependencies offline so their queries do not leak into this repo's cache:

```bash
SQLX_OFFLINE=true cargo check --workspace --all-targets
cargo sqlx prepare --workspace -- --all-targets
```

If prepare fails, run `git restore .sqlx`.

## Taking a `schema` change

Push `schema` first, then here:

```bash
cargo update schema
```

Regenerate `.sqlx/` if queries changed, run the checks, commit `Cargo.lock`.
Never give two entries for the same org repo different `branch`/`tag`/`rev`
values: cargo would treat them as different crates and types would stop
matching.

For local work across repos without pushing, `../infra/scripts/dev-link.sh on`
points every org crate at the sibling checkouts; `off` removes the link and
`status` reports it. Never commit a `Cargo.lock` written while the link is on.

## Toolchain

Nightly for local development (`rust-toolchain.toml`), stable in CI.
`rust-toolchain.toml`, `rustfmt.toml`, `clippy.toml` and `.cargo/config.toml`
are owned by `infra/template/`; `../infra/scripts/sync-standards.sh` copies them
here and `--check` reports drift.

## CI

`.github/workflows/ci.yml` runs on pushes to `main`, `v*` tags, pull requests,
manual dispatch and a weekly schedule. Five jobs run in parallel:

| Job                 | What it runs                                                    |
| ------------------- | --------------------------------------------------------------- |
| `rustfmt`           | `cargo fmt --all --check` on nightly                            |
| `clippy`            | `cargo clippy --workspace --all-targets --locked -- -D warnings` |
| `test`              | `cargo test --workspace --all-targets --locked`, Postgres service |
| `deny`              | `cargo-deny check`                                              |
| `sqlx`              | applies the pinned `schema` migrations, then `cargo sqlx prepare --workspace --check` |

Apart from `rustfmt`, jobs build on **stable** (`RUSTUP_TOOLCHAIN=stable`
outranks `rust-toolchain.toml`, and `RUSTFLAGS=""` clears the nightly-only flags
in `.cargo/config.toml`).

`deny.toml` allows git sources from the orgs listed under `[sources.allow-org]`;
any other git source needs an `allow-git` entry.
