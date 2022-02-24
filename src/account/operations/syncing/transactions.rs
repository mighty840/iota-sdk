// Copyright 2021 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use crate::account::{
    handle::AccountHandle,
    operations::transfer::submit_transaction::submit_transaction_payload,
    types::{InclusionState, Transaction},
};

use iota_client::{
    bee_message::{input::Input, output::OutputId, payload::transaction::TransactionEssence, MessageId},
    bee_rest_api::types::dtos::LedgerInclusionStateDto,
};

use std::{
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

// ignore outputs and transactions from other networks
// check if outputs are unspent, rebroadcast, reattach...
// also revalidate that the locked outputs needs to be there, maybe there was a conflict or the transaction got
// confirmed, then they should get removed

pub(crate) struct TransactionSyncResult {
    pub(crate) updated_transactions: Vec<Transaction>,
    // Outputs that got spent
    pub(crate) spent_output_ids: Vec<OutputId>,
    // Inputs from conflicting transactions that are unspent, but should be removed from the locked outputs so they are
    // available again
    pub(crate) output_ids_to_unlock: Vec<OutputId>,
}

/// Sync transactions and reattach them if unconfirmed. Returns the transaction with updated metadata and spent output
/// ids that don't need to be locked anymore
pub(crate) async fn sync_transactions(account_handle: &AccountHandle) -> crate::Result<TransactionSyncResult> {
    log::debug!("[SYNC] sync pending transactions");
    let account = account_handle.read().await;

    let network_id = account_handle.client.get_network_id().await?;

    let mut updated_transactions = Vec::new();
    let mut spent_output_ids = Vec::new();
    // Inputs from conflicting transactions that are unspent, but should be removed from the locked outputs so they are
    // available again
    let mut output_ids_to_unlock = Vec::new();
    let mut transactions_to_reattach = Vec::new();

    for transaction_id in &account.pending_transactions {
        log::debug!("[SYNC] sync pending transaction {}", transaction_id);
        let mut transaction = account
            .transactions
            .get(transaction_id)
            // panic during development to easier detect if something is wrong, should be handled different later
            .expect("transaction id stored, but transaction is missing")
            .clone();
        // only check transaction from the network we're connected to
        if transaction.network_id == network_id {
            if let Some(message_id) = transaction.message_id {
                let metadata = account_handle.client.get_message_metadata(&message_id).await?;
                if let Some(inclusion_state) = metadata.ledger_inclusion_state {
                    match inclusion_state {
                        LedgerInclusionStateDto::Included => {
                            log::debug!(
                                "[SYNC] confirmed transaction {} in message {}",
                                transaction_id,
                                metadata.message_id
                            );
                            updated_transaction_and_outputs(
                                transaction,
                                MessageId::from_str(&metadata.message_id)?,
                                InclusionState::Confirmed,
                                &mut updated_transactions,
                                &mut spent_output_ids,
                            );
                        }
                        LedgerInclusionStateDto::Conflicting => {
                            log::debug!("[SYNC] conflicting transaction {}", transaction_id);
                            // try to get the included message, because maybe only this attachment is conflicting
                            // because it got confirmed in another message
                            if let Ok(included_message) = account_handle
                                .client
                                .get_included_message(&transaction.payload.id())
                                .await
                            {
                                updated_transaction_and_outputs(
                                    transaction,
                                    included_message.id(),
                                    InclusionState::Confirmed,
                                    &mut updated_transactions,
                                    &mut spent_output_ids,
                                );
                            } else {
                                // if we didn't get the included message it means that it got pruned, an input was spent
                                // in another transaction or there is another conflict reason
                                // we check the inputs because some of them could still be unspent
                                let TransactionEssence::Regular(essence) = transaction.payload.essence();
                                for input in essence.inputs() {
                                    if let Input::Utxo(input) = input {
                                        if let Ok(output_response) =
                                            account_handle.client.get_output(input.output_id()).await
                                        {
                                            if output_response.is_spent {
                                                spent_output_ids.push(*input.output_id());
                                            } else {
                                                output_ids_to_unlock.push(*input.output_id());
                                            }
                                        } else {
                                            // if we didn't get the output it could be because it got already spent and
                                            // pruned, even if that's not the case we well get it again during next
                                            // syncing
                                            spent_output_ids.push(*input.output_id());
                                        }
                                    }
                                }

                                transaction.inclusion_state = InclusionState::Conflicting;
                                updated_transactions.push(transaction);
                            }
                        }
                        LedgerInclusionStateDto::NoTransaction => {
                            unreachable!("We should only get the metadata for messages with a transaction payload")
                        }
                    }
                } else {
                    let time_now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .expect("Time went backwards")
                        .as_millis();
                    // Reattach if older than 30 seconds
                    if transaction.timestamp + 30000 < time_now {
                        transactions_to_reattach.push(transaction);
                    }
                }
            } else {
                // transaction wasn't submitted yet, so we have to send it again
                transactions_to_reattach.push(transaction);
            }
        }
    }
    drop(account);
    for mut transaction in transactions_to_reattach {
        log::debug!("[SYNC] reattach transaction");
        let reattached_msg = submit_transaction_payload(account_handle, transaction.payload.clone()).await?;
        transaction.message_id.replace(reattached_msg);
        updated_transactions.push(transaction);
    }

    Ok(TransactionSyncResult {
        updated_transactions,
        spent_output_ids,
        output_ids_to_unlock,
    })
}

fn updated_transaction_and_outputs(
    mut transaction: Transaction,
    message_id: MessageId,
    inclusion_state: InclusionState,
    updated_transactions: &mut Vec<Transaction>,
    spent_output_ids: &mut Vec<OutputId>,
) {
    transaction.message_id.replace(message_id);
    transaction.inclusion_state = inclusion_state;
    // get spent inputs
    let TransactionEssence::Regular(essence) = transaction.payload.essence();
    for input in essence.inputs() {
        if let Input::Utxo(input) = input {
            spent_output_ids.push(*input.output_id());
        }
    }
    updated_transactions.push(transaction);
}