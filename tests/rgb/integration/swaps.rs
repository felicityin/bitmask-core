#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![cfg(target_arch = "wasm32")]

use crate::rgb::integration::utils::{
    get_uda_data, issuer_issue_contract_v2, send_some_coins, UtxoFilter,
};
use bitmask_core::{
    debug, info,
    structs::{
        AcceptRequest, AcceptResponse, BatchRgbTransferResponse, ContractResponse,
        DecryptedWalletData, FundVaultDetails, IssueResponse, PsbtFeeRequest,
        PublishedPsbtResponse, RgbBidRequest, RgbBidResponse, RgbOfferRequest, RgbOfferResponse,
        RgbSwapRequest, RgbSwapResponse, SecretString, SignPsbtRequest, SignedPsbtResponse,
        WatcherRequest, WatcherResponse,
    },
    web::{
        bitcoin::{
            fund_vault, get_new_address, get_wallet, new_mnemonic, sign_and_publish_psbt_file,
            sign_psbt_file, sync_wallet,
        },
        json_parse, resolve,
        rgb::{
            accept_transfer, create_buyer_bid, create_seller_offer, create_swap_transfer,
            create_watcher, get_contract, verify_transfers,
        },
        set_panic_hook,
    },
};
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
async fn create_scriptless_swap() -> anyhow::Result<()> {
    // 1. Initial Setup
    let seller_keys = resolve(new_mnemonic("".to_string())).await;
    let seller_keys: DecryptedWalletData = json_parse(&seller_keys);
    let buyer_keys = resolve(new_mnemonic("".to_string())).await;
    let buyer_keys: DecryptedWalletData = json_parse(&buyer_keys);

    let watcher_name = "default";
    let issuer_sk = seller_keys.private.nostr_prv.clone();
    let create_watch_req = WatcherRequest {
        name: watcher_name.to_string(),
        xpub: seller_keys.public.watcher_xpub.clone(),
        force: true,
    };
    let create_watch_req = serde_wasm_bindgen::to_value(&create_watch_req).expect("");
    let resp = resolve(create_watcher(issuer_sk, create_watch_req)).await;
    let resp: WatcherResponse = json_parse(&resp);

    let owner_sk = buyer_keys.private.nostr_prv.clone();
    let create_watch_req = WatcherRequest {
        name: watcher_name.to_string(),
        xpub: buyer_keys.public.watcher_xpub.clone(),
        force: true,
    };
    let create_watch_req = serde_wasm_bindgen::to_value(&create_watch_req).expect("");
    let resp = resolve(create_watcher(owner_sk, create_watch_req)).await;
    let resp: WatcherResponse = json_parse(&resp);

    // 2. Setup Wallets (Seller)
    let btc_address_1 = resolve(get_new_address(
        seller_keys.public.btc_descriptor_xpub.clone(),
        None,
    ))
    .await;
    let btc_address_1: String = json_parse(&btc_address_1);

    let default_coins = "0.001";
    resolve(send_some_coins(&btc_address_1, default_coins)).await;

    let btc_descriptor_xprv = SecretString(seller_keys.private.btc_descriptor_xprv.clone());
    let btc_change_descriptor_xprv =
        SecretString(seller_keys.private.btc_change_descriptor_xprv.clone());

    let assets_address_1 = resolve(get_new_address(
        seller_keys.public.rgb_assets_descriptor_xpub.clone(),
        None,
    ))
    .await;
    let assets_address_1: String = json_parse(&assets_address_1);

    let assets_address_2 = resolve(get_new_address(
        seller_keys.public.rgb_assets_descriptor_xpub.clone(),
        None,
    ))
    .await;
    let assets_address_2: String = json_parse(&assets_address_2);

    let uda_address_1 = resolve(get_new_address(
        seller_keys.public.rgb_udas_descriptor_xpub.clone(),
        None,
    ))
    .await;
    let uda_address_1: String = json_parse(&uda_address_1);

    let uda_address_2 = resolve(get_new_address(
        seller_keys.public.rgb_udas_descriptor_xpub.clone(),
        None,
    ))
    .await;
    let uda_address_2: String = json_parse(&uda_address_2);

    let btc_wallet = resolve(get_wallet(
        &btc_descriptor_xprv,
        Some(&btc_change_descriptor_xprv),
    ))
    .await;
    resolve(sync_wallet(&btc_wallet)).await;

    let fund_vault = resolve(fund_vault(
        btc_descriptor_xprv.to_string(),
        btc_change_descriptor_xprv.to_string(),
        assets_address_1,
        assets_address_2,
        uda_address_1,
        uda_address_2,
        Some(1.1),
    ))
    .await;
    let fund_vault: FundVaultDetails = json_parse(&fund_vault);

    // 3. Send some coins (Buyer)
    let btc_address_1 = resolve(get_new_address(
        buyer_keys.public.btc_descriptor_xpub.clone(),
        None,
    ))
    .await;
    let btc_address_1: String = json_parse(&btc_address_1);
    let asset_address_1 = resolve(get_new_address(
        buyer_keys.public.rgb_assets_descriptor_xpub.clone(),
        None,
    ))
    .await;
    let asset_address_1: String = json_parse(&asset_address_1);

    let default_coins = "0.1";
    resolve(send_some_coins(&btc_address_1, default_coins)).await;
    resolve(send_some_coins(&asset_address_1, default_coins)).await;

    // 4. Issue Contract (Seller)
    let issuer_resp = resolve(issuer_issue_contract_v2(
        1,
        "RGB20",
        5,
        false,
        false,
        None,
        None,
        Some(UtxoFilter::with_outpoint(
            fund_vault.assets_output.unwrap_or_default(),
        )),
        Some(seller_keys.clone()),
    ))
    .await;
    let issuer_resp: Vec<IssueResponse> = json_parse(&issuer_resp);
    let IssueResponse {
        contract_id,
        iface,
        supply,
        ..
    } = issuer_resp[0].clone();

    // 5. Create Seller Swap Side
    let contract_amount = supply - 1;
    let seller_sk = seller_keys.private.nostr_prv.clone();
    let bitcoin_price: u64 = 100000;
    let seller_asset_desc = seller_keys.public.rgb_assets_descriptor_xpub.clone();
    let expire_at = (chrono::Local::now() + chrono::Duration::minutes(5))
        .naive_utc()
        .timestamp();
    let seller_swap_req = RgbOfferRequest {
        contract_id: contract_id.clone(),
        iface,
        contract_amount,
        bitcoin_price,
        descriptor: SecretString(seller_asset_desc),
        change_terminal: "/20/1".to_string(),
        bitcoin_changes: vec![],
        expire_at: Some(expire_at),
    };

    let seller_swap_resp = resolve(create_seller_offer(&seller_sk, seller_swap_req)).await;
    let seller_swap_resp: RgbOfferResponse = json_parse(&seller_swap_resp);

    // 7. Create Buyer Swap Side
    let RgbOfferResponse {
        offer_id,
        contract_amount,
        ..
    } = seller_swap_resp;

    let buyer_sk = buyer_keys.private.nostr_prv.clone();
    let buyer_btc_desc = buyer_keys.public.btc_descriptor_xpub.clone();
    let buyer_swap_req = RgbBidRequest {
        offer_id: offer_id.clone(),
        asset_amount: contract_amount,
        descriptor: SecretString(buyer_btc_desc),
        change_terminal: "/1/0".to_string(),
        fee: PsbtFeeRequest::Value(1000),
    };

    let buyer_swap_resp = resolve(create_buyer_bid(&buyer_sk, buyer_swap_req)).await;
    let buyer_swap_resp: RgbBidResponse = json_parse(&buyer_swap_resp);

    // 8. Sign the Buyer Side
    let RgbBidResponse {
        bid_id, swap_psbt, ..
    } = buyer_swap_resp;
    let request = SignPsbtRequest {
        psbt: swap_psbt,
        descriptors: vec![
            SecretString(buyer_keys.private.btc_descriptor_xprv.clone()),
            SecretString(buyer_keys.private.btc_change_descriptor_xprv.clone()),
        ],
    };
    let buyer_psbt_resp = resolve(sign_psbt_file(request)).await;
    let buyer_psbt_resp: SignedPsbtResponse = json_parse(&buyer_psbt_resp);

    // 9. Create Swap PSBT
    let SignedPsbtResponse {
        psbt: swap_psbt, ..
    } = buyer_psbt_resp;
    let final_swap_req = RgbSwapRequest {
        offer_id,
        bid_id,
        swap_psbt,
    };

    let final_swap_resp = resolve(create_swap_transfer(issuer_sk, final_swap_req)).await;
    let final_swap_resp: RgbSwapResponse = json_parse(&final_swap_resp);

    // 8. Sign the Final PSBT
    let RgbSwapResponse {
        final_consig,
        final_psbt,
        ..
    } = final_swap_resp;

    let request = SignPsbtRequest {
        psbt: final_psbt.clone(),
        descriptors: vec![
            SecretString(seller_keys.private.btc_descriptor_xprv.clone()),
            SecretString(seller_keys.private.btc_change_descriptor_xprv.clone()),
            SecretString(seller_keys.private.rgb_assets_descriptor_xprv.clone()),
        ],
    };
    let seller_psbt_resp = resolve(sign_and_publish_psbt_file(request)).await;
    let seller_psbt_resp: PublishedPsbtResponse = json_parse(&seller_psbt_resp);

    // 9. Accept Consig (Buyer/Seller)
    let all_sks = [buyer_sk.clone(), seller_sk.clone()];
    for sk in all_sks {
        let request = AcceptRequest {
            consignment: final_consig.clone(),
            force: false,
        };
        let request = serde_wasm_bindgen::to_value(&request).expect("");
        let resp = resolve(accept_transfer(sk, request)).await;
        let resp: AcceptResponse = json_parse(&resp);
    }

    // 10 Mine Some Blocks
    let whatever_address = "bcrt1p76gtucrxhmn8s5622r859dpnmkj0kgfcel9xy0sz6yj84x6ppz2qk5hpsw";
    resolve(send_some_coins(whatever_address, "0.001")).await;

    // 11. Retrieve Contract (Seller Side)
    let resp = resolve(get_contract(seller_sk.clone(), contract_id.clone())).await;
    let resp: ContractResponse = json_parse(&resp);

    // 12. Retrieve Contract (Buyer Side)
    let resp = resolve(get_contract(buyer_sk, contract_id)).await;
    let resp: ContractResponse = json_parse(&resp);

    // 13. Verify transfers (Seller Side)
    let resp = resolve(verify_transfers(seller_sk)).await;
    let resp: BatchRgbTransferResponse = json_parse(&resp);

    Ok(())
}

#[tokio::test]
async fn create_scriptless_swap_for_uda() -> anyhow::Result<()> {
    // 1. Initial Setup
    let seller_keys = resolve(new_mnemonic("".to_string())).await;
    let seller_keys: DecryptedWalletData = json_parse(&seller_keys);
    let buyer_keys = resolve(new_mnemonic("".to_string())).await;
    let buyer_keys: DecryptedWalletData = json_parse(&buyer_keys);

    let watcher_name = "default";
    let issuer_sk = seller_keys.private.nostr_prv.clone();
    let create_watch_req = WatcherRequest {
        name: watcher_name.to_string(),
        xpub: seller_keys.public.watcher_xpub.clone(),
        force: true,
    };
    let create_watch_req = serde_wasm_bindgen::to_value(&create_watch_req).expect("");
    resolve(create_watcher(issuer_sk, create_watch_req)).await;

    let owner_sk = buyer_keys.private.nostr_prv.clone();
    let create_watch_req = WatcherRequest {
        name: watcher_name.to_string(),
        xpub: buyer_keys.public.watcher_xpub.clone(),
        force: true,
    };
    let create_watch_req = serde_wasm_bindgen::to_value(&create_watch_req).expect("");
    let watcher_resp = resolve(create_watcher(owner_sk, create_watch_req)).await;
    let watcher_resp: WatcherResponse = json_parse(&watcher_resp);

    // 2. Setup Wallets (Seller)
    let btc_address_1 = resolve(get_new_address(
        seller_keys.public.btc_descriptor_xpub.clone(),
        None,
    ))
    .await;
    let btc_address_1: String = json_parse(&btc_address_1);

    let default_coins = "0.001";
    resolve(send_some_coins(&btc_address_1, default_coins)).await;

    let btc_descriptor_xprv = SecretString(seller_keys.private.btc_descriptor_xprv.clone());
    let btc_change_descriptor_xprv =
        SecretString(seller_keys.private.btc_change_descriptor_xprv.clone());

    let assets_address_1 = resolve(get_new_address(
        seller_keys.public.rgb_assets_descriptor_xpub.clone(),
        None,
    ))
    .await;
    let assets_address_1: String = json_parse(&assets_address_1);

    let assets_address_2 = resolve(get_new_address(
        seller_keys.public.rgb_assets_descriptor_xpub.clone(),
        None,
    ))
    .await;
    let assets_address_2: String = json_parse(&assets_address_2);

    let uda_address_1 = resolve(get_new_address(
        seller_keys.public.rgb_udas_descriptor_xpub.clone(),
        None,
    ))
    .await;
    let uda_address_1: String = json_parse(&uda_address_1);

    let uda_address_2 = resolve(get_new_address(
        seller_keys.public.rgb_udas_descriptor_xpub.clone(),
        None,
    ))
    .await;
    let uda_address_2: String = json_parse(&uda_address_2);

    let btc_wallet = resolve(get_wallet(
        &btc_descriptor_xprv,
        Some(&btc_change_descriptor_xprv),
    ))
    .await;
    resolve(sync_wallet(btc_wallet)).await;

    let fund_vault = resolve(fund_vault(
        btc_descriptor_xprv.to_string(),
        btc_change_descriptor_xprv.to_string(),
        assets_address_1,
        assets_address_2,
        uda_address_1,
        uda_address_2,
        Some(1.1),
    ))
    .await;
    let fund_vault: FundVaultDetails = json_parse(&fund_vault);

    // 3. Send some coins (Buyer)
    let btc_address_1 = resolve(get_new_address(
        buyer_keys.public.btc_descriptor_xpub.clone(),
        None,
    ))
    .await;
    let btc_address_1: String = json_parse(&btc_address_1);
    let asset_address_1 = resolve(get_new_address(
        buyer_keys.public.rgb_udas_descriptor_xpub.clone(),
        None,
    ))
    .await;
    let asset_address_1: String = json_parse(&asset_address_1);

    let default_coins = "0.1";
    resolve(send_some_coins(&btc_address_1, default_coins)).await;
    resolve(send_some_coins(&asset_address_1, default_coins)).await;

    // 4. Issue Contract (Seller)
    let metadata = get_uda_data();
    let issuer_resp = resolve(issuer_issue_contract_v2(
        1,
        "RGB21",
        1,
        false,
        false,
        Some(metadata),
        None,
        Some(UtxoFilter::with_outpoint(
            fund_vault.udas_output.unwrap_or_default(),
        )),
        Some(seller_keys.clone()),
    ))
    .await;
    let issuer_resp: Vec<IssueResponse> = json_parse(&issuer_resp);
    let IssueResponse {
        contract_id, iface, ..
    } = issuer_resp[0].clone();

    // 5. Create Seller Swap Side
    let contract_amount = 1;
    let seller_sk = seller_keys.private.nostr_prv.clone();
    let bitcoin_price: u64 = 100000;
    let seller_asset_desc = seller_keys.public.rgb_udas_descriptor_xpub.clone();
    let expire_at = (chrono::Local::now() + chrono::Duration::minutes(5))
        .naive_utc()
        .timestamp();
    let seller_swap_req = RgbOfferRequest {
        contract_id: contract_id.clone(),
        iface,
        contract_amount,
        bitcoin_price,
        descriptor: SecretString(seller_asset_desc),
        change_terminal: "/21/1".to_string(),
        bitcoin_changes: vec![],
        expire_at: Some(expire_at),
    };

    let seller_swap_resp = resolve(create_seller_offer(&seller_sk, seller_swap_req)).await;
    let seller_swap_resp: RgbOfferResponse = json_parse(&seller_swap_resp);

    // 7. Create Buyer Swap Side
    let RgbOfferResponse {
        offer_id,
        contract_amount,
        ..
    } = seller_swap_resp;

    let buyer_sk = buyer_keys.private.nostr_prv.clone();
    let buyer_btc_desc = buyer_keys.public.btc_descriptor_xpub.clone();
    let buyer_swap_req = RgbBidRequest {
        offer_id: offer_id.clone(),
        asset_amount: contract_amount,
        descriptor: SecretString(buyer_btc_desc),
        change_terminal: "/1/0".to_string(),
        fee: PsbtFeeRequest::Value(1000),
    };

    let buyer_swap_resp = resolve(create_buyer_bid(&buyer_sk, buyer_swap_req)).await;
    let buyer_swap_resp: RgbBidResponse = json_parse(&buyer_swap_resp);

    // 8. Sign the Buyer Side
    let RgbBidResponse {
        bid_id, swap_psbt, ..
    } = buyer_swap_resp;
    let request = SignPsbtRequest {
        psbt: swap_psbt,
        descriptors: vec![
            SecretString(buyer_keys.private.btc_descriptor_xprv.clone()),
            SecretString(buyer_keys.private.btc_change_descriptor_xprv.clone()),
        ],
    };
    let buyer_psbt_resp = resolve(sign_psbt_file(request)).await;
    let buyer_psbt_resp: SignedPsbtResponse = json_parse(&buyer_psbt_resp);

    // 9. Create Swap PSBT
    let SignedPsbtResponse {
        psbt: swap_psbt, ..
    } = buyer_psbt_resp;
    let final_swap_req = RgbSwapRequest {
        offer_id,
        bid_id,
        swap_psbt,
    };

    let final_swap_resp = resolve(create_swap_transfer(issuer_sk, final_swap_req)).await;
    let final_swap_resp: RgbSwapResponse = json_parse(&final_swap_resp);

    // 8. Sign the Final PSBT
    let RgbSwapResponse {
        final_consig,
        final_psbt,
        ..
    } = final_swap_resp;

    let request = SignPsbtRequest {
        psbt: final_psbt.clone(),
        descriptors: vec![
            SecretString(seller_keys.private.btc_descriptor_xprv.clone()),
            SecretString(seller_keys.private.btc_change_descriptor_xprv.clone()),
            SecretString(seller_keys.private.rgb_udas_descriptor_xprv.clone()),
        ],
    };
    let seller_psbt_resp = resolve(sign_and_publish_psbt_file(request)).await;
    let seller_psbt_resp: PublishedPsbtResponse = json_parse(&seller_psbt_resp);

    // 9. Accept Consig (Buyer/Seller)
    let all_sks = [buyer_sk.clone(), seller_sk.clone()];
    for sk in all_sks {
        let request = AcceptRequest {
            consignment: final_consig.clone(),
            force: false,
        };
        let request = serde_wasm_bindgen::to_value(&request).expect("");
        let resp = resolve(accept_transfer(sk, request)).await;
        let resp: AcceptResponse = json_parse(&resp);
    }

    // 10 Mine Some Blocks
    let whatever_address = "bcrt1p76gtucrxhmn8s5622r859dpnmkj0kgfcel9xy0sz6yj84x6ppz2qk5hpsw";
    send_some_coins(whatever_address, "0.001").await;

    // 11. Retrieve Contract (Seller Side)
    let resp = resolve(get_contract(seller_sk.clone(), contract_id.clone())).await;
    let resp: ContractResponse = json_parse(&resp);

    // 12. Retrieve Contract (Buyer Side)
    let resp = resolve(get_contract(buyer_sk, contract_id.clone())).await;
    let resp: ContractResponse = json_parse(&resp);

    // 13. Verify transfers (Seller Side)
    let resp = resolve(verify_transfers(seller_sk)).await;
    let resp: BatchRgbTransferResponse = json_parse(&resp);

    Ok(())
}
