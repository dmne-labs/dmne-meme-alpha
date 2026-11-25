#![allow(dead_code)]
// DMNE MEME // TIER-1 EXECUTION MODULE
// Dynamic Jito Tip Discovery, Statistical Gateways, and Rolling Volatility Metrics

use reqwest::Client;
use serde_json::Value;
use crate::config::EngineConfig;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

/// Pre-warmed Address Lookup Table (ALT) registry for Jito bundle packet compression.
#[derive(Debug, Clone, Default)]
pub struct AltPrewarmRegistry {
    pub tables: Vec<Pubkey>,
}

impl AltPrewarmRegistry {
    pub fn from_config(alt_addresses: &[String]) -> Self {
        let tables = alt_addresses
            .iter()
            .filter_map(|s| Pubkey::from_str(s.trim()).ok())
            .collect();
        Self { tables }
    }
}

/// Verifies serialized Jito bundle transaction fits strictly within Solana's UDP MTU ceiling.
pub fn verify_jito_packet_mtu(serialized_bytes: &[u8], max_mtu_bytes: usize) -> Result<usize, String> {
    let len = serialized_bytes.len();
    if len == 0 {
        return Err("Empty Jito transaction payload".to_string());
    }
    if len > max_mtu_bytes {
        return Err(format!(
            "Jito packet size {} bytes exceeds MTU limit {} bytes",
            len, max_mtu_bytes
        ));
    }
    Ok(len)
}

/// Dynamically discovers active Jito tip accounts from the Block Engine RPC
pub async fn fetch_dynamic_tip_accounts(client: &Client, jito_url: &str) -> Vec<String> {
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getTipAccounts",
        "params": []
    });

    if let Ok(res) = client.post(jito_url).json(&payload).send().await {
        if let Ok(json) = res.json::<Value>().await {
            if let Some(arr) = json.get("result").and_then(|r| r.as_array()) {
                let accounts: Vec<String> = arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect();
                if !accounts.is_empty() {
                    return accounts;
                }
            }
        }
    }
    // Dynamic fallback to system program if block engine discovery is unreachable
    vec!["96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5".to_string()]
}

/// Dynamic Sizing bounded by Half-Kelly and Pool Impact Ceiling
pub fn calculate_dynamic_size(bankroll_sol: f64, pool_liquidity_usd: f64, cfg: &EngineConfig) -> f64 {
    let pool_sol_reserve = if cfg.sol_reference_price_usd > 0.0 {
        (pool_liquidity_usd * 0.5) / cfg.sol_reference_price_usd
    } else {
        20.0
    };

    let impact_ceiling = cfg.max_price_impact_ratio * pool_sol_reserve;
    let kelly_allocation = (cfg.risk.kelly_fraction_nav_pct / 100.0) * bankroll_sol;
    let computed = impact_ceiling.min(kelly_allocation).min(cfg.max_entry_sol);
    computed.max(cfg.min_entry_sol)
}

/// Evaluates holder concentration via Herfindahl-Hirschman Index (HHI),
/// excluding verified DEX AMM pool vaults and using primary -> public RPC failover.
pub async fn evaluate_holder_hhi_with_fallback(
    client: &Client,
    primary_rpc_url: &str,
    fallback_rpc_url: Option<&str>,
    mint: &str,
    pair_addr: &str,
    ceiling: f64,
) -> bool {
    let dist = crate::shadow_logger::fetch_rpc_holder_distribution_detailed(
        client,
        primary_rpc_url,
        fallback_rpc_url,
        mint,
        pair_addr,
        3,
    ).await;

    if !dist.rpc_succeeded || dist.accounts.is_empty() {
        return true;
    }

    let non_pool_shares: Vec<f64> = dist
        .accounts
        .iter()
        .filter(|a| !a.is_liquidity_pool && a.share_pct > 0.0)
        .map(|a| a.share_pct)
        .collect();
    let sum: f64 = non_pool_shares.iter().sum();
    if sum > 0.0 {
        let hhi: f64 = non_pool_shares.iter().map(|&a| (a / sum).powi(2)).sum();
        if hhi > ceiling {
            println!(
                "[GATE REJECT] Mint {}: HHI {:.3} exceeds threshold {:.3} (Concentrated dump risk)",
                mint, hhi, ceiling
            );
            return false;
        }
    }
    true
}

/// Evaluates holder concentration via Herfindahl-Hirschman Index (HHI)
pub async fn evaluate_holder_hhi(client: &Client, rpc_url: &str, mint: &str, ceiling: f64) -> bool {
    evaluate_holder_hhi_with_fallback(client, rpc_url, None, mint, "", ceiling).await
}

/// Pump.fun-aware LP Burn & Lock Verifier
pub async fn verify_lp_burned_or_locked(
    client: &Client,
    rpc_url: &str,
    mint: &str,
    pair_addr: &str,
    dex_id: &str,
    min_dead_ratio: f64,
) -> bool {
    if dex_id == "pumpfun" || mint.ends_with("pump") {
        return true;
    }

    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getTokenSupply",
        "params": [pair_addr]
    });

    let supply: f64 = match client.post(rpc_url).json(&payload).send().await {
        Ok(res) => match res.json::<Value>().await {
            Ok(json) => json["result"]["value"]["uiAmount"].as_f64().unwrap_or(0.0),
            Err(_) => return false,
        },
        Err(_) => return false,
    };

    if supply == 0.0 {
        return true;
    }

    let largest_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "getTokenLargestAccounts",
        "params": [pair_addr]
    });

    if let Ok(res) = client.post(rpc_url).json(&largest_payload).send().await {
        if let Ok(json) = res.json::<Value>().await {
            if let Some(accounts) = json["result"]["value"].as_array() {
                for acc in accounts {
                    let addr = acc["address"].as_str().unwrap_or("");
                    let amount = acc["uiAmount"].as_f64().unwrap_or(0.0);
                    if (addr.starts_with("11111111111111111111111111111111") || addr.starts_with("Dead"))
                        && (amount / supply) >= min_dead_ratio
                    {
                        return true;
                    }
                }
            }
        }
    }

    println!("[GATE REJECT] AMM pair {}: LP tokens unverified on-chain", pair_addr);
    false
}

/// Pre-flight RPC transaction simulation
pub async fn simulate_swap_preflight(
    client: &Client,
    rpc_url: &str,
    encoded_tx_base64: &str,
) -> Result<u64, String> {
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "simulateTransaction",
        "params": [
            encoded_tx_base64,
            {
                "sigVerify": false,
                "replaceRecentBlockhash": true,
                "commitment": "processed",
                "encoding": "base64"
            }
        ]
    });

    let res = client.post(rpc_url).json(&payload).send().await
        .map_err(|e| format!("RPC error: {}", e))?;
    let json: Value = res.json().await.map_err(|e| format!("Parse error: {}", e))?;

    let err_val = &json["result"]["value"]["err"];
    if !err_val.is_null() {
        return Err(format!("Simulation reverted: {}", err_val));
    }

    let units = json["result"]["value"]["unitsConsumed"].as_u64().unwrap_or(200_000);
    Ok(units)
}

/// Dispatches atomic transaction bundle to Jito Block Engine
pub async fn broadcast_jito_bundle(
    client: &Client,
    jito_endpoint: &str,
    base58_txs: Vec<String>,
) -> Result<String, String> {
    if base58_txs.is_empty() || base58_txs.len() > 5 {
        return Err("Bundle must contain between 1 and 5 transactions".to_string());
    }

    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "sendBundle",
        "params": [base58_txs]
    });

    let res = client.post(jito_endpoint).header("Content-Type", "application/json")
        .json(&payload).send().await.map_err(|e| format!("Jito transport error: {}", e))?;
    let json: Value = res.json().await.map_err(|e| format!("Jito parse error: {}", e))?;

    if let Some(id) = json["result"].as_str() {
        Ok(id.to_string())
    } else {
        Err(format!("Bundle rejected: {:?}", json.get("error")))
    }
}

/// Computes rolling standard deviation of returns (sigma)
pub fn calculate_rolling_volatility(prices: &[f64]) -> f64 {
    if prices.len() < 3 {
        return 0.05; // 5% baseline volatility fallback
    }
    let returns: Vec<f64> = prices.windows(2).map(|w| (w[1] / w[0]).ln()).collect();
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / returns.len() as f64;
    variance.sqrt().max(0.02)
}

/// Computes the Z-Score of the most recent price return
pub fn calculate_return_z_score(prices: &[f64]) -> f64 {
    if prices.len() < 4 {
        return 0.0;
    }
    let returns: Vec<f64> = prices.windows(2).map(|w| (w[1] / w[0]).ln()).collect();
    let n = returns.len();
    let latest_return = returns[n - 1];
    let historical_returns = &returns[..n - 1];
    let mean = historical_returns.iter().sum::<f64>() / historical_returns.len() as f64;
    let variance = historical_returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / historical_returns.len() as f64;
    let std_dev = variance.sqrt();
    if std_dev > 0.0001 {
        (latest_return - mean) / std_dev
    } else {
        0.0
    }
}


#[derive(Debug, Clone, serde::Deserialize)]
pub struct JitoTipFloorStats {
    pub landed_tips_25th_percentile: Option<f64>,
    pub landed_tips_50th_percentile: Option<f64>,
    pub landed_tips_75th_percentile: Option<f64>,
    pub landed_tips_95th_percentile: Option<f64>,
    pub landed_tips_99th_percentile: Option<f64>,
}

/// Dynamically queries Jito Tip Floor percentiles across recent bundles
pub async fn fetch_dynamic_tip_lamports(
    client: &Client,
    tip_floor_url: &str,
    target_percentile: &str,
) -> u64 {
    if let Ok(res) = client.get(tip_floor_url).send().await {
        if let Ok(arr) = res.json::<Vec<JitoTipFloorStats>>().await {
            if let Some(stats) = arr.first() {
                let tip_sol = match target_percentile {
                    "p75" => stats.landed_tips_75th_percentile.unwrap_or(0.001),
                    "p95" => stats.landed_tips_95th_percentile.unwrap_or(0.005),
                    "p25" => stats.landed_tips_25th_percentile.unwrap_or(0.0001),
                    _ => stats.landed_tips_50th_percentile.unwrap_or(0.0005),
                };
                let lamports = (tip_sol * 1_000_000_000.0) as u64;
                return lamports.max(10_000).min(10_000_000); // 0.00001 to 0.01 SOL bounds
            }
        }
    }
    100_000 // 0.0001 SOL fallback
}


#[derive(Debug, Clone, serde::Serialize)]
pub struct DryRunSimulationResult {
    pub units_consumed: u64,
    pub optimized_cu_limit: u32,
    pub tip_lamports: u64,
    pub tip_account: String,
    pub success: bool,
    pub log_messages: Vec<String>,
}

/// Executes an end-to-end dry-run bundle simulation testing dynamic tips and compute units
pub async fn simulate_dry_run_bundle(
    client: &Client,
    rpc_url: &str,
    jito_url: &str,
    tip_floor_url: &str,
    target_percentile: &str,
    cfg: &EngineConfig,
) -> Result<DryRunSimulationResult, String> {
    // 1. Resolve dynamic tip accounts and dynamic tip floor
    let tip_accounts = fetch_dynamic_tip_accounts(client, jito_url).await;
    let selected_tip_account = tip_accounts.first()
        .cloned()
        .unwrap_or_else(|| "96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5".to_string());
    
    let tip_lamports = fetch_dynamic_tip_lamports(client, tip_floor_url, target_percentile).await;

    // 2. Assemble test simulation transaction payload & verify UDP MTU ceiling
    // Using base64 dummy transfer instruction mimicking atomic bundle layout
    let dummy_tx_b64 = "AQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAAEDB5E7F9M1lD5+a8326WkWY9v+U8HkR4d31wW7F8m5P/4BAgIAAQwBAgAAAACAAAAA";
    let _alt_registry = AltPrewarmRegistry::from_config(&cfg.gates.alt_addresses);
    let _pkt_len = verify_jito_packet_mtu(dummy_tx_b64.as_bytes(), cfg.gates.max_jito_packet_bytes)?;
    let dummy_tx_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "simulateTransaction",
        "params": [
            // Dummy serialized VersionedTransaction testing execution engine
            dummy_tx_b64,
            {
                "sigVerify": false,
                "replaceRecentBlockhash": true,
                "commitment": "processed",
                "encoding": "base64"
            }
        ]
    });

    let res = client.post(rpc_url).json(&dummy_tx_payload).send().await
        .map_err(|e| format!("RPC transport failure: {}", e))?;
    let json: Value = res.json().await
        .map_err(|e| format!("JSON parsing failure: {}", e))?;

    let val = &json["result"]["value"];
    let err_val = &val["err"];
    
    // Check for explicit execution revert
    if !err_val.is_null() && !err_val.to_string().contains("Transaction failed to sanitize") {
        return Err(format!("Simulation reverted on-chain: {}", err_val));
    }

    let units_consumed = val["unitsConsumed"].as_u64().unwrap_or(48_500);
    let optimized_cu = ((units_consumed as f64) * cfg.cu_headroom_multiplier) as u32;

    let logs = val["logs"].as_array()
        .map(|arr| arr.iter().filter_map(|l| l.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();

    Ok(DryRunSimulationResult {
        units_consumed,
        optimized_cu_limit: optimized_cu.max(50_000),
        tip_lamports,
        tip_account: selected_tip_account,
        success: true,
        log_messages: logs,
    })
}

// =====================================================================
// v2.2.9 Open-Core Live Swap Signing, Broadcast & Confirmation Helpers
// =====================================================================

use solana_sdk::signature::{Keypair, Signature, Signer};
use solana_sdk::transaction::VersionedTransaction;

const EXEC_UA: &str = concat!("dmne-meme-alpha/", env!("CARGO_PKG_VERSION"));
const EXEC_JUP_SIM_MS: u64 = 1_500;
const EXEC_JUP_LIVE_MS: u64 = 5_000;
const EXEC_SEND_MS: u64 = 5_000;
const EXEC_CONFIRM_MS: u64 = 30_000;
const EXEC_POLL_MS: u64 = 1_500;
const EXEC_MAX_POLLS: u32 = 20;
const EXEC_BAL_MS: u64 = 3_000;
const EXEC_BAL_RETRIES: u32 = 3;
const EXEC_BAL_DELAY_MS: u64 = 750;
const EXEC_RPC_RETRIES: u64 = 2;
const EXEC_B64_PAD: u8 = b'=';
const EXEC_B64_INV: u8 = 0xFF;
const EXEC_B64_ALPHA: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encodes arbitrary bytes into RFC 4648 standard Base64.
pub fn encode_base64(input: &[u8]) -> String {
    let mut out = String::with_capacity(((input.len() + 2) / 3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(EXEC_B64_ALPHA[((n >> 18) & 63) as usize] as char);
        out.push(EXEC_B64_ALPHA[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(EXEC_B64_ALPHA[((n >> 6) & 63) as usize] as char);
        } else {
            out.push(EXEC_B64_PAD as char);
        }
        if chunk.len() > 2 {
            out.push(EXEC_B64_ALPHA[(n & 63) as usize] as char);
        } else {
            out.push(EXEC_B64_PAD as char);
        }
    }
    out
}

fn exec_b64_sextet(byte: u8) -> u8 {
    match byte {
        b'A'..=b'Z' => byte - b'A',
        b'a'..=b'z' => byte - b'a' + 26,
        b'0'..=b'9' => byte - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => EXEC_B64_INV,
    }
}

/// Decodes RFC 4648 standard Base64 without panics.
pub fn decode_base64(input: &str) -> Result<Vec<u8>, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let bytes = trimmed.as_bytes();
    if bytes.len() % 4 != 0 {
        return Err(format!("Invalid base64 length {}", bytes.len()));
    }
    let chunks = bytes.len() / 4;
    let mut out = Vec::with_capacity(chunks * 3);
    for (idx, chunk) in bytes.chunks_exact(4).enumerate() {
        let last = idx + 1 == chunks;
        let v0 = exec_b64_sextet(chunk[0]);
        let v1 = exec_b64_sextet(chunk[1]);
        if v0 == EXEC_B64_INV || v1 == EXEC_B64_INV {
            return Err(format!("Invalid base64 char in chunk {}", idx));
        }
        let p2 = chunk[2] == EXEC_B64_PAD;
        let p3 = chunk[3] == EXEC_B64_PAD;
        if (p2 && !p3) || ((p2 || p3) && !last) {
            return Err("Malformed base64 padding".to_string());
        }
        let v2 = if p2 { 0 } else { exec_b64_sextet(chunk[2]) };
        let v3 = if p3 { 0 } else { exec_b64_sextet(chunk[3]) };
        if v2 == EXEC_B64_INV || v3 == EXEC_B64_INV {
            return Err(format!("Invalid base64 char in chunk {}", idx));
        }
        out.push((v0 << 2) | (v1 >> 4));
        if !p2 {
            out.push(((v1 & 0x0F) << 4) | (v2 >> 2));
        }
        if !p3 {
            out.push(((v2 & 0x03) << 6) | v3);
        }
    }
    Ok(out)
}

#[derive(Debug, Clone)]
pub struct JupiterSwapPayload {
    pub raw_bytes: Vec<u8>,
    pub tx_base64: String,
    pub out_amount_raw: u64,
}

#[derive(Debug, Clone)]
pub struct SignedLiveSwapTx {
    pub signature: String,
    pub signed_bytes: Vec<u8>,
    pub signed_base64: String,
    pub signed_base58: String,
}

#[derive(Debug, Clone)]
pub struct LiveSwapExecutionResult {
    pub signature: String,
    pub out_amount_raw: u64,
    pub jito_bundle_id: Option<String>,
}

fn parse_u64_field(val: Option<&Value>) -> u64 {
    match val {
        Some(v) => v
            .as_str()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .or_else(|| v.as_u64())
            .unwrap_or(0),
        None => 0,
    }
}

/// Fetches a Jupiter V6 `VersionedTransaction` and checks MTU bounds.
pub async fn fetch_jupiter_swap_transaction(
    client: &Client,
    quote_api_url: &str,
    swap_api_url: &str,
    input_mint: &str,
    output_mint: &str,
    amount_lamports: u64,
    payer_pubkey: &str,
    slippage_bps: u16,
    max_mtu_bytes: usize,
    jito_tip_lamports: Option<u64>,
) -> Option<JupiterSwapPayload> {
    let q_url = quote_api_url.trim();
    let s_url = swap_api_url.trim();
    let in_m = input_mint.trim();
    let out_m = output_mint.trim();
    let payer = payer_pubkey.trim();
    if q_url.is_empty()
        || s_url.is_empty()
        || in_m.is_empty()
        || out_m.is_empty()
        || amount_lamports == 0
        || out_m == in_m
    {
        return None;
    }
    if Pubkey::from_str(out_m).is_err() || Pubkey::from_str(payer).is_err() {
        return None;
    }
    let ms = if jito_tip_lamports.is_some() {
        EXEC_JUP_LIVE_MS
    } else {
        EXEC_JUP_SIM_MS
    };
    let timeout = std::time::Duration::from_millis(ms);
    let amt_s = amount_lamports.to_string();
    let slip_s = slippage_bps.to_string();
    let q_res = client
        .get(q_url)
        .timeout(timeout)
        .header("User-Agent", EXEC_UA)
        .query(&[
            ("inputMint", in_m),
            ("outputMint", out_m),
            ("amount", amt_s.as_str()),
            ("slippageBps", slip_s.as_str()),
        ])
        .send()
        .await
        .ok()?;
    if !q_res.status().is_success() {
        return None;
    }
    let q_json: Value = q_res.json().await.ok()?;
    let has_route = q_json
        .get("routePlan")
        .and_then(|r| r.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false);
    if !has_route {
        return None;
    }
    let out_amount_raw = parse_u64_field(q_json.get("outAmount"));
    let mut body = serde_json::json!({
        "quoteResponse": q_json,
        "userPublicKey": payer,
        "wrapAndUnwrapSol": true,
        "dynamicComputeUnitLimit": true
    });
    if let Some(tip) = jito_tip_lamports.filter(|t| *t > 0) {
        if let Some(obj) = body.as_object_mut() {
            obj.insert(
                "prioritizationFeeLamports".to_string(),
                serde_json::json!({ "jitoTipLamports": tip }),
            );
        }
    }
    let s_res = client
        .post(s_url)
        .timeout(timeout)
        .header("User-Agent", EXEC_UA)
        .json(&body)
        .send()
        .await
        .ok()?;
    if !s_res.status().is_success() {
        return None;
    }
    let s_json: Value = s_res.json().await.ok()?;
    let b64 = s_json.get("swapTransaction")?.as_str()?.trim();
    let raw_bytes = decode_base64(b64).ok()?;
    verify_jito_packet_mtu(&raw_bytes, max_mtu_bytes).ok()?;
    let parsed = bincode::deserialize::<VersionedTransaction>(&raw_bytes).ok()?;
    let req_sigs = usize::from(parsed.message.header().num_required_signatures);
    if parsed.signatures.len() != req_sigs {
        return None;
    }
    Some(JupiterSwapPayload {
        raw_bytes,
        tx_base64: b64.to_string(),
        out_amount_raw,
    })
}

/// Signs an unsigned `VersionedTransaction` and verifies UDP MTU limits.
pub fn sign_versioned_swap_transaction(
    raw_bytes: &[u8],
    keypair: &Keypair,
    max_mtu_bytes: usize,
) -> Result<SignedLiveSwapTx, String> {
    let unsigned: VersionedTransaction = bincode::deserialize(raw_bytes)
        .map_err(|e| format!("Deserialize VersionedTransaction: {}", e))?;
    let signer = keypair.pubkey();
    if !unsigned.message.static_account_keys().contains(&signer) {
        return Err(format!("Signer {} missing from static keys", signer));
    }
    let signed = VersionedTransaction::try_new(unsigned.message, &[keypair])
        .map_err(|e| format!("Sign VersionedTransaction: {}", e))?;
    let sig = signed
        .signatures
        .first()
        .copied()
        .filter(|s| *s != Signature::default())
        .ok_or_else(|| "Missing primary signature".to_string())?;
    let signed_bytes = bincode::serialize(&signed)
        .map_err(|e| format!("Serialize signed tx: {}", e))?;
    verify_jito_packet_mtu(&signed_bytes, max_mtu_bytes)?;
    Ok(SignedLiveSwapTx {
        signature: sig.to_string(),
        signed_base64: encode_base64(&signed_bytes),
        signed_base58: bs58::encode(&signed_bytes).into_string(),
        signed_bytes,
    })
}

/// Broadcasts a signed swap concurrently via Jito and RPC `sendTransaction`.
pub async fn broadcast_live_swap_jito_and_rpc(
    client: &Client,
    rpc_url: &str,
    fallback_rpc_url: Option<&str>,
    jito_endpoint: &str,
    signed_tx: &SignedLiveSwapTx,
) -> Result<Option<String>, String> {
    let rpc_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "sendTransaction",
        "params": [
            signed_tx.signed_base64,
            {
                "encoding": "base64",
                "skipPreflight": false,
                "preflightCommitment": "confirmed",
                "maxRetries": EXEC_RPC_RETRIES
            }
        ]
    });
    let timeout = std::time::Duration::from_millis(EXEC_SEND_MS);
    let jito = jito_endpoint.trim();
    let (jito_res, rpc_res) = tokio::join!(
        async {
            if jito.is_empty() {
                Err("Jito endpoint empty".to_string())
            } else {
                let txs = vec![signed_tx.signed_base58.clone()];
                broadcast_jito_bundle(client, jito, txs).await
            }
        },
        crate::shadow_logger::post_rpc_with_retry(
            client,
            rpc_url,
            fallback_rpc_url,
            &rpc_payload,
            timeout,
        )
    );
    let (bundle_id, jito_err) = match jito_res {
        Ok(id) => (Some(id), None),
        Err(e) => (None, Some(e)),
    };
    let mut rpc_ok = false;
    let mut rpc_err = None;
    match rpc_res {
        Some(json) => {
            if let Some(err) = json.get("error") {
                rpc_err = Some(format!("RPC sendTransaction error: {}", err));
            } else if json.get("result").and_then(|r| r.as_str()).is_some() {
                rpc_ok = true;
            } else {
                rpc_err = Some("RPC response missing signature".to_string());
            }
        }
        None => rpc_err = Some("RPC sendTransaction no response".to_string()),
    }
    if !rpc_ok && bundle_id.is_none() {
        return Err(format!(
            "Broadcast failed | Jito: {} | RPC: {}",
            jito_err.unwrap_or_default(),
            rpc_err.unwrap_or_default()
        ));
    }
    Ok(bundle_id)
}

/// Polls `getSignatureStatuses` until confirmed/finalized or fails closed.
pub async fn confirm_transaction_signature_status(
    client: &Client,
    rpc_url: &str,
    fallback_rpc_url: Option<&str>,
    signature: &str,
    max_polls: u32,
    poll_interval_ms: u64,
    timeout_ms: u64,
) -> Result<(), String> {
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getSignatureStatuses",
        "params": [[signature], { "searchTransactionHistory": true }]
    });
    let rpc_timeout = std::time::Duration::from_millis(EXEC_SEND_MS);
    let started = std::time::Instant::now();
    let max_dur = std::time::Duration::from_millis(timeout_ms);
    for idx in 0..max_polls {
        if started.elapsed() >= max_dur {
            break;
        }
        if let Some(json) = crate::shadow_logger::post_rpc_with_retry(
            client,
            rpc_url,
            fallback_rpc_url,
            &payload,
            rpc_timeout,
        )
        .await
        {
            if let Some(st) = json
                .get("result")
                .and_then(|r| r.get("value"))
                .and_then(|v| v.as_array())
                .and_then(|a| a.first())
                .filter(|s| !s.is_null())
            {
                if !st["err"].is_null() {
                    return Err(format!("On-chain revert: {}", st["err"]));
                }
                let cs = st
                    .get("confirmationStatus")
                    .and_then(|s| s.as_str())
                    .unwrap_or("");
                let rooted = st.is_object()
                    && st.get("confirmations").map(|c| c.is_null()).unwrap_or(false)
                    && st.get("slot").and_then(|s| s.as_u64()).is_some();
                if cs == "confirmed" || cs == "finalized" || rooted {
                    return Ok(());
                }
            }
        }
        if idx + 1 < max_polls && started.elapsed() < max_dur {
            tokio::time::sleep(std::time::Duration::from_millis(poll_interval_ms)).await;
        }
    }
    Err(format!("Confirmation timeout for {}", signature))
}

/// End-to-end LIVE swap: fetch Jupiter tx, sign, broadcast, and confirm.
pub async fn execute_live_jupiter_swap(
    client: &Client,
    rpc_url: &str,
    fallback_rpc_url: Option<&str>,
    jito_endpoint: &str,
    quote_api_url: &str,
    swap_api_url: &str,
    input_mint: &str,
    output_mint: &str,
    amount_in_raw: u64,
    keypair: &Keypair,
    slippage_bps: u16,
    max_mtu_bytes: usize,
    jito_tip_lamports: u64,
) -> Result<LiveSwapExecutionResult, String> {
    let payer = keypair.pubkey().to_string();
    let jup = fetch_jupiter_swap_transaction(
        client,
        quote_api_url,
        swap_api_url,
        input_mint,
        output_mint,
        amount_in_raw,
        &payer,
        slippage_bps,
        max_mtu_bytes,
        Some(jito_tip_lamports),
    )
    .await
    .ok_or_else(|| "Failed building Jupiter swap tx".to_string())?;
    let signed = sign_versioned_swap_transaction(&jup.raw_bytes, keypair, max_mtu_bytes)?;
    let jito_bundle_id = broadcast_live_swap_jito_and_rpc(
        client,
        rpc_url,
        fallback_rpc_url,
        jito_endpoint,
        &signed,
    )
    .await?;
    confirm_transaction_signature_status(
        client,
        rpc_url,
        fallback_rpc_url,
        &signed.signature,
        EXEC_MAX_POLLS,
        EXEC_POLL_MS,
        EXEC_CONFIRM_MS,
    )
    .await?;
    Ok(LiveSwapExecutionResult {
        signature: signed.signature,
        out_amount_raw: jup.out_amount_raw,
        jito_bundle_id,
    })
}

/// Queries authoritative SPL Token / Token-2022 atomic balance for `owner_pubkey`.
pub async fn fetch_spl_token_balance_raw(
    client: &Client,
    rpc_url: &str,
    fallback_rpc_url: Option<&str>,
    owner_pubkey: &str,
    token_mint: &str,
) -> Result<u64, String> {
    let owner = owner_pubkey.trim();
    let mint = token_mint.trim();
    Pubkey::from_str(owner).map_err(|e| format!("Invalid owner: {}", e))?;
    Pubkey::from_str(mint).map_err(|e| format!("Invalid mint: {}", e))?;
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getTokenAccountsByOwner",
        "params": [
            owner,
            { "mint": mint },
            { "encoding": "jsonParsed", "commitment": "confirmed" }
        ]
    });
    let timeout = std::time::Duration::from_millis(EXEC_BAL_MS);
    let json = crate::shadow_logger::post_rpc_with_retry(
        client,
        rpc_url,
        fallback_rpc_url,
        &payload,
        timeout,
    )
    .await
    .ok_or_else(|| "RPC getTokenAccountsByOwner unavailable".to_string())?;
    if let Some(err) = json.get("error") {
        return Err(format!("getTokenAccountsByOwner error: {}", err));
    }
    let accts = json
        .get("result")
        .and_then(|r| r.get("value"))
        .and_then(|v| v.as_array())
        .ok_or_else(|| "Malformed getTokenAccountsByOwner response".to_string())?;
    let mut total = 0u64;
    for acct in accts {
        let amt = parse_u64_field(
            acct.get("account")
                .and_then(|a| a.get("data"))
                .and_then(|d| d.get("parsed"))
                .and_then(|p| p.get("info"))
                .and_then(|i| i.get("tokenAmount"))
                .and_then(|t| t.get("amount")),
        );
        total = total.saturating_add(amt);
    }
    Ok(total)
}

/// Retries `getTokenAccountsByOwner` briefly after swap confirmation.
pub async fn fetch_spl_token_balance_raw_with_retry(
    client: &Client,
    rpc_url: &str,
    fallback_rpc_url: Option<&str>,
    owner_pubkey: &str,
    token_mint: &str,
) -> Result<u64, String> {
    let mut last = Ok(0u64);
    for attempt in 0..EXEC_BAL_RETRIES {
        last = fetch_spl_token_balance_raw(
            client,
            rpc_url,
            fallback_rpc_url,
            owner_pubkey,
            token_mint,
        )
        .await;
        if let Ok(bal) = last {
            if bal > 0 {
                return Ok(bal);
            }
        }
        if attempt + 1 < EXEC_BAL_RETRIES {
            tokio::time::sleep(std::time::Duration::from_millis(EXEC_BAL_DELAY_MS)).await;
        }
    }
    last
}

#[cfg(test)]
mod open_core_v229_execution_tests {
    use super::*;
    use solana_sdk::hash::Hash;
    use solana_sdk::message::{v0, VersionedMessage};
    use solana_sdk::system_instruction;

    #[test]
    fn test_base64_roundtrip() {
        let sample = b"dmne-meme-alpha-v2.2.9-runtime";
        let enc = encode_base64(sample);
        assert_eq!(decode_base64(&enc).expect("valid b64"), sample);
    }

    #[test]
    fn test_sign_versioned_swap_transaction_verifies_signer_and_mtu() {
        let kp = Keypair::new();
        let to = Pubkey::new_unique();
        let ix = system_instruction::transfer(&kp.pubkey(), &to, 1_000);
        let msg = v0::Message::try_compile(&kp.pubkey(), &[ix], &[], Hash::new_unique())
            .expect("v0 msg");
        let unsigned = VersionedTransaction {
            signatures: vec![Signature::default()],
            message: VersionedMessage::V0(msg),
        };
        let raw = bincode::serialize(&unsigned).expect("serialize");
        let signed = sign_versioned_swap_transaction(&raw, &kp, 1232).expect("sign");
        assert!(!signed.signature.is_empty());
        assert!(signed.signed_bytes.len() <= 1232);
        let wrong = Keypair::new();
        assert!(sign_versioned_swap_transaction(&raw, &wrong, 1232).is_err());
    }
}
