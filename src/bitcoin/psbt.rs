use bdk::{blockchain::Blockchain, psbt::PsbtUtils, SignOptions, TransactionDetails};
use bitcoin::{consensus::serialize, hashes::hex::ToHex, util::psbt::PartiallySignedTransaction};
use thiserror::Error;

use crate::{
    bitcoin::{get_blockchain, MemoryWallet},
    debug,
};

#[derive(Error, Debug)]
pub enum BitcoinPsbtError {
    /// Could not finalize when signing PSBT
    #[error("Could not finalize when signing PSBT")]
    CouldNotFinalizePsbt,
    /// BDK error
    #[error(transparent)]
    BdkError(#[from] bdk::Error),
    /// BDK esplora error
    #[error(transparent)]
    BdkEsploraError(#[from] bdk::esplora_client::Error),
}

// Only signs an original psbt.
pub async fn sign_psbt(
    wallet: &MemoryWallet,
    mut psbt: PartiallySignedTransaction,
) -> Result<PartiallySignedTransaction, BitcoinPsbtError> {
    debug!("Funding PSBT...");
    let opts = SignOptions {
        allow_all_sighashes: true,
        remove_partial_sigs: false,
        ..Default::default()
    };
    wallet.lock().await.sign(&mut psbt, opts)?;
    Ok(psbt)
}

pub async fn multi_sign_psbt(
    wallets: Vec<MemoryWallet>,
    mut psbt: PartiallySignedTransaction,
) -> Result<PartiallySignedTransaction, BitcoinPsbtError> {
    let total_wallets = wallets.len();
    debug!(format!(
        "Signing PSBT ({total_wallets}/{total_wallets}) ..."
    ));

    let mut sign_count = 0;
    for wallet in wallets {
        wallet.lock().await.sign(
            &mut psbt,
            SignOptions {
                allow_all_sighashes: true,
                remove_partial_sigs: false,
                ..Default::default()
            },
        )?;

        sign_count += 1;
        debug!(format!("PSBT Sign: ({sign_count}/{total_wallets})"));
    }

    Ok(psbt)
}

/// Signs and broadcasts a transaction given a Psbt
pub async fn sign_and_publish_psbt(
    wallet: &MemoryWallet,
    mut psbt: PartiallySignedTransaction,
) -> Result<TransactionDetails, BitcoinPsbtError> {
    debug!("Signing PSBT...");
    let finalized = wallet
        .lock()
        .await
        .sign(&mut psbt, SignOptions::default())?;
    debug!(format!("Finalized: {finalized}"));
    if finalized {
        debug!("Signed PSBT:", base64::encode(&serialize(&psbt)));
        let fee_amount = psbt.fee_amount().expect("fee amount on PSBT is known");
        let tx = psbt.extract_tx();
        debug!("tx:", &serialize(&tx.clone()).to_hex());
        let blockchain = get_blockchain().await;
        blockchain.broadcast(&tx).await?;

        let txid = tx.txid();
        let tx = blockchain.get_tx(&txid).await?;

        let mut sent = 0;
        let mut received = 0;

        if let Some(tx) = tx.clone() {
            sent = tx.output.iter().fold(0, |sum, output| output.value + sum);
            received = sent - fee_amount;
        }

        let details = TransactionDetails {
            transaction: tx,
            txid,
            received,
            sent,
            fee: Some(fee_amount),
            confirmation_time: None,
        };

        Ok(details)
    } else {
        Err(BitcoinPsbtError::CouldNotFinalizePsbt)
    }
}

/// Signs and broadcasts a transaction given a Psbt
pub async fn multi_sign_and_publish_psbt(
    wallets: Vec<MemoryWallet>,
    mut psbt: PartiallySignedTransaction,
) -> Result<TransactionDetails, BitcoinPsbtError> {
    let total_wallets = wallets.len();
    debug!(format!(
        "Signing PSBT ({total_wallets}/{total_wallets}) ..."
    ));

    let mut sign_count = 0;
    let mut finalized = false;
    for wallet in wallets {
        finalized = wallet.lock().await.sign(
            &mut psbt,
            SignOptions {
                allow_all_sighashes: true,
                remove_partial_sigs: false,
                ..Default::default()
            },
        )?;

        sign_count += 1;
        debug!(format!("PSBT Sign: ({sign_count}/{total_wallets})"));
    }

    debug!(format!("Finalized: {finalized}"));
    if finalized {
        debug!("Signed PSBT:", base64::encode(&serialize(&psbt)));
        let fee_amount = psbt.fee_amount().expect("fee amount on PSBT is known");
        let tx = psbt.extract_tx();
        debug!("tx:", &serialize(&tx.clone()).to_hex());
        let blockchain = get_blockchain().await;
        blockchain.broadcast(&tx).await?;

        let txid = tx.txid();
        let tx = blockchain.get_tx(&txid).await?;

        let mut sent = 0;
        let mut received = 0;

        if let Some(tx) = tx.clone() {
            sent = tx.output.iter().fold(0, |sum, output| output.value + sum);
            received = sent - fee_amount;
        }

        let details = TransactionDetails {
            transaction: tx,
            txid,
            received,
            sent,
            fee: Some(fee_amount),
            confirmation_time: None,
        };

        Ok(details)
    } else {
        Err(BitcoinPsbtError::CouldNotFinalizePsbt)
    }
}
