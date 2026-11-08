# Contributing

Thanks for helping. Bug reports, fixes and small focused features are all welcome.

## Setup

```bash
brew install xcodegen
cargo install bore-cli   # optional: lets the integration tests run against a real relay
xcodegen generate
open Localfly.xcodeproj
```

Run the core tests:

```bash
swift test --package-path LocalflyCore
```

## Guidelines

- The tunnel engine lives in `LocalflyCore` and has no UI dependencies. Behaviour changes there need a test.
- The app in `App/` stays a thin layer over `TunnelManager`.
- Keep the app working with stock `bore server` 0.6 and with bore.pub.
- For anything large, open an issue first so we can agree on the approach.

By contributing, you agree that your contributions are licensed under the MIT License.
