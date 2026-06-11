//! SV1 client monitoring integration for Sv1Server
//!
//! This module implements the Sv1ClientsMonitoring trait on `Sv1Server`.
use std::{collections::HashSet, net::IpAddr, sync::Arc, time::Duration};

use asic_rs::{
    core::data::{collector::DataField, hashrate::HashRateUnit, miner::MinerData},
    MinerFactory,
};
use stratum_apps::{
    monitoring::sv1::{MinerTelemetry, Sv1ClientInfo, Sv1ClientsMonitoring},
    utils::types::DownstreamId,
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};

use crate::sv1::{Downstream, Sv1Server};

const MINER_TELEMETRY_EXCLUDED_FIELDS: &[DataField] = &[
    DataField::Mac,
    DataField::SerialNumber,
    DataField::Hostname,
    DataField::ApiVersion,
    DataField::ControlBoardVersion,
    DataField::Chips,
    DataField::ExpectedHashrate,
    DataField::Fans,
    DataField::PsuFans,
    DataField::FluidTemperature,
    DataField::TuningTarget,
    DataField::LightFlashing,
    DataField::Messages,
    DataField::Pools,
];

/// Helper to convert a Downstream to Sv1ClientInfo
fn downstream_to_sv1_client_info(
    downstream: &Downstream,
    miner_telemetry: Option<MinerTelemetry>,
) -> Option<Sv1ClientInfo> {
    downstream
        .downstream_data
        .safe_lock(|dd| Sv1ClientInfo {
            client_id: downstream.downstream_id,
            channel_id: dd.channel_id,
            connection_ip: Some(dd.connection_ip.to_string()),
            authorized_worker_name: dd.authorized_worker_name.clone(),
            user_identity: dd.user_identity.clone(),
            target_hex: hex::encode(dd.target.to_be_bytes()),
            hashrate: dd.hashrate,
            miner_telemetry,
            stable_hashrate: dd.stable_hashrate,
            extranonce1_hex: hex::encode(&dd.extranonce1),
            extranonce2_len: dd.extranonce2_len,
            version_rolling_mask: dd
                .version_rolling_mask
                .as_ref()
                .map(|mask| format!("{:08x}", mask.0)),
            version_rolling_min_bit: dd
                .version_rolling_min_bit
                .as_ref()
                .map(|bit| format!("{:08x}", bit.0)),
        })
        .ok()
}

impl Sv1ClientsMonitoring for Sv1Server {
    fn get_sv1_clients(&self) -> Vec<Sv1ClientInfo> {
        self.downstreams
            .iter()
            .filter_map(|downstream| {
                let miner_telemetry = self.miner_telemetry_for(*downstream.key());
                downstream_to_sv1_client_info(downstream.value(), miner_telemetry)
            })
            .collect()
    }

    fn get_sv1_client_by_id(&self, client_id: usize) -> Option<Sv1ClientInfo> {
        let miner_telemetry = self.miner_telemetry_for(client_id);
        self.downstreams.get(&client_id).and_then(|downstream| {
            downstream_to_sv1_client_info(downstream.value(), miner_telemetry)
        })
    }
}

impl Sv1Server {
    pub(crate) fn miner_telemetry_for(
        &self,
        downstream_id: DownstreamId,
    ) -> Option<MinerTelemetry> {
        self.miner_telemetry
            .get(&downstream_id)
            .map(|telemetry| telemetry.clone())
    }

    pub(crate) async fn run_miner_telemetry_loop(
        self: Arc<Self>,
        refresh_interval: Duration,
        cancellation_token: CancellationToken,
        fallback_token: CancellationToken,
    ) {
        let refresh_interval = refresh_interval.max(Duration::from_secs(1));
        let factory = MinerFactory::new();
        let mut interval = tokio::time::interval(refresh_interval);

        info!(
            "Starting SV1 miner telemetry loop with interval of {} seconds",
            refresh_interval.as_secs()
        );

        'telemetry: loop {
            tokio::select! {
                _ = cancellation_token.cancelled() => {
                    info!("SV1 miner telemetry loop received shutdown signal");
                    break 'telemetry;
                }
                _ = fallback_token.cancelled() => {
                    info!("SV1 miner telemetry loop received fallback signal");
                    break 'telemetry;
                }
                _ = interval.tick() => {
                    tokio::select! {
                        _ = cancellation_token.cancelled() => {
                            info!("SV1 miner telemetry loop received shutdown signal");
                            break 'telemetry;
                        }
                        _ = fallback_token.cancelled() => {
                            info!("SV1 miner telemetry loop received fallback signal");
                            break 'telemetry;
                        }
                        _ = self.refresh_miner_telemetry(&factory) => {}
                    }
                }
            }
        }
    }

    async fn refresh_miner_telemetry(&self, factory: &MinerFactory) {
        let downstreams = self.current_downstream_connection_ips();
        let active_downstream_ids = downstreams
            .iter()
            .map(|(downstream_id, _)| *downstream_id)
            .collect::<HashSet<_>>();

        self.miner_telemetry
            .retain(|downstream_id, _| active_downstream_ids.contains(downstream_id));

        if downstreams.is_empty() {
            return;
        }

        for (downstream_id, ip) in downstreams {
            match fetch_miner_telemetry(factory, ip).await {
                Some(telemetry) => {
                    if self.downstreams.contains_key(&downstream_id) {
                        self.miner_telemetry.insert(downstream_id, telemetry);
                    }
                }
                None => {
                    self.miner_telemetry.remove(&downstream_id);
                }
            }
        }
    }

    fn current_downstream_connection_ips(&self) -> Vec<(DownstreamId, IpAddr)> {
        self.downstreams
            .iter()
            .filter_map(|downstream| {
                downstream
                    .value()
                    .downstream_data
                    .safe_lock(|data| (*downstream.key(), data.connection_ip))
                    .ok()
            })
            .collect()
    }
}

async fn fetch_miner_telemetry(factory: &MinerFactory, ip: IpAddr) -> Option<MinerTelemetry> {
    let miner = match factory.get_miner(ip).await {
        Ok(Some(miner)) => miner,
        Ok(None) => {
            debug!("No miner management interface found at {ip}");
            return None;
        }
        Err(error) => {
            debug!("Failed to get miner management interface at {ip}: {error}");
            return None;
        }
    };

    Some(miner_data_to_telemetry(
        miner
            .get_data_filtered(MINER_TELEMETRY_EXCLUDED_FIELDS.to_vec())
            .await,
    ))
}

fn miner_data_to_telemetry(data: MinerData) -> MinerTelemetry {
    MinerTelemetry {
        ip: data.ip.to_string(),
        make: Some(data.device_info.make),
        model: Some(data.device_info.model),
        firmware_version: data.firmware_version,
        reported_hashrate_hs: data
            .hashrate
            .map(|hashrate| hashrate.as_unit(HashRateUnit::Hash).value),
        power_consumption_w: data.wattage.map(|power| power.as_watts()),
        efficiency_j_per_th: data.efficiency,
        average_temperature_c: data
            .average_temperature
            .map(|temperature| temperature.as_celsius()),
        uptime_secs: data.uptime.map(|uptime| uptime.as_secs()),
        is_mining: Some(data.is_mining),
    }
}
