use crate::sv1::downstream::downstream::Downstream;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU32, AtomicUsize, Ordering},
        Arc, RwLock,
    },
};
use stratum_apps::{
    stratum_core::{
        bitcoin::Target, channels_sv2::vardiff::classic::VardiffState, mining_sv2::SetNewPrevHash,
        sv1_api::server_to_client,
    },
    utils::types::{ChannelId, DownstreamId, Hashrate, RequestId},
};

#[derive(Debug, Clone)]
pub struct PendingTargetUpdate {
    pub downstream_id: DownstreamId,
    pub new_target: Target,
    pub new_hashrate: Hashrate,
}

#[derive(Debug)]
pub struct Sv1ServerData {
    pub downstreams: HashMap<DownstreamId, Arc<Downstream>>,
    pub request_id_to_downstream_id: HashMap<RequestId, DownstreamId>,
    pub vardiff: HashMap<DownstreamId, Arc<RwLock<VardiffState>>>,
    /// HashMap to store the SetNewPrevHash for each channel
    /// Used in both aggregated and non-aggregated mode
    pub prevhashes: HashMap<ChannelId, SetNewPrevHash<'static>>,
    pub downstream_id_factory: AtomicUsize,
    pub request_id_factory: AtomicU32,
    /// Job storage for aggregated mode - all Sv1 downstreams share the same jobs
    pub aggregated_valid_jobs: Option<Vec<server_to_client::Notify<'static>>>,
    /// Job storage for non-aggregated mode - each Sv1 downstream has its own jobs
    pub non_aggregated_valid_jobs:
        Option<HashMap<ChannelId, Vec<server_to_client::Notify<'static>>>>,
    /// Tracks pending target updates that are waiting for SetTarget response from upstream
    pub pending_target_updates: Vec<PendingTargetUpdate>,
    /// The initial target used when opening channels - used when no downstreams remain
    pub initial_target: Option<Target>,
    /// Counter for generating unique keepalive job IDs
    pub keepalive_job_id_counter: AtomicU32,
}

/// Delimiter used to separate original job ID from keepalive mutation counter.
/// Format: `{original_job_id}#{counter}`
pub const KEEPALIVE_JOB_ID_DELIMITER: char = '#';

impl Sv1ServerData {
    pub fn new(aggregate_channels: bool) -> Self {
        Self {
            downstreams: HashMap::new(),
            request_id_to_downstream_id: HashMap::new(),
            vardiff: HashMap::new(),
            prevhashes: HashMap::new(),
            downstream_id_factory: AtomicUsize::new(1),
            request_id_factory: AtomicU32::new(1),
            aggregated_valid_jobs: aggregate_channels.then(Vec::new),
            non_aggregated_valid_jobs: (!aggregate_channels).then(HashMap::new),
            pending_target_updates: Vec::new(),
            initial_target: None,
            keepalive_job_id_counter: AtomicU32::new(0),
        }
    }

    /// Generates a keepalive job ID by appending a mutation counter to the original job ID.
    /// Format: `{original_job_id}#{counter}` where `#` is the delimiter.
    /// When receiving a share, split on `#` to extract the original job ID.
    pub fn next_keepalive_job_id(&self, original_job_id: &str) -> String {
        let counter = self
            .keepalive_job_id_counter
            .fetch_add(1, Ordering::Relaxed);
        format!("{}#{}", original_job_id, counter)
    }

    /// Extracts the original upstream job ID from a keepalive job ID.
    /// Returns None if the job_id doesn't contain the keepalive delimiter.
    pub fn extract_original_job_id(job_id: &str) -> Option<String> {
        job_id
            .split_once(KEEPALIVE_JOB_ID_DELIMITER)
            .map(|(original, _)| original.to_string())
    }

    /// Returns true if the job_id is a keepalive job (contains the delimiter).
    #[inline]
    pub fn is_keepalive_job_id(job_id: &str) -> bool {
        job_id.contains(KEEPALIVE_JOB_ID_DELIMITER)
    }

    /// Gets the prevhash for a given channel.
    pub fn get_prevhash(&self, channel_id: u32) -> Option<SetNewPrevHash<'static>> {
        self.prevhashes.get(&channel_id).cloned()
    }

    /// Sets the prevhash for a given channel.
    pub fn set_prevhash(&mut self, channel_id: u32, prevhash: SetNewPrevHash<'static>) {
        self.prevhashes.insert(channel_id, prevhash);
    }

    /// Gets the last job from the jobs storage.
    /// In aggregated mode, returns the last job from the shared job list.
    /// In non-aggregated mode, returns the last job for the specified channel.
    pub fn get_last_job(
        &self,
        channel_id: Option<u32>,
    ) -> Option<server_to_client::Notify<'static>> {
        if let Some(jobs) = &self.aggregated_valid_jobs {
            return jobs.last().cloned();
        }
        let channel_jobs = self.non_aggregated_valid_jobs.as_ref()?;
        let ch_id = channel_id?;
        channel_jobs.get(&ch_id)?.last().cloned()
    }

    /// Gets the original upstream job by its job_id.
    /// This is used to find the base time for keepalive time capping.
    pub fn get_original_job(
        &self,
        job_id: &str,
        channel_id: Option<u32>,
    ) -> Option<server_to_client::Notify<'static>> {
        if let Some(jobs) = &self.aggregated_valid_jobs {
            return jobs.iter().find(|j| j.job_id == job_id).cloned();
        }
        let channel_jobs = self.non_aggregated_valid_jobs.as_ref()?;
        let ch_id = channel_id?;
        channel_jobs
            .get(&ch_id)?
            .iter()
            .find(|j| j.job_id == job_id)
            .cloned()
    }
}
