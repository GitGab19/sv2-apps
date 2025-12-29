// This file contains integration tests for the `TranslatorSv2` module.
use integration_tests_sv2::{
    interceptor::{IgnoreMessage, MessageDirection, ReplaceMessage},
    template_provider::DifficultyLevel,
    *,
};
use stratum_apps::stratum_core::mining_sv2::*;

use std::collections::HashSet;
use stratum_apps::stratum_core::{
    common_messages_sv2::{
        SetupConnectionError, MESSAGE_TYPE_SETUP_CONNECTION, MESSAGE_TYPE_SETUP_CONNECTION_ERROR,
        MESSAGE_TYPE_SETUP_CONNECTION_SUCCESS,
    },
    mining_sv2::{
        OpenMiningChannelError, MESSAGE_TYPE_OPEN_EXTENDED_MINING_CHANNEL,
        MESSAGE_TYPE_OPEN_EXTENDED_MINING_CHANNEL_SUCCESS,
    },
    parsers_sv2::{self, AnyMessage},
    template_distribution_sv2::MESSAGE_TYPE_SUBMIT_SOLUTION,
};

// This test runs an sv2 translator between an sv1 mining device and a pool. the connection between
// the translator and the pool is intercepted by a sniffer. The test checks if the translator and
// the pool exchange the correct messages upon connection. And that the miner is able to submit
// shares.
#[tokio::test]
async fn translate_sv1_to_sv2_successfully() {
    start_tracing();
    let (_tp, tp_addr) = start_template_provider(None, DifficultyLevel::Low);
    let (_pool, pool_addr) = start_pool(sv2_tp_config(tp_addr), vec![], vec![]).await;
    let (pool_translator_sniffer, pool_translator_sniffer_addr) =
        start_sniffer("0", pool_addr, false, vec![], None);
    let (_, tproxy_addr) =
        start_sv2_translator(&[pool_translator_sniffer_addr], false, vec![], vec![], None).await;
    let (_minerd_process, _minerd_addr) = start_minerd(tproxy_addr, None, None, false).await;
    pool_translator_sniffer
        .wait_for_message_type(MessageDirection::ToUpstream, MESSAGE_TYPE_SETUP_CONNECTION)
        .await;
    pool_translator_sniffer
        .wait_for_message_type(
            MessageDirection::ToDownstream,
            MESSAGE_TYPE_SETUP_CONNECTION_SUCCESS,
        )
        .await;
    pool_translator_sniffer
        .wait_for_message_type(
            MessageDirection::ToUpstream,
            MESSAGE_TYPE_OPEN_EXTENDED_MINING_CHANNEL,
        )
        .await;
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
}

// Demonstrates the scenario where TProxy falls back to the secondary pool
// after the primary pool returns a `SetupConnection.Error`.
#[tokio::test]
async fn test_translator_fallback_on_setup_connection_error() {
    start_tracing();
    let (_tp, tp_addr) = start_template_provider(None, DifficultyLevel::Low);
    let (_pool_1, pool_addr_1) = start_pool(sv2_tp_config(tp_addr), vec![], vec![]).await;
    let (_pool_2, pool_addr_2) = start_pool(sv2_tp_config(tp_addr), vec![], vec![]).await;

    let random_error_code = "Something went wrong".to_string();

    let setup_connection_success_replace = ReplaceMessage::new(
        MessageDirection::ToDownstream,
        MESSAGE_TYPE_SETUP_CONNECTION_SUCCESS,
        AnyMessage::Common(parsers_sv2::CommonMessages::SetupConnectionError(
            SetupConnectionError {
                flags: 0,
                error_code: random_error_code.try_into().unwrap(),
            },
        )),
    );

    let (pool_translator_sniffer_1, pool_translator_sniffer_addr_1) = start_sniffer(
        "A",
        pool_addr_1,
        false,
        vec![setup_connection_success_replace.into()],
        None,
    );

    let (pool_translator_sniffer_2, pool_translator_sniffer_addr_2) =
        start_sniffer("B", pool_addr_2, false, vec![], None);

    let (_, tproxy_addr) = start_sv2_translator(
        &[
            pool_translator_sniffer_addr_1,
            pool_translator_sniffer_addr_2,
        ],
        false,
        vec![],
        vec![],
        None,
    )
    .await;

    let (_minerd_process, _minerd_addr) = start_minerd(tproxy_addr, None, None, false).await;

    pool_translator_sniffer_1
        .wait_for_message_type(MessageDirection::ToUpstream, MESSAGE_TYPE_SETUP_CONNECTION)
        .await;
    pool_translator_sniffer_1
        .wait_for_message_type(
            MessageDirection::ToDownstream,
            MESSAGE_TYPE_SETUP_CONNECTION_ERROR,
        )
        .await;

    pool_translator_sniffer_2
        .wait_for_message_type(MessageDirection::ToUpstream, MESSAGE_TYPE_SETUP_CONNECTION)
        .await;

    pool_translator_sniffer_2
        .wait_for_message_type(
            MessageDirection::ToDownstream,
            MESSAGE_TYPE_SETUP_CONNECTION_SUCCESS,
        )
        .await;

    pool_translator_sniffer_2
        .wait_for_message_type(
            MessageDirection::ToUpstream,
            MESSAGE_TYPE_OPEN_EXTENDED_MINING_CHANNEL,
        )
        .await;
    pool_translator_sniffer_2
        .wait_for_message_type(
            MessageDirection::ToDownstream,
            MESSAGE_TYPE_OPEN_EXTENDED_MINING_CHANNEL_SUCCESS,
        )
        .await;
}

// Demonstrates the scenario where the primary pool returns an `OpenMiningChannel.Error`,
// causing TProxy to fall back to the secondary pool.
#[tokio::test]
async fn test_translator_fallback_on_open_mining_message_error() {
    start_tracing();
    let (_tp, tp_addr) = start_template_provider(None, DifficultyLevel::Low);
    let (_pool_1, pool_addr_1) = start_pool(sv2_tp_config(tp_addr), vec![], vec![]).await;
    let (_pool_2, pool_addr_2) = start_pool(sv2_tp_config(tp_addr), vec![], vec![]).await;

    let random_error_code = "Something went wrong".to_string();

    let open_mining_channel_success_replace = ReplaceMessage::new(
        MessageDirection::ToDownstream,
        MESSAGE_TYPE_OPEN_EXTENDED_MINING_CHANNEL_SUCCESS,
        AnyMessage::Mining(parsers_sv2::Mining::OpenMiningChannelError(
            OpenMiningChannelError {
                request_id: 0,
                error_code: random_error_code.try_into().unwrap(),
            },
        )),
    );

    let (pool_translator_sniffer_1, pool_translator_sniffer_addr_1) = start_sniffer(
        "A",
        pool_addr_1,
        false,
        vec![open_mining_channel_success_replace.into()],
        None,
    );

    let (pool_translator_sniffer_2, pool_translator_sniffer_addr_2) =
        start_sniffer("B", pool_addr_2, false, vec![], None);

    let (_, tproxy_addr) = start_sv2_translator(
        &[
            pool_translator_sniffer_addr_1,
            pool_translator_sniffer_addr_2,
        ],
        false,
        vec![],
        vec![],
        None,
    )
    .await;

    let (_minerd_process, _minerd_addr) = start_minerd(tproxy_addr, None, None, false).await;

    pool_translator_sniffer_1
        .wait_for_message_type(MessageDirection::ToUpstream, MESSAGE_TYPE_SETUP_CONNECTION)
        .await;
    pool_translator_sniffer_1
        .wait_for_message_type(
            MessageDirection::ToDownstream,
            MESSAGE_TYPE_SETUP_CONNECTION_SUCCESS,
        )
        .await;
    pool_translator_sniffer_1
        .wait_for_message_type(
            MessageDirection::ToUpstream,
            MESSAGE_TYPE_OPEN_EXTENDED_MINING_CHANNEL,
        )
        .await;

    pool_translator_sniffer_2
        .wait_for_message_type(MessageDirection::ToUpstream, MESSAGE_TYPE_SETUP_CONNECTION)
        .await;

    pool_translator_sniffer_2
        .wait_for_message_type(
            MessageDirection::ToDownstream,
            MESSAGE_TYPE_SETUP_CONNECTION_SUCCESS,
        )
        .await;

    pool_translator_sniffer_2
        .wait_for_message_type(
            MessageDirection::ToUpstream,
            MESSAGE_TYPE_OPEN_EXTENDED_MINING_CHANNEL,
        )
        .await;

    pool_translator_sniffer_2
        .wait_for_message_type(
            MessageDirection::ToDownstream,
            MESSAGE_TYPE_OPEN_EXTENDED_MINING_CHANNEL_SUCCESS,
        )
        .await;
}

// This test verifies that the translator sends keepalive jobs to downstream miners when no new
// jobs are received from upstream, and that shares submitted for keepalive jobs are properly
// received by the pool. Keepalive job_id(s) use the format `{original_job_id}#{counter}`.
#[tokio::test]
async fn test_translator_keepalive_job_sent_and_share_received_by_pool() {
    start_tracing();
    let (_tp, tp_addr) = start_template_provider(None, DifficultyLevel::High);
    let (_pool, pool_addr) = start_pool(sv2_tp_config(tp_addr), vec![], vec![]).await;
    let (pool_translator_sniffer, pool_translator_sniffer_addr) =
        start_sniffer("0", pool_addr, false, vec![], None);

    // Start translator with a short keepalive interval (5 seconds)
    let keepalive_interval_secs = 5_u16;
    let (_, tproxy_addr) = start_sv2_translator(
        &[pool_translator_sniffer_addr],
        false,
        vec![],
        vec![],
        Some(keepalive_interval_secs),
    )
    .await;
    let (sv1_sniffer, sv1_sniffer_addr) = start_sv1_sniffer(tproxy_addr);
    let (_minerd_process, _minerd_addr) = start_minerd(sv1_sniffer_addr, None, None, false).await;

    sv1_sniffer
        .wait_for_message(&["mining.notify"], MessageDirection::ToDownstream)
        .await;

    pool_translator_sniffer
        .wait_for_message_type(
            MessageDirection::ToUpstream,
            MESSAGE_TYPE_SUBMIT_SHARES_EXTENDED,
        )
        .await;

    // Wait for keepalive interval plus some buffer time
    tokio::time::sleep(std::time::Duration::from_secs(
        keepalive_interval_secs as u64 + 3,
    ))
    .await;

    // Wait for a keepalive mining.notify message (job_id contains '#' delimiter)
    sv1_sniffer
        .wait_for_keepalive_notify(MessageDirection::ToDownstream)
        .await;

    // Wait for the share submission success message
    // This proves the keepalive job was valid and the share was properly mapped
    pool_translator_sniffer
        .wait_for_message_type(
            MessageDirection::ToDownstream,
            MESSAGE_TYPE_SUBMIT_SHARES_SUCCESS,
        )
        .await;
}

// This test launches a tProxy in aggregated mode and leverages a MockUpstream to test the correct
// functionalities of grouping extended channels.
#[tokio::test]
async fn aggregated_translator_correctly_deals_with_group_channels() {
    start_tracing();
    let (tp, tp_addr) = start_template_provider(None, DifficultyLevel::Low);
    tp.fund_wallet().unwrap();

    // block SubmitSolution messages from arriving to TP
    // so we avoid shares triggering chain tip updates
    // which we want to do explicitly via generate_blocks()
    let ignore_submit_solution =
        IgnoreMessage::new(MessageDirection::ToUpstream, MESSAGE_TYPE_SUBMIT_SOLUTION);
    let (_sniffer_pool_tp, sniffer_pool_tp_addr) = start_sniffer(
        "0",
        tp_addr,
        false,
        vec![ignore_submit_solution.into()],
        None,
    );

    let (_pool, pool_addr) = start_pool(sv2_tp_config(sniffer_pool_tp_addr), vec![], vec![]).await;

    // ignore SubmitSharesSuccess messages, so we can keep the assertion flow simple
    let ignore_submit_shares_success = IgnoreMessage::new(
        MessageDirection::ToDownstream,
        MESSAGE_TYPE_SUBMIT_SHARES_SUCCESS,
    );
    let (sniffer, sniffer_addr) = start_sniffer(
        "0",
        pool_addr,
        false,
        vec![ignore_submit_shares_success.into()],
        None,
    );

    // aggregated tProxy
    let (_, tproxy_addr) = start_sv2_translator(&[sniffer_addr], true, vec![], vec![], None).await;

    sniffer
        .wait_for_message_type_and_clean_queue(
            MessageDirection::ToDownstream,
            MESSAGE_TYPE_SETUP_CONNECTION_SUCCESS,
        )
        .await;

    let mut minerd_vec = Vec::new();

    // start the first minerd process, to trigger the first OpenExtendedMiningChannel message
    let (minerd_process, _minerd_addr) = start_minerd(tproxy_addr, None, None, false).await;
    minerd_vec.push(minerd_process);

    sniffer
        .wait_for_message_type_and_clean_queue(
            MessageDirection::ToUpstream,
            MESSAGE_TYPE_OPEN_EXTENDED_MINING_CHANNEL,
        )
        .await;
    sniffer
        .wait_for_message_type(
            MessageDirection::ToDownstream,
            MESSAGE_TYPE_OPEN_EXTENDED_MINING_CHANNEL_SUCCESS,
        )
        .await;

    // save the aggregated and group channel IDs
    let (aggregated_channel_id, group_channel_id) = match sniffer.next_message_from_upstream() {
        Some((
            _,
            AnyMessage::Mining(parsers_sv2::Mining::OpenExtendedMiningChannelSuccess(msg)),
        )) => (msg.channel_id, msg.group_channel_id),
        msg => panic!(
            "Expected OpenExtendedMiningChannelSuccess message, found: {:?}",
            msg
        ),
    };

    // wait for the expected NewExtendedMiningJob and SetNewPrevHash messages
    // and clean the queue
    sniffer
        .wait_for_message_type(
            MessageDirection::ToDownstream,
            MESSAGE_TYPE_NEW_EXTENDED_MINING_JOB,
        )
        .await;
    sniffer
        .wait_for_message_type_and_clean_queue(
            MessageDirection::ToDownstream,
            MESSAGE_TYPE_MINING_SET_NEW_PREV_HASH,
        )
        .await;

    // open a few more extended channels to be aggregated with the first one
    const N_MINERDS: u32 = 5;
    for _i in 0..N_MINERDS {
        let (minerd_process, _minerd_addr) = start_minerd(tproxy_addr, None, None, false).await;
        minerd_vec.push(minerd_process);

        // wait a bit
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        // assert no furter OpenExtendedMiningChannel messages are sent
        sniffer
            .assert_message_not_present(
                MessageDirection::ToUpstream,
                MESSAGE_TYPE_OPEN_EXTENDED_MINING_CHANNEL,
            )
            .await;
    }

    // wait for a SubmitSharesExtended message
    sniffer
        .wait_for_message_type(
            MessageDirection::ToUpstream,
            MESSAGE_TYPE_SUBMIT_SHARES_EXTENDED,
        )
        .await;

    let share_channel_id = match sniffer.next_message_from_downstream() {
        Some((_, AnyMessage::Mining(parsers_sv2::Mining::SubmitSharesExtended(msg)))) => {
            msg.channel_id
        }
        msg => panic!("Expected SubmitSharesExtended message, found: {:?}", msg),
    };

    assert_eq!(
        aggregated_channel_id, share_channel_id,
        "Share submitted to the correct channel ID"
    );
    assert_ne!(
        share_channel_id, group_channel_id,
        "Share NOT submitted to the group channel ID"
    );

    // wait for another share, so we can clean the queue
    sniffer
        .wait_for_message_type_and_clean_queue(
            MessageDirection::ToUpstream,
            MESSAGE_TYPE_SUBMIT_SHARES_EXTENDED,
        )
        .await;

    // now let's force a mempool update, so we trigger a NewExtendedMiningJob message
    // it's actually directed to the group channel Id, not the aggregated channel Id
    // nevertheless, tProxy should still submit the share to the aggregated channel Id
    tp.create_mempool_transaction().unwrap();

    sniffer
        .wait_for_message_type(
            MessageDirection::ToDownstream,
            MESSAGE_TYPE_NEW_EXTENDED_MINING_JOB,
        )
        .await;
    let new_extended_mining_job = match sniffer.next_message_from_upstream() {
        Some((_, AnyMessage::Mining(parsers_sv2::Mining::NewExtendedMiningJob(msg)))) => msg,
        msg => panic!("Expected NewExtendedMiningJob message, found: {:?}", msg),
    };

    // here we're actually asserting pool behavior, not tProxy
    // but still good to have, to ensure the global sanity of the test
    assert_ne!(new_extended_mining_job.channel_id, aggregated_channel_id);
    assert_eq!(new_extended_mining_job.channel_id, group_channel_id);

    loop {
        sniffer
            .wait_for_message_type(
                MessageDirection::ToUpstream,
                MESSAGE_TYPE_SUBMIT_SHARES_EXTENDED,
            )
            .await;
        let submit_shares_extended = match sniffer.next_message_from_downstream() {
            Some((_, AnyMessage::Mining(parsers_sv2::Mining::SubmitSharesExtended(msg)))) => msg,
            msg => panic!("Expected SubmitSharesExtended message, found: {:?}", msg),
        };

        // assert the share is submitted to the aggregated channel Id
        assert_eq!(submit_shares_extended.channel_id, aggregated_channel_id);
        assert_ne!(submit_shares_extended.channel_id, group_channel_id);

        if submit_shares_extended.job_id == 2 {
            break;
        }
    }

    // now let's force a chain tip update, so we trigger a SetNewPrevHash + NewExtendedMiningJob
    // message pair
    tp.generate_blocks(1);

    sniffer
        .wait_for_message_type(
            MessageDirection::ToDownstream,
            MESSAGE_TYPE_NEW_EXTENDED_MINING_JOB,
        )
        .await;
    let new_extended_mining_job = match sniffer.next_message_from_upstream() {
        Some((_, AnyMessage::Mining(parsers_sv2::Mining::NewExtendedMiningJob(msg)))) => msg,
        msg => panic!("Expected NewExtendedMiningJob message, found: {:?}", msg),
    };

    // again, asserting pool behavior, not tProxy
    // just to ensure the global sanity of the test
    assert_ne!(new_extended_mining_job.channel_id, aggregated_channel_id);
    assert_eq!(new_extended_mining_job.channel_id, group_channel_id);

    sniffer
        .wait_for_message_type(
            MessageDirection::ToDownstream,
            MESSAGE_TYPE_MINING_SET_NEW_PREV_HASH,
        )
        .await;
    let set_new_prev_hash = match sniffer.next_message_from_upstream() {
        Some((_, AnyMessage::Mining(parsers_sv2::Mining::SetNewPrevHash(msg)))) => msg,
        msg => panic!("Expected SetNewPrevHash message, found: {:?}", msg),
    };

    // again, asserting pool behavior, not tProxy
    // just to ensure the global sanity of the test
    assert_eq!(set_new_prev_hash.channel_id, group_channel_id);
    assert_ne!(set_new_prev_hash.channel_id, aggregated_channel_id);

    loop {
        sniffer
            .wait_for_message_type(
                MessageDirection::ToUpstream,
                MESSAGE_TYPE_SUBMIT_SHARES_EXTENDED,
            )
            .await;
        let submit_shares_extended = match sniffer.next_message_from_downstream() {
            Some((_, AnyMessage::Mining(parsers_sv2::Mining::SubmitSharesExtended(msg)))) => msg,
            msg => panic!("Expected SubmitSharesExtended message, found: {:?}", msg),
        };

        // assert the share is submitted to the aggregated channel Id
        assert_eq!(submit_shares_extended.channel_id, aggregated_channel_id);
        assert_ne!(submit_shares_extended.channel_id, group_channel_id);

        if submit_shares_extended.job_id == 3 {
            break;
        }
    }
}

// This test launches a tProxy in non-aggregated mode and leverages a MockUpstream to test the
// correct functionalities of grouping extended channels.
#[tokio::test]
async fn non_aggregated_translator_correctly_deals_with_group_channels() {
    start_tracing();

    let (tp, tp_addr) = start_template_provider(None, DifficultyLevel::Low);
    tp.fund_wallet().unwrap();

    // block SubmitSolution messages from arriving to TP
    // so we avoid shares triggering chain tip updates
    // which we want to do explicitly via generate_blocks()
    let ignore_submit_solution =
        IgnoreMessage::new(MessageDirection::ToUpstream, MESSAGE_TYPE_SUBMIT_SOLUTION);
    let (_sniffer_pool_tp, sniffer_pool_tp_addr) = start_sniffer(
        "0",
        tp_addr,
        false,
        vec![ignore_submit_solution.into()],
        None,
    );

    let (_pool, pool_addr) = start_pool(sv2_tp_config(sniffer_pool_tp_addr), vec![], vec![]).await;

    // ignore SubmitSharesSuccess messages, so we can keep the assertion flow simple
    let ignore_submit_shares_success = IgnoreMessage::new(
        MessageDirection::ToDownstream,
        MESSAGE_TYPE_SUBMIT_SHARES_SUCCESS,
    );
    let (sniffer, sniffer_addr) = start_sniffer(
        "0",
        pool_addr,
        false,
        vec![ignore_submit_shares_success.into()],
        None,
    );
    let (_, tproxy_addr) = start_sv2_translator(&[sniffer_addr], false, vec![], vec![], None).await;

    sniffer
        .wait_for_message_type_and_clean_queue(
            MessageDirection::ToUpstream,
            MESSAGE_TYPE_SETUP_CONNECTION,
        )
        .await;
    sniffer
        .wait_for_message_type_and_clean_queue(
            MessageDirection::ToDownstream,
            MESSAGE_TYPE_SETUP_CONNECTION_SUCCESS,
        )
        .await;

    const N_EXTENDED_CHANNELS: u32 = 5;
    const EXPECTED_GROUP_CHANNEL_ID: u32 = 1;
    let mut minerd_vec = Vec::new();
    let mut channel_ids = Vec::new();

    for _i in 0..N_EXTENDED_CHANNELS {
        let (minerd, _minerd_addr) = start_minerd(tproxy_addr, None, None, false).await;
        minerd_vec.push(minerd);
        sniffer
            .wait_for_message_type_and_clean_queue(
                MessageDirection::ToUpstream,
                MESSAGE_TYPE_OPEN_EXTENDED_MINING_CHANNEL,
            )
            .await;
        sniffer
            .wait_for_message_type(
                MessageDirection::ToDownstream,
                MESSAGE_TYPE_OPEN_EXTENDED_MINING_CHANNEL_SUCCESS,
            )
            .await;
        let open_extended_mining_channel_success = match sniffer.next_message_from_upstream() {
            Some((
                _,
                AnyMessage::Mining(parsers_sv2::Mining::OpenExtendedMiningChannelSuccess(msg)),
            )) => msg,
            msg => panic!(
                "Expected OpenExtendedMiningChannelSuccess message, found: {:?}",
                msg
            ),
        };
        let channel_id = open_extended_mining_channel_success.channel_id;
        channel_ids.push(channel_id);

        // we expect this initial NewExtendedMiningJob message to be directed to the newly created
        // channel ID, not the group channel ID this is actually asserting pool behavior,
        // not tProxy but still good to have, to ensure the global sanity of the test
        sniffer
            .wait_for_message_type(
                MessageDirection::ToDownstream,
                MESSAGE_TYPE_NEW_EXTENDED_MINING_JOB,
            )
            .await;
        let new_extended_mining_job = match sniffer.next_message_from_upstream() {
            Some((_, AnyMessage::Mining(parsers_sv2::Mining::NewExtendedMiningJob(msg)))) => msg,
            msg => panic!("Expected NewExtendedMiningJob message, found: {:?}", msg),
        };
        assert_eq!(new_extended_mining_job.channel_id, channel_id);
        assert_ne!(
            new_extended_mining_job.channel_id,
            EXPECTED_GROUP_CHANNEL_ID
        );

        // we expect this initial SetNewPrevHash message to be directed to the newly created channel
        // ID, not the group channel ID this is actually asserting pool behavior, not tProxy
        // but still good to have, to ensure the global sanity of the test
        sniffer
            .wait_for_message_type(
                MessageDirection::ToDownstream,
                MESSAGE_TYPE_MINING_SET_NEW_PREV_HASH,
            )
            .await;
        let set_new_prev_hash = match sniffer.next_message_from_upstream() {
            Some((_, AnyMessage::Mining(parsers_sv2::Mining::SetNewPrevHash(msg)))) => msg,
            msg => panic!("Expected SetNewPrevHash message, found: {:?}", msg),
        };
        assert_eq!(set_new_prev_hash.channel_id, channel_id);
        assert_ne!(set_new_prev_hash.channel_id, EXPECTED_GROUP_CHANNEL_ID);
    }

    // all channels must submit at least one share with job_id = 1
    let mut channel_submitted_to: HashSet<u32> = channel_ids.clone().into_iter().collect();
    loop {
        sniffer
            .wait_for_message_type(
                MessageDirection::ToUpstream,
                MESSAGE_TYPE_SUBMIT_SHARES_EXTENDED,
            )
            .await;
        let submit_shares_extended = match sniffer.next_message_from_downstream() {
            Some((_, AnyMessage::Mining(parsers_sv2::Mining::SubmitSharesExtended(msg)))) => msg,
            msg => panic!("Expected SubmitSharesExtended message, found: {:?}", msg),
        };

        if submit_shares_extended.job_id != 1 {
            continue;
        }

        assert_ne!(submit_shares_extended.channel_id, EXPECTED_GROUP_CHANNEL_ID);

        channel_submitted_to.remove(&submit_shares_extended.channel_id);
        if channel_submitted_to.is_empty() {
            break;
        }
    }

    // now let's force a mempool update, so we trigger a NewExtendedMiningJob message
    // that's actually directed to the group channel ID, and not each individual channel
    tp.create_mempool_transaction().unwrap();

    sniffer
        .wait_for_message_type(
            MessageDirection::ToDownstream,
            MESSAGE_TYPE_NEW_EXTENDED_MINING_JOB,
        )
        .await;
    let new_extended_mining_job = match sniffer.next_message_from_upstream() {
        Some((_, AnyMessage::Mining(parsers_sv2::Mining::NewExtendedMiningJob(msg)))) => msg,
        msg => panic!("Expected NewExtendedMiningJob message, found: {:?}", msg),
    };
    assert_eq!(
        new_extended_mining_job.channel_id,
        EXPECTED_GROUP_CHANNEL_ID
    );

    // all channels must submit at least one share with job_id = 2
    let mut channel_submitted_to: HashSet<u32> = channel_ids.clone().into_iter().collect();
    loop {
        sniffer
            .wait_for_message_type(
                MessageDirection::ToUpstream,
                MESSAGE_TYPE_SUBMIT_SHARES_EXTENDED,
            )
            .await;
        let submit_shares_extended = match sniffer.next_message_from_downstream() {
            Some((_, AnyMessage::Mining(parsers_sv2::Mining::SubmitSharesExtended(msg)))) => msg,
            msg => panic!("Expected SubmitSharesExtended message, found: {:?}", msg),
        };

        if submit_shares_extended.job_id != 2 {
            continue;
        }

        assert_ne!(submit_shares_extended.channel_id, EXPECTED_GROUP_CHANNEL_ID);

        channel_submitted_to.remove(&submit_shares_extended.channel_id);
        if channel_submitted_to.is_empty() {
            break;
        }
    }

    // now let's force a chain tip update, so we trigger a NewExtendedMiningJob + SetNewPrevHash
    // message pair
    tp.generate_blocks(1);

    sniffer
        .wait_for_message_type(
            MessageDirection::ToDownstream,
            MESSAGE_TYPE_NEW_EXTENDED_MINING_JOB,
        )
        .await;
    let new_extended_mining_job = match sniffer.next_message_from_upstream() {
        Some((_, AnyMessage::Mining(parsers_sv2::Mining::NewExtendedMiningJob(msg)))) => msg,
        msg => panic!("Expected NewExtendedMiningJob message, found: {:?}", msg),
    };
    assert_eq!(
        new_extended_mining_job.channel_id,
        EXPECTED_GROUP_CHANNEL_ID
    );

    sniffer
        .wait_for_message_type(
            MessageDirection::ToDownstream,
            MESSAGE_TYPE_MINING_SET_NEW_PREV_HASH,
        )
        .await;
    let set_new_prev_hash = match sniffer.next_message_from_upstream() {
        Some((_, AnyMessage::Mining(parsers_sv2::Mining::SetNewPrevHash(msg)))) => msg,
        msg => panic!("Expected SetNewPrevHash message, found: {:?}", msg),
    };
    assert_eq!(set_new_prev_hash.channel_id, EXPECTED_GROUP_CHANNEL_ID);

    // all channels must submit at least one share with job_id = 3
    let mut channel_submitted_to: HashSet<u32> = channel_ids.clone().into_iter().collect();
    loop {
        sniffer
            .wait_for_message_type(
                MessageDirection::ToUpstream,
                MESSAGE_TYPE_SUBMIT_SHARES_EXTENDED,
            )
            .await;
        let submit_shares_extended = match sniffer.next_message_from_downstream() {
            Some((_, AnyMessage::Mining(parsers_sv2::Mining::SubmitSharesExtended(msg)))) => msg,
            msg => panic!("Expected SubmitSharesExtended message, found: {:?}", msg),
        };

        if submit_shares_extended.job_id != 3 {
            continue;
        }

        assert_ne!(submit_shares_extended.channel_id, EXPECTED_GROUP_CHANNEL_ID);

        channel_submitted_to.remove(&submit_shares_extended.channel_id);
        if channel_submitted_to.is_empty() {
            break;
        }
    }
}
