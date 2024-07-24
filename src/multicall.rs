use alloy::network::{ReceiptResponse, TransactionBuilder};
use alloy::providers::Provider;
use alloy::rpc::types::TransactionRequest;
use alloy::{
    providers::{Network, RootProvider},
    transports::{Transport, TransportErrorKind, TransportResult},
};
use alloy_primitives::{Address, Bytes};
use alloy_sol_types::{sol, SolCall};
use futures::future::join_all;
use oneshot::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::time::sleep;

// Define the Multicall3 interface
sol! {
    interface Multicall3 {
        struct Call3 {
            address target;
            bytes callData;
            bool allow_failure;
        }
        struct Result {
            bool success;
            bytes returnData;
        }
        function aggregate3(Call3[] calldata calls) external returns (Result[] memory returnData);
    }
}

struct BatchedCall {
    target: Address,
    call_data: Bytes,
    allow_failure: bool,
    callback: tokio::sync::oneshot::Sender<TransportResult<Bytes>>,
}

pub struct BatchingProvider<T, N> {
    inner: Arc<RootProvider<T, N>>,
    batch: Arc<Mutex<Vec<BatchedCall>>>,
    multicall_address: Address,
    batch_interval: Duration,
}

impl<
        T: Transport + Clone + 'static,
        N: Network<TransactionRequest = TransactionRequest> + 'static,
    > BatchingProvider<T, N>
{
    pub fn new(
        provider: RootProvider<T, N>,
        multicall_address: Address,
        batch_interval: Duration,
        chain_id: u64,
    ) -> Self {
        let provider = Arc::new(provider);
        let batch = Arc::new(Mutex::new(Vec::new()));

        // Start the batching task
        let batch_clone = Arc::clone(&batch);
        let provider_clone = Arc::clone(&provider);
        tokio::spawn(async move {
            loop {
                sleep(batch_interval).await;
                Self::process_batch(&provider_clone, &batch_clone, multicall_address, chain_id)
                    .await;
            }
        });

        Self {
            inner: provider,
            batch,
            multicall_address,
            batch_interval,
        }
    }

    async fn process_batch(
        provider: &Arc<RootProvider<T, N>>,
        batch: &Arc<Mutex<Vec<BatchedCall>>>,
        multicall_address: Address,
        chain_id: u64,
    ) {
        let calls = {
            let mut batch = batch.lock().unwrap();
            std::mem::take(&mut *batch)
        };

        if calls.is_empty() {
            return;
        }

        // Prepare the multicall
        let multicall_calls: Vec<_> = calls
            .iter()
            .map(|call| Multicall3::Call3 {
                target: call.target,
                callData: call.call_data.clone(),
                allow_failure: call.allow_failure,
            })
            .collect();

        let multicall = Multicall3::aggregate3Call {
            calls: multicall_calls,
        };

        let tx = TransactionRequest::default()
            .with_to(multicall_address)
            .with_chain_id(chain_id);
        // Execute the multicall
        let result = provider.send_transaction(tx).await.unwrap();
        let receipt = result.get_receipt().await;
        // match receipt {
        //     Ok(data) => {
        //         // Distribute results
        //         for (call, result) in calls.into_iter().zip(data.) {
        //             let _ = call.callback.send(if result.status() {
        //                 Ok(Bytes::from(result.block_hash().unwrap()))
        //             } else {
        //                 Err(TransportErrorKind::backend_gone())
        //             });
        //         }
        //     }
        //     Err(e) => {
        //         // If the multicall itself failed, send the error to all callbacks
        //         for call in calls {
        //             let _ = call.callback.send(Err(e));
        //         }
        //     }
        // }
    }
}
