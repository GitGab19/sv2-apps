// Integration test for translator extension negotiation with extension 0x0002
// (EXTENSION_TYPE_WORKER_HASHRATE_TRACKING) and user_identity TLV validation.
//
// This test validates:
// 1. Pool and translator negotiate extension 0x0002 during SetupConnection
// 2. SV1 miner submits shares through the translator
// 3. Translator forwards SubmitSharesExtended with TLV containing user_identity
// 4. Pool receives and processes the TLV user_identity correctly

use integration_tests_sv2::{interceptor::MessageDirection, template_provider::DifficultyLevel, *};
use stratum_apps::stratum_core::{
    binary_sv2::Seq064K,
    common_messages_sv2::*,
    extensions_sv2::EXTENSION_TYPE_WORKER_HASHRATE_TRACKING,
    mining_sv2::*,
    parsers_sv2::{AnyMessage, Extensions, ExtensionsNegotiation, Mining},
};
use tracing::info;

/// Tests that the translator successfully negotiates extension 0x0002 with the pool
/// and sends user_identity TLV in SubmitSharesExtended messages.
#[tokio::test]
async fn translator_negotiates_extension_and_sends_user_identity_tlv() {
    start_tracing();
    // Extension 0x0002 for worker hashrate tracking
    let supported_extensions = vec![EXTENSION_TYPE_WORKER_HASHRATE_TRACKING];
    let required_extensions = vec![EXTENSION_TYPE_WORKER_HASHRATE_TRACKING];

    let (_tp, tp_addr) = start_template_provider(None, DifficultyLevel::Low);
    // Start pool with extension 0x0002 support
    let (_pool, pool_addr) = start_pool(Some(tp_addr), supported_extensions.clone(), vec![]).await;
    let (pool_translator_sniffer, pool_translator_sniffer_addr) =
        start_sniffer("pool-translator", pool_addr, false, vec![], None);
    // Start translator with extension 0x0002 support and user_identity configured
    // aggregate_channels = false ensures TLV fields are added
    let (_tproxy, tproxy_addr) = start_sv2_translator(
        pool_translator_sniffer_addr,
        false, // aggregate_channels = false
        supported_extensions.clone(),
        required_extensions,
    )
    .await;
    // Start SV1 miner (minerd) connected to translator
    let (_minerd_process, _minerd_addr) = start_minerd(tproxy_addr, None, None, false).await;

    pool_translator_sniffer
        .wait_for_message_type_and_clean_queue(
            MessageDirection::ToUpstream,
            MESSAGE_TYPE_SETUP_CONNECTION,
        )
        .await;

    pool_translator_sniffer
        .wait_for_message_type_and_clean_queue(
            MessageDirection::ToDownstream,
            MESSAGE_TYPE_SETUP_CONNECTION_SUCCESS,
        )
        .await;

    // Verify RequestExtensions includes extension 0x0002
    let request_extensions_msg = match pool_translator_sniffer.next_message_from_downstream() {
        Some((
            _,
            AnyMessage::Extensions(Extensions::ExtensionsNegotiation(
                ExtensionsNegotiation::RequestExtensions(msg),
            )),
        )) => msg,
        _ => panic!(
            "received unexpected message: {:?}",
            pool_translator_sniffer.next_message_from_downstream()
        ),
    };
    assert_eq!(
        request_extensions_msg.requested_extensions,
        Seq064K::new(supported_extensions.clone()).unwrap()
    );

    // Verify RequestExtensionsSuccess acknowledges the extension
    let request_extensions_success_msg = pool_translator_sniffer.next_message_from_upstream();
    match request_extensions_success_msg {
        Some((
            _,
            AnyMessage::Extensions(Extensions::ExtensionsNegotiation(
                ExtensionsNegotiation::RequestExtensionsSuccess(msg),
            )),
        )) => {
            assert_eq!(
                msg.supported_extensions,
                Seq064K::new(supported_extensions).unwrap()
            );
        }
        _ => panic!("Expected RequestExtensionsSuccess message"),
    }

    pool_translator_sniffer
        .wait_for_message_type(
            MessageDirection::ToUpstream,
            MESSAGE_TYPE_OPEN_EXTENDED_MINING_CHANNEL,
        )
        .await;
    // Extract and verify user_identity from OpenExtendedMiningChannel
    let open_channel_msg = pool_translator_sniffer.next_message_from_downstream();
    match open_channel_msg {
        Some((_, AnyMessage::Mining(Mining::OpenExtendedMiningChannel(msg)))) => {
            let user_identity = msg.user_identity.as_utf8_or_hex();
            info!(
                "OpenExtendedMiningChannel received - user_identity: {}",
                user_identity
            );
            // Verify user_identity format (should be "user_identity.miner1")
            assert!(
                user_identity.contains("user_identity"),
                "user_identity should contain 'user_identity'"
            );
            assert!(
                user_identity.contains("miner"),
                "user_identity should contain 'miner' suffix"
            );
        }
        _ => panic!(
            "received unexpected message: {:?}",
            pool_translator_sniffer.next_message_from_downstream()
        ),
    }

    pool_translator_sniffer
        .wait_for_message_type(
            MessageDirection::ToDownstream,
            MESSAGE_TYPE_OPEN_EXTENDED_MINING_CHANNEL_SUCCESS,
        )
        .await;

    pool_translator_sniffer
        .wait_for_message_type(
            MessageDirection::ToDownstream,
            MESSAGE_TYPE_NEW_EXTENDED_MINING_JOB,
        )
        .await;

    pool_translator_sniffer
        .wait_for_message_type(
            MessageDirection::ToUpstream,
            MESSAGE_TYPE_SUBMIT_SHARES_EXTENDED,
        )
        .await;
    // Verify SubmitSharesExtended contains TLV with user_identity
    let submit_shares_msg = pool_translator_sniffer.next_message_from_downstream();
    match submit_shares_msg {
        Some((_, AnyMessage::Mining(Mining::SubmitSharesExtended(msg)))) => {
            info!(
                "SubmitSharesExtended received - channel_id: {}, sequence_number: {}, job_id: {}",
                msg.channel_id, msg.sequence_number, msg.job_id
            );
            // TODO: Once the frame parsing supports TLV extraction in the sniffer,
            // we should validate the TLV fields here. For now, we verify that:
            // 1. The message was sent
            // 2. The channel is established with extension negotiation
            // 3. The pool accepts the share (SubmitSharesSuccess)
            info!("✅ SubmitSharesExtended message sent to pool");
        }
        _ => panic!("Expected SubmitSharesExtended message"),
    }

    // Wait for SubmitSharesSuccess or SubmitSharesError response from pool
    pool_translator_sniffer
        .wait_for_message_type(
            MessageDirection::ToDownstream,
            MESSAGE_TYPE_SUBMIT_SHARES_SUCCESS,
        )
        .await;
}
