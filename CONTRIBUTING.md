# Contributing to Lanes

Thanks for your interest in improving Lanes! Contributions of all kinds are welcome — bug reports, fixes, features, and docs.

## Getting set up

See the **Development** section of the [README](./README.md) for the toolchain and how to run the hot-reloading dev server (`cargo leptos watch`).

## Before you open a PR

Please make sure the following pass locally:

```bash
cargo fmt --all
cargo clippy --no-default-features --features ssr -- -D warnings
cargo test --no-default-features --features ssr
```

- Match the surrounding code style and patterns.
- Keep changes focused; one logical change per PR.
- If you touch a `sqlx::query!` macro or add a migration, run `cargo sqlx prepare` and commit the updated `.sqlx/` cache.
- Update docs when behavior changes.

## Reporting bugs

Open an issue with steps to reproduce, what you expected, and what happened (logs/screenshots help). For security issues, follow [SECURITY.md](./SECURITY.md) instead of opening a public issue.

## Commit / PR conventions

- Write clear, imperative commit messages (e.g. "fix: prevent duplicate invite acceptance").
- Reference related issues where relevant.

By contributing, you agree that your contributions are licensed under the project's [MIT License](./LICENSE).