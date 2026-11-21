# DMNE Meme Alpha (Open-Core)

Deterministic Solana on-chain microstructure gating, Hawkes point-process wash-trading detection, funder-graph sybil filtering, volatility-scaled exit arbitration, and Jito execution engine by **DMNE Labs**.

## Official Links

- **DMNE Labs:** https://dmnelabs.com
- **DMNE Meme:** https://dmne-meme.com
- **DMNE Quant:** https://dmne-quant.com
- **DMNE Institutional:** https://dmne-institutional.com
- **Commercial & Enterprise Licensing:** `licensing@dmnelabs.com`

## Quick Start

1. Copy the example configuration and environment templates:
   ```bash
   cp config/config.example.toml config/config.toml
   cp .env.example .env
   ```
2. Calibrate thresholds in `config/config.toml` on your own out-of-sample data and configure your RPC endpoints in `.env`.
3. Run the unit test suite and build the release binary:
   ```bash
   cargo test --all
   cargo build --release
   ```

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE).
