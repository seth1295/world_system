# VEYRA World System

Bootstrap repository for the VEYRA world-system project. Development occurs through reviewed pull requests.

The canonical product and architecture context is [DOC/VEYRA_WORLD_SYSTEM_ARCHITECTURE.md](DOC/VEYRA_WORLD_SYSTEM_ARCHITECTURE.md).

## Build and verify

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --target wasm32-unknown-unknown -p veyra-core -p veyra-wasm
cargo run -p veyra-cli -- conformance verify
cargo run -p veyra-cli -- --help
```
