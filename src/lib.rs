use near_jsonrpc_client::{methods, JsonRpcClient};
use near_primitives::{
    hash::CryptoHash,
    views::{
        BlockView, ChunkHeaderView, ExecutionOutcomeWithIdView, SignedTransactionView,
        StatusSyncInfo,
    },
};
use near_sdk::AccountId;
use tokio::{
    sync::mpsc::{self},
    time::{sleep, Duration},
};

pub async fn get_logs(
    client: &JsonRpcClient,
    chunk_header: ChunkHeaderView,
    account_ids: Vec<AccountId>,
) -> anyhow::Result<Vec<(CryptoHash, String)>> {
    let txs = get_transactions(&client, chunk_header.chunk_hash).await?;
    let mut logs = Vec::new();
    for tx in txs {
        let outcomes = get_receipt(&client, tx.hash, tx.signer_id).await?;
        for oc in outcomes {
            let outcome = oc.outcome;
            if account_ids.contains(&outcome.executor_id) {
                for log in outcome.logs.iter() {
                    logs.push((tx.hash, log.to_string()));
                }
            }
        }
    }

    Ok(logs)
}

pub async fn get_receipt(
    client: &JsonRpcClient,
    tx_hash: CryptoHash,
    sender_account_id: AccountId,
) -> anyhow::Result<Vec<ExecutionOutcomeWithIdView>> {
    Ok(client
        .call(methods::tx::RpcTransactionStatusRequest {
            transaction_info: methods::tx::TransactionInfo::TransactionId {
                tx_hash,
                sender_account_id,
            },
        })
        .await?
        .receipts_outcome)
}

pub async fn get_transactions(
    client: &JsonRpcClient,
    chunk_id: CryptoHash,
) -> anyhow::Result<Vec<SignedTransactionView>> {
    Ok(client
        .call(methods::chunk::RpcChunkRequest {
            chunk_reference: near_jsonrpc_primitives::types::chunks::ChunkReference::ChunkHash {
                chunk_id,
            },
        })
        .await?
        .transactions)
}

pub async fn get_chain_info(client: &JsonRpcClient) -> anyhow::Result<StatusSyncInfo> {
    Ok(client
        .call(methods::status::RpcStatusRequest)
        .await?
        .sync_info)
}

pub async fn get_block(client: &JsonRpcClient, height: u64) -> anyhow::Result<BlockView> {
    Ok(client
        .call(methods::block::RpcBlockRequest {
            block_reference: near_primitives::types::BlockReference::BlockId(
                near_primitives::types::BlockId::Height(height),
            ),
        })
        .await?)
}

pub fn fetch_blocks_from(
    mut height: u64,
    new_height: u64,
    rpc: &str,
) -> anyhow::Result<mpsc::Receiver<BlockView>> {
    let (tx, rx) = mpsc::channel(20);
    let client = JsonRpcClient::connect(rpc);

    tokio::spawn(async move {
        loop {
            if height >= new_height {
                break;
            }
            println!("fetching height -- {}", height);
            match get_block_with_retries(&client, height).await {
                Ok(block) => {
                    if let Err(e) = tx.send(block).await {
                        println!("Block receiver disconnected: {e}");
                        break;
                    }
                    height += 1;
                }
                Err(err) => {
                    println!("get_block_with_retries error:{}", err);
                    break;
                }
            }
        }
    });

    Ok(rx)
}

async fn get_block_with_retries(client: &JsonRpcClient, height: u64) -> anyhow::Result<BlockView> {
    let mut errors = 0;
    loop {
        match get_block(client, height).await {
            Ok(block) => return Ok(block),
            Err(err) => {
                if cfg!(test) {
                    return Err(err);
                }

                errors += 1;
                let seconds = 1 << errors;
                println!("failed to fetch block {height}, retrying in {seconds}s: {err}");

                if seconds > 120 {
                    println!("would sleep for more than 120s, giving up");
                    return Err(err);
                }

                sleep(Duration::from_secs(seconds)).await;
            }
        }
    }
}
