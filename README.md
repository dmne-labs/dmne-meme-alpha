# DMNE Meme Alpha (Open-Core)

Deterministic Solana on-chain microstructure gating, Hawkes point-process
wash-trading detection, funder-graph sybil filtering, volatility-scaled
exit arbitration, and signed Jupiter V6 / Jito execution engine by
**DMNE Labs**.

## Official Links

- **DMNE Labs:** https://dmnelabs.com
- **DMNE Meme:** https://dmne-meme.com
- **Research Brief 002 (On-Chain Execution Drag):** https://dmnelabs.com/research/brief-002.html
- **Commercial & Enterprise Licensing:** `licensing@dmnelabs.com`

## Open-Core Scope & Calibration

This public release (`v2.2.9`) provides the deterministic Rust execution
engine—including signed Ed25519 `VersionedTransaction` swaps, concurrent
Jito bundle and RPC dispatch, fail-closed signature confirmation, atomic
SPL token balance verification, microstructure guards, and forensic
telemetry pipeline—configured with conservative baseline thresholds.
Proprietary parameter calibrations, empirical tick-level datasets, and
private signal ingestion pipelines remain exclusive to DMNE Labs'
production deployments and licensed partner environments.

## Quick Start

1. Copy the example configuration and environment templates:
   ```bash
   cp config/config.example.toml config/config.toml
   cp .env.example .env
   ```
2. Calibrate thresholds in `config/config.toml` on your own
   out-of-sample data and configure your RPC endpoints in `.env`.
3. Run the unit test suite and build the release binary:
   ```bash
   cargo test --all
   cargo build --release
   ```

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE).
