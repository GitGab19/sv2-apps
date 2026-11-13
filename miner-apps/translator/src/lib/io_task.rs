use std::sync::Arc;

use async_channel::{Receiver, Sender};
use stratum_apps::{
    channel_utils::ReceiverCleanup,
    fallback_coordinator::FallbackCoordinator,
    network_helpers::{
        Error as NetworkError,
        noise_stream::{NoiseTcpReadHalf, NoiseTcpWriteHalf},
    },
    stratum_core::framing_sv2::header::Header,
    task_manager::TaskManager,
    utils::types::{InboundFrame, OutboundFrame},
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, trace};

#[cfg_attr(not(test), hotpath::measure)]
#[track_caller]
#[allow(clippy::too_many_arguments)]
pub fn spawn_io_tasks(
    task_manager: Arc<TaskManager>,
    mut reader: NoiseTcpReadHalf,
    mut writer: NoiseTcpWriteHalf,
    outbound_rx: Receiver<OutboundFrame>,
    inbound_tx: Sender<InboundFrame>,
    cancellation_token: CancellationToken,
    fallback_coordinator: FallbackCoordinator,
) {
    let inbound_tx_clone = inbound_tx.clone();
    let outbound_rx_clone = outbound_rx.clone();

    {
        let cancellation_token_clone = cancellation_token.clone();
        let fallback_token = fallback_coordinator.token();
        let reader_task = async move {
            trace!("Reader task started");
            loop {
                tokio::select! {
                    biased;
                    _ = cancellation_token_clone.cancelled() => {
                        trace!("Received app shutdown signal");
                        inbound_tx.close();
                        break;
                    }
                    _ = fallback_token.cancelled() => {
                        trace!("Received fallback signal");
                        inbound_tx.close();
                        break;
                    }
                    res = reader.read_frame() => {
                        match res {
                            Ok(sv2_frame) => {
                                trace!(
                                    "Read frame from SV2 server: {}",
                                    frame_summary(sv2_frame.header())
                                );
                                if let Err(e) = inbound_tx.send(sv2_frame).await {
                                    inbound_tx.close();
                                    trace!(error = ?e, "Inbound channel closed, stopping reader");
                                    break;
                                }
                            }
                            Err(NetworkError::SocketClosed) => {
                                debug!("SV2 server closed the connection");
                                inbound_tx.close();
                                break;
                            }
                            Err(e) => {
                                error!(error = %e, "Failed to read or decode frame from peer; terminating connection");
                                inbound_tx.close();
                                break;
                            }
                        }
                    }
                }
            }
            inbound_tx.close();
            outbound_rx_clone.close_and_drain();
            drop(inbound_tx);
            drop(outbound_rx_clone);

            trace!("Reader task exited");
        };
        task_manager.spawn(fallback_coordinator.track(reader_task));
    }

    {
        let fallback_token = fallback_coordinator.token();
        let writer_task = async move {
            trace!("Writer task started");
            loop {
                tokio::select! {
                    biased;
                    _ = cancellation_token.cancelled() => {
                        trace!("Received app shutdown signal");
                        inbound_tx_clone.close();
                        break;
                    }
                    _ = fallback_token.cancelled() => {
                        trace!("Received fallback signal");
                        inbound_tx_clone.close();
                        break;
                    }
                    res = outbound_rx.recv() => {
                        match res {
                            Ok(frame) => {
                                trace!(
                                    "Writing frame to SV2 server: {}",
                                    frame_summary(outbound_header(&frame))
                                );
                                match writer.write_frame(frame).await {
                                    Ok(()) => {}
                                    Err(NetworkError::SocketClosed) => {
                                        debug!("SV2 server closed the connection while tProxy was writing to it");
                                        outbound_rx.close_and_drain();
                                        break;
                                    }
                                    Err(e) => {
                                        error!(error = %e, "Failed to encode or send frame to peer; terminating connection");
                                        outbound_rx.close_and_drain();
                                        break;
                                    }
                                }
                            }
                            Err(_) => {
                                outbound_rx.close_and_drain();
                                trace!("Outbound channel closed");
                                break;
                            }
                        }
                    }
                }
            }
            outbound_rx.close_and_drain();
            inbound_tx_clone.close();
            drop(outbound_rx);
            drop(inbound_tx_clone);

            trace!("Writer task exited");
        };
        task_manager.spawn(fallback_coordinator.track(writer_task));
    }
}

// Describes a frame by its header, for the TRACE lines that mark each frame read from or written
// to the SV2 server. The channel-message bit travels in the extension type field but is not part
// of the extension, so it is named on its own.
fn frame_summary(header: Header) -> String {
    format!(
        "extension 0x{:04x}, message type 0x{:02x}{}, payload {} bytes",
        header.ext_type_without_channel_msg(),
        header.msg_type(),
        if header.channel_msg() {
            ", channel message"
        } else {
            ""
        },
        header.payload_length()
    )
}

// The header of a frame on its way out, whether the encoder serializes it or it is already
// serialized.
fn outbound_header(frame: &OutboundFrame) -> Header {
    match frame {
        OutboundFrame::Message(frame) => frame.header(),
        OutboundFrame::Raw(frame) => frame.header(),
    }
}
