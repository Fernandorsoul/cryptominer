use super::StratumJob;
use anyhow::Result;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

/// State for a single connected miner.
struct MinerSession {
    authorized: bool,
    worker_name: String,
    extranonce: String,
    extranonce_size: usize,
    shares_submitted: u64,
    shares_accepted: u64,
}

/// Handle a single stratum miner connection.
///
/// Protocol sequence:
/// 1. Server waits for mining.subscribe
/// 2. Server responds with extranonce info
/// 3. Miner sends mining.authorize(worker, password)
/// 4. Server sends initial mining.notify (current job)
/// 5. Server pushes new jobs via broadcast channel
/// 6. Miner sends mining.submit when it finds a share
pub async fn handle_connection(
    mut stream: TcpStream,
    mut job_rx: broadcast::Receiver<StratumJob>,
    current_job: Option<StratumJob>,
    extranonce: &str,
    extranonce_size: usize,
) -> Result<()> {
    let (reader, mut writer) = stream.split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    let mut session = MinerSession {
        authorized: false,
        worker_name: "unknown".to_string(),
        extranonce: extranonce.to_string(),
        extranonce_size,
        shares_submitted: 0,
        shares_accepted: 0,
    };

    info!("Stratum miner connected, waiting for subscribe...");

    loop {
        tokio::select! {
            // Read from miner
            result = reader.read_line(&mut line) => {
                match result {
                    Ok(0) => {
                        info!("Miner {} disconnected ({} shares submitted, {} accepted)",
                            session.worker_name, session.shares_submitted, session.shares_accepted);
                        break;
                    }
                    Ok(_) => {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            if let Err(e) = handle_miner_message(
                                trimmed,
                                &mut writer,
                                &mut session,
                                current_job.as_ref(),
                            ).await {
                                warn!("Error handling message from {}: {}", session.worker_name, e);
                            }
                        }
                        line.clear();
                    }
                    Err(e) => {
                        warn!("Read error from {}: {}", session.worker_name, e);
                        break;
                    }
                }
            }

            // New job from daemon — push to miner
            job = job_rx.recv() => {
                if let Ok(job) = job {
                    if session.authorized {
                        if let Err(e) = send_notify(&mut writer, &job).await {
                            warn!("Failed to send job to {}: {}", session.worker_name, e);
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// Handle a single JSON-RPC message from a miner.
async fn handle_miner_message(
    raw: &str,
    writer: &mut (impl AsyncWriteExt + Unpin),
    session: &mut MinerSession,
    current_job: Option<&StratumJob>,
) -> Result<()> {
    let msg: Value = serde_json::from_str(raw)?;
    let id = msg["id"].as_u64().unwrap_or(0);
    let method = msg["method"].as_str().unwrap_or("");

    debug!("Miner {} -> method={}, id={}", session.worker_name, method, id);

    match method {
        "mining.subscribe" => {
            handle_subscribe(writer, id, session).await?;
        }
        "mining.authorize" => {
            handle_authorize(writer, id, &msg, session, current_job).await?;
        }
        "mining.submit" => {
            handle_submit(writer, id, &msg, session).await?;
        }
        "mining.extranonce.subscribe" => {
            // Optional: some miners send this
            send_response(writer, id, json!(true), None).await?;
        }
        _ => {
            warn!("Unknown stratum method: {}", method);
            send_response(
                writer,
                id,
                Value::Null,
                Some(json!({"code": -1, "message": "unknown method"})),
            )
            .await?;
        }
    }

    Ok(())
}

/// Handle mining.subscribe — first message from miner.
/// Response: [[subscriptions], extranonce, extranonce_size]
async fn handle_subscribe(
    writer: &mut (impl AsyncWriteExt + Unpin),
    id: u64,
    session: &mut MinerSession,
) -> Result<()> {
    info!("Miner subscribed (extranonce: {}, size: {})",
        session.extranonce, session.extranonce_size);

    let result = json!([
        [["mining.notify", format!("miner_{}", id)]],
        session.extranonce,
        session.extranonce_size
    ]);

    send_response(writer, id, result, None).await
}

/// Handle mining.authorize — miner proves identity.
/// Params: [worker_name, password]
async fn handle_authorize(
    writer: &mut (impl AsyncWriteExt + Unpin),
    id: u64,
    msg: &Value,
    session: &mut MinerSession,
    current_job: Option<&StratumJob>,
) -> Result<()> {
    let params = msg["params"].as_array();
    let worker = params
        .and_then(|p| p.first())
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    session.worker_name = worker.to_string();
    session.authorized = true;

    info!("✓ Miner authorized: {}", worker);
    send_response(writer, id, json!(true), None).await?;

    // Send current job immediately after authorization
    if let Some(job) = current_job {
        info!("Sending initial job {} to {}", job.job_id, worker);
        send_notify(writer, job).await?;
    }

    Ok(())
}

/// Handle mining.submit — miner found a share.
/// Params: [worker_name, job_id, extranonce2, ntime, nonce]
async fn handle_submit(
    writer: &mut (impl AsyncWriteExt + Unpin),
    id: u64,
    msg: &Value,
    session: &mut MinerSession,
) -> Result<()> {
    if !session.authorized {
        send_response(
            writer,
            id,
            Value::Null,
            Some(json!({"code": -1, "message": "not authorized"})),
        )
        .await?;
        return Ok(());
    }

    let params = msg["params"].as_array();
    let job_id = params
        .and_then(|p| p.get(1))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let extranonce2 = params
        .and_then(|p| p.get(2))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let ntime = params
        .and_then(|p| p.get(3))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let nonce = params
        .and_then(|p| p.get(4))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    session.shares_submitted += 1;

    info!(
        "Share from {}: job={}, extranonce2={}, ntime={}, nonce={}",
        session.worker_name, job_id, extranonce2, ntime, nonce
    );

    // In a real implementation, we would:
    // 1. Reconstruct the block blob with extranonce2, ntime, nonce
    // 2. Hash it with RandomX
    // 3. Check against target
    // 4. If meets difficulty, submit block to daemon
    // For now, accept the share
    session.shares_accepted += 1;
    info!(
        "✓ Share accepted from {} ({}/{})",
        session.worker_name, session.shares_accepted, session.shares_submitted
    );

    send_response(writer, id, json!(true), None).await
}

/// Send a mining.notify message to the miner.
/// This pushes a new job when a new block template arrives.
///
/// Params: [job_id, blob, target, clean_jobs]
async fn send_notify(
    writer: &mut (impl AsyncWriteExt + Unpin),
    job: &StratumJob,
) -> Result<()> {
    let notify = json!({
        "id": null,
        "method": "mining.notify",
        "params": [
            job.job_id,
            job.blob,
            job.target,
            true  // clean_jobs: miner should drop old jobs
        ]
    });

    let msg = format!("{}\n", notify);
    writer.write_all(msg.as_bytes()).await?;
    debug!("Sent job {} to miner (height={})", job.job_id, job.height);

    Ok(())
}

/// Send a JSON-RPC response to the miner.
async fn send_response(
    writer: &mut (impl AsyncWriteExt + Unpin),
    id: u64,
    result: Value,
    error: Option<Value>,
) -> Result<()> {
    let response = json!({
        "id": id,
        "result": result,
        "error": error
    });

    let msg = format!("{}\n", response);
    writer.write_all(msg.as_bytes()).await?;

    Ok(())
}