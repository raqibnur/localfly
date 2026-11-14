# Contributing

Thanks for helping. Bug reports, fixes and small focused features are all welcome.

## Setup

```bash
cargo install bore-cli   # optional: lets the integration tests run against a real relay
cargo test
cargo build
```

## Guidelines

- The tunnel engine lives in `src/` (`bore.rs`, `client.rs`, `manager.rs`) with no UI toolkit dependencies. Behaviour changes there need a test.
- The CLI in `src/main.rs` stays a thin layer over `TunnelManager`.
- Keep the client working with stock `bore server` 0.6 and with bore.pub.
- For anything large, open an issue first so we can agree on the approach.

By contributing, you agree that your contributions are licensed under the MIT License.
