use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU32},
        Arc,
    },
};

use async_channel::{unbounded, Receiver, Sender};
use stratum_apps::{
    custom_mutex::Mutex,
    network_helpers::noise_stream::NoiseTcpStream,
    stratum_core::{
        channels_sv2::server::{
            extended::ExtendedChannel,
            group::GroupChannel,
            jobs::{extended::ExtendedJob, job_store::DefaultJobStore, standard::StandardJob},
            standard::StandardChannel,
        },
        common_messages_sv2::MESSAGE_TYPE_SETUP_CONNECTION,
        framing_sv2,
        handlers_sv2::{HandleCommonMessagesFromClientAsync, HandleExtensionsFromClientAsync},
        noise_sv2::Error,
        parsers_sv2::{parse_message_frame_with_tlvs, AnyMessage, Mining, Tlv},
    },
    task_manager::TaskManager,
    utils::{
        protocol_message_type::{protocol_message_type, MessageType},
        types::{ChannelId, DownstreamId, Message, Sv2Frame},
    },
};
use tokio::sync::broadcast;
use tracing::{debug, error, warn};

use crate::{
    error::{PoolError, PoolResult},
    io_task::spawn_io_tasks,
    status::{handle_error, Status, StatusSender},
    utils::ShutdownMessage,
};

mod common_message_handler;
mod extensions_message_handler;

/// Holds state related to a downstream connection's mining channels.
///
/// This includes:
/// - Whether the downstream requires a standard job (`require_std_job`).
/// - An optional [`GroupChannel`] if group channeling is used.
/// - Active [`ExtendedChannel`]s keyed by channel ID.
/// - Active [`StandardChannel`]s keyed by channel ID.
/// - Extensions that have been successfully negotiated with this client
/// - Per-channel coinbase outputs for solo mining (channel_id -> coinbase_output)
pub struct DownstreamData {
    pub group_channels: Option<GroupChannel<'static, DefaultJobStore<ExtendedJob<'static>>>>,
    pub extended_channels:
        HashMap<ChannelId, ExtendedChannel<'static, DefaultJobStore<ExtendedJob<'static>>>>,
    pub standard_channels:
        HashMap<ChannelId, StandardChannel<'static, DefaultJobStore<StandardJob<'static>>>>,
    pub channel_id_factory: AtomicU32,
    /// Extensions that have been successfully negotiated with this client
    pub negotiated_extensions: Vec<u16>,
    /// Per-channel coinbase outputs for solo mining channels
    pub solo_coinbase_outputs: HashMap<ChannelId, Vec<u8>>,
}

/// Communication layer for a downstream connection.
///
/// Provides the messaging primitives for interacting with the
/// channel manager and the downstream peer:
/// - `channel_manager_sender`: sends frames to the channel manager.
/// - `channel_manager_receiver`: receives messages from the channel manager.
/// - `downstream_sender`: sends frames to the downstream.
/// - `downstream_receiver`: receives frames from the downstream.
#[derive(Clone)]
pub struct DownstreamChannel {
    channel_manager_sender: Sender<(DownstreamId, Mining<'static>, Option<Vec<Tlv>>)>,
    channel_manager_receiver: broadcast::Sender<(DownstreamId, Mining<'static>, Option<Vec<Tlv>>)>,
    downstream_sender: Sender<Sv2Frame>,
    downstream_receiver: Receiver<Sv2Frame>,
}

/// Represents a downstream client connected to this node.
#[derive(Clone)]
pub struct Downstream {
    pub downstream_data: Arc<Mutex<DownstreamData>>,
    downstream_channel: DownstreamChannel,
    pub downstream_id: usize,
    pub requires_standard_jobs: Arc<AtomicBool>,
    pub requires_custom_work: Arc<AtomicBool>,
    /// Extensions that the pool supports
    pub supported_extensions: Vec<u16>,
    /// Extensions that the pool requires
    pub required_extensions: Vec<u16>,
}

impl Downstream {
    /// Creates a new [`Downstream`] instance and spawns the necessary I/O tasks.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        downstream_id: DownstreamId,
        channel_manager_sender: Sender<(DownstreamId, Mining<'static>, Option<Vec<Tlv>>)>,
        channel_manager_receiver: broadcast::Sender<(
            DownstreamId,
            Mining<'static>,
            Option<Vec<Tlv>>,
        )>,
        noise_stream: NoiseTcpStream<Message>,
        notify_shutdown: broadcast::Sender<ShutdownMessage>,
        task_manager: Arc<TaskManager>,
        status_sender: Sender<Status>,
        supported_extensions: Vec<u16>,
        required_extensions: Vec<u16>,
    ) -> Self {
        let (noise_stream_reader, noise_stream_writer) = noise_stream.into_split();
        let status_sender = StatusSender::Downstream {
            downstream_id,
            tx: status_sender,
        };
        let (inbound_tx, inbound_rx) = unbounded::<Sv2Frame>();
        let (outbound_tx, outbound_rx) = unbounded::<Sv2Frame>();
        spawn_io_tasks(
            task_manager,
            noise_stream_reader,
            noise_stream_writer,
            outbound_rx,
            inbound_tx,
            notify_shutdown,
            status_sender,
        );

        let downstream_channel = DownstreamChannel {
            channel_manager_receiver,
            channel_manager_sender,
            downstream_sender: outbound_tx,
            downstream_receiver: inbound_rx,
        };
        let downstream_data = Arc::new(Mutex::new(DownstreamData {
            extended_channels: HashMap::new(),
            standard_channels: HashMap::new(),
            group_channels: None,
            channel_id_factory: AtomicU32::new(1),
            negotiated_extensions: vec![],
            solo_coinbase_outputs: HashMap::new(),
        }));
        Downstream {
            downstream_channel,
            downstream_data,
            downstream_id,
            requires_standard_jobs: Arc::new(AtomicBool::new(false)),
            requires_custom_work: Arc::new(AtomicBool::new(false)),
            supported_extensions,
            required_extensions,
        }
    }

    /// Starts the downstream loop.
    ///
    /// Responsibilities:
    /// - Performs the initial `SetupConnection` handshake with the downstream.
    /// - Forwards mining-related messages to the channel manager.
    /// - Forwards channel manager messages back to the downstream peer.
    pub async fn start(
        mut self,
        notify_shutdown: broadcast::Sender<ShutdownMessage>,
        status_sender: Sender<Status>,
        task_manager: Arc<TaskManager>,
    ) {
        let status_sender = StatusSender::Downstream {
            downstream_id: self.downstream_id,
            tx: status_sender,
        };

        let mut shutdown_rx = notify_shutdown.subscribe();

        // Setup initial connection
        if let Err(e) = self.setup_connection_with_downstream().await {
            error!(?e, "Failed to set up downstream connection");
            handle_error(&status_sender, e).await;
            return;
        }

        let mut receiver = self.downstream_channel.channel_manager_receiver.subscribe();
        task_manager.spawn(async move {
            loop {
                let mut self_clone_1 = self.clone();
                let downstream_id = self_clone_1.downstream_id;
                let self_clone_2 = self.clone();
                tokio::select! {
                    message = shutdown_rx.recv() => {
                        match message {
                            Ok(ShutdownMessage::ShutdownAll) => {
                                debug!("Downstream {downstream_id}: Received global shutdown");
                                break;
                            }
                            Ok(ShutdownMessage::DownstreamShutdown(id)) if downstream_id == id => {
                                debug!("Downstream {downstream_id}: Received downstream {id} shutdown");
                                break;
                            }
                            _ => {}
                        }
                    }
                    res = self_clone_1.handle_downstream_message() => {
                        if let Err(e) = res {
                            error!(?e, "Error handling downstream message for {downstream_id}");
                            handle_error(&status_sender, e).await;
                            break;
                        }
                    }
                    res = self_clone_2.handle_channel_manager_message(&mut receiver) => {
                        if let Err(e) = res {
                            error!(?e, "Error handling channel manager message for {downstream_id}");
                            handle_error(&status_sender, e).await;
                            break;
                        }
                    }

                }
            }
            warn!("Downstream: unified message loop exited.");
        });
    }

    // Performs the initial handshake with a downstream peer.
    async fn setup_connection_with_downstream(&mut self) -> PoolResult<()> {
        let mut frame = self.downstream_channel.downstream_receiver.recv().await?;
        let header = frame.get_header().ok_or_else(|| {
            error!("SV2 frame missing header");
            PoolError::Framing(framing_sv2::Error::MissingHeader)
        })?;
        // The first ever message received on a new downstream connection
        // should always be a setup connection message.
        if header.msg_type() == MESSAGE_TYPE_SETUP_CONNECTION {
            self.handle_common_message_frame_from_client(None, header, frame.payload())
                .await?;
            return Ok(());
        }
        Err(PoolError::UnexpectedMessage(
            header.ext_type_without_channel_msg(),
            header.msg_type(),
        ))
    }

    // Handles messages sent from the channel manager to this downstream.
    async fn handle_channel_manager_message(
        self,
        receiver: &mut broadcast::Receiver<(DownstreamId, Mining<'static>, Option<Vec<Tlv>>)>,
    ) -> PoolResult<()> {
        let (downstream_id, msg, _tlv_fields) = match receiver.recv().await {
            Ok(msg) => msg,
            Err(e) => {
                warn!(?e, "Broadcast receive failed");
                return Ok(());
            }
        };

        if downstream_id != self.downstream_id {
            debug!(
                ?downstream_id,
                "Message ignored for non-matching downstream"
            );
            return Ok(());
        }

        let message = AnyMessage::Mining(msg);
        let std_frame: Sv2Frame = message.try_into()?;

        self.downstream_channel
            .downstream_sender
            .send(std_frame)
            .await
            .map_err(|e| {
                error!(?e, "Downstream send failed");
                PoolError::Noise(Error::ExpectedIncomingHandshakeMessage)
            })?;

        Ok(())
    }

    // Handles incoming messages from the downstream peer.
    async fn handle_downstream_message(&mut self) -> PoolResult<()> {
        let mut sv2_frame = self.downstream_channel.downstream_receiver.recv().await?;
        let header = sv2_frame.get_header().ok_or_else(|| {
            error!("SV2 frame missing header");
            PoolError::Framing(framing_sv2::Error::MissingHeader)
        })?;

        match protocol_message_type(header.ext_type(), header.msg_type()) {
            MessageType::Mining => {
                debug!("Received mining SV2 frame from downstream.");
                let negotiated_extensions = self
                    .downstream_data
                    .super_safe_lock(|data| data.negotiated_extensions.clone());
                let (any_message, tlv_fields) = parse_message_frame_with_tlvs(
                    header,
                    sv2_frame.payload(),
                    &negotiated_extensions,
                )?;
                let mining_message = match any_message {
                    AnyMessage::Mining(msg) => msg,
                    _ => {
                        error!("Expected Mining message but got different type");
                        return Err(PoolError::UnexpectedMessage(
                            header.ext_type_without_channel_msg(),
                            header.msg_type(),
                        ));
                    }
                };
                self.downstream_channel
                    .channel_manager_sender
                    .send((self.downstream_id, mining_message, tlv_fields))
                    .await
                    .map_err(|e| {
                        error!(?e, "Failed to send mining message to channel manager.");
                        PoolError::ChannelErrorSender
                    })?;
            }
            MessageType::Extensions => {
                self.handle_extensions_message_frame_from_client(None, header, sv2_frame.payload())
                    .await?;
            }
            MessageType::Common
            | MessageType::JobDeclaration
            | MessageType::TemplateDistribution => {
                warn!(
                    ext_type = ?header.ext_type(),
                    msg_type = ?header.msg_type(),
                    "Received unexpected message from downstream."
                );
            }
            MessageType::Unknown => {
                warn!(
                    ext_type = ?header.ext_type(),
                    msg_type = ?header.msg_type(),
                    "Received unknown message from downstream."
                );
            }
        }

        Ok(())
    }
}
