# Contributing to Xmas.JS 🎄

## What you need

- A Rust toolchain. `rustup toolchain install stable` is enough; nothing here is nightly-only.
- The engine, checked out next to this repository:

  ```sh
  git clone https://github.com/0xfe-project/quickjs-oxide ../quickjs-oxide
  git clone https://github.com/lemonhx/xmas.js Xmas.JS
  ```

  `runtime/Cargo.toml` points at `../../quickjs-oxide`, so the two have to be siblings.

No C toolchain, no cmake, no pkg-config: every dependency is pure Rust, and `tests/purity.rs`
fails the build if that ever stops being true.

## Before you open a pull request

```sh
cargo test --workspace
./scripts/check-targets.sh
```

The second one checks the workspace for macOS, linux-gnu and windows-msvc. `cargo check` does
not link, so it needs no cross linker and no SDK, and it is the cheapest way to catch a change
that compiles only on the machine it was written on.

## Sending it

1. Fork the repository on GitHub and create a branch for your change.
2. Commit in English, with a message that says why the change is there.
3. Open a pull request against `master`.
4. Engage in code review and address the feedback.
