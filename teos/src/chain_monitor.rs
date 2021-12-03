use std::ops::Deref;
use std::{thread, time};

use lightning::chain;
use lightning_block_sync::poll::{ChainTip, Poll, ValidatedBlockHeader};
use lightning_block_sync::BlockSourceError;
use lightning_block_sync::{Cache, SpvClient};

/// Component in charge of monitoring the chain for new blocks.
///
/// Takes care of polling `bitcoind` for new tips and hand it to subscribers.
/// It is mainly a wrapper around [chain::Listen] that provides some logging.

// TODO: Most of the logic here may be redundant. The same core functionality can be implemented using
// a loop that calls `SpvClient::poll_best_chain` every `polling_delta`. However, this gives us some
// nice logging (user feedback) so leaving as is for now.
pub struct ChainMonitor<'a, P, C, L>
where
    P: Poll,
    C: Cache,
    L: Deref,
    L::Target: chain::Listen,
{
    /// A bitcoin client to poll best tips from.
    spv_client: SpvClient<'a, P, C, L>,
    /// The lat known block header by the [ChainMonitor].
    last_known_block_header: ValidatedBlockHeader,
    /// The time between polls.
    polling_delta: time::Duration,
}

impl<'a, P, C, L> ChainMonitor<'a, P, C, L>
where
    P: Poll,
    C: Cache,
    L: Deref,
    L::Target: chain::Listen,
{
    /// Creates a new [ChainMonitor] instance.
    pub async fn new(
        spv_client: SpvClient<'a, P, C, L>,
        last_known_block_header: ValidatedBlockHeader,
        polling_delta_sec: u64,
    ) -> ChainMonitor<'a, P, C, L> {
        ChainMonitor {
            spv_client,
            last_known_block_header,
            polling_delta: time::Duration::from_secs(polling_delta_sec),
        }
    }

    /// Monitors `bitcoind` polling the best chain tip every [polling_delta](Self::polling_delta).
    ///
    /// Serves the data to its listeners (through [chain::Listen]) and logs data about the polled tips.
    pub async fn monitor_chain(&mut self) -> Result<(), BlockSourceError> {
        loop {
            match self.spv_client.poll_best_tip().await {
                Ok((chain_tip, _)) => match chain_tip {
                    ChainTip::Common => {
                        log::info!("No new tip found");
                    }
                    ChainTip::Better(new_best) => {
                        log::info!("New tip found: {}", new_best.header.block_hash());
                        self.last_known_block_header = new_best;
                    }

                    ChainTip::Worse(worse) => {
                        // This would happen both if a block has less chainwork than the previous one, or if it has the same chainwork
                        // but it forks from the parent. The former should not matter, given a reorg will be detected by the subscribers
                        // once we reach the same work*. The latter is a valid case and should be passed along.
                        // The only caveat here would be that the caches of the subscribers are smaller than the reorg, which should
                        // never happen under reasonable assumptions (e.g. cache of 6 blocks).
                        log::warn!("Worse tip found: {:?}", worse);

                        if worse.chainwork == self.last_known_block_header.chainwork {
                        } else {
                            log::warn!("New tip has less work than the previous one")
                        }
                    }
                },
                // FIXME: This may need finer catching
                Err(_) => log::error!("Connection lost with bitcoind"),
            };

            thread::sleep(self.polling_delta);
        }
    }
}
