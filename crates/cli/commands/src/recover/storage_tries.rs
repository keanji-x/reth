use crate::common::{AccessRights, CliNodeTypes, Environment, EnvironmentArgs};
use alloy_consensus::BlockHeader;
use clap::Parser;
use reth_chainspec::{EthChainSpec, EthereumHardforks};
use reth_cli::chainspec::ChainSpecParser;
use reth_cli_runner::CliContext;
use reth_db_api::{
    cursor::{DbCursorRO, DbDupCursorRW},
    tables,
    transaction::DbTx,
};
use reth_provider::{BlockNumReader, HeaderProvider, ProviderError};
use reth_trie::StateRoot;
use reth_trie_db::DatabaseStateRoot;
use std::sync::Arc;
use tracing::*;

/// `reth recover storage-tries` command
#[derive(Debug, Parser)]
pub struct Command<C: ChainSpecParser> {
    #[command(flatten)]
    env: EnvironmentArgs<C>,
}

impl<C: ChainSpecParser<ChainSpec: EthChainSpec + EthereumHardforks>> Command<C> {
    /// Execute `storage-tries` recovery command
    pub async fn execute<N: CliNodeTypes<ChainSpec = C::ChainSpec>>(
        self,
        _ctx: CliContext,
    ) -> eyre::Result<()> {
        let Environment { provider_factory, .. } = self.env.init::<N>(AccessRights::RW)?;

        let mut provider = provider_factory.provider_rw()?;
        let best_block = provider.best_block_number()?;
        let best_header = provider
            .sealed_header(best_block)?
            .ok_or_else(|| ProviderError::HeaderNotFound(best_block.into()))?;

        let mut deleted_tries = 0;
        let tx_mut = provider.tx_mut();
        let mut hashed_account_cursor = tx_mut.cursor_read::<tables::HashedAccounts>()?;
        let mut storage_trie_cursor = tx_mut.cursor_dup_read::<tables::StoragesTrie>()?;
        let mut entry = storage_trie_cursor.first()?;

        info!(target: "reth::cli", "Starting pruning of storage tries");
        while let Some((hashed_address, _)) = entry {
            if hashed_account_cursor.seek_exact(hashed_address)?.is_none() {
                deleted_tries += 1;
                storage_trie_cursor.delete_current_duplicates()?;
            }

            entry = storage_trie_cursor.next()?;
        }

        let state_root = StateRoot::from_tx(tx_mut).root()?;
        if state_root != best_header.state_root() {
            eyre::bail!(
                "Recovery failed. Incorrect state root. Expected: {:?}. Received: {:?}",
                best_header.state_root(),
                state_root
            );
        }

        provider.commit()?;
        info!(target: "reth::cli", deleted = deleted_tries, "Finished recovery");

        Ok(())
    }
}

impl<C: ChainSpecParser> Command<C> {
    /// Returns the underlying chain being used to run this command
    pub fn chain_spec(&self) -> Option<&Arc<C::ChainSpec>> {
        Some(&self.env.chain)
    }
}
