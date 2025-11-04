use crate::{downstream::Downstream, error::PoolError};
use std::convert::TryInto;
use stratum_apps::{
    stratum_core::{
        binary_sv2::Seq064K,
        extensions_sv2::{RequestExtensions, RequestExtensionsError, RequestExtensionsSuccess},
        handlers_sv2::HandleExtensionsFromClientAsync,
        parsers_sv2::{AnyMessage, Tlv},
    },
    utils::types::Sv2Frame,
};
use tracing::{error, info};

impl HandleExtensionsFromClientAsync for Downstream {
    type Error = PoolError;

    fn get_negotiated_extensions_with_client(&self, _client_id: Option<usize>) -> Vec<u16> {
        self.downstream_data
            .super_safe_lock(|data| data.negotiated_extensions.clone())
    }

    async fn handle_request_extensions(
        &mut self,
        _client_id: Option<usize>,
        msg: RequestExtensions<'_>,
        _tlv_fields: Option<&[Tlv]>,
    ) -> Result<(), Self::Error> {
        let requested: Vec<u16> = msg.requested_extensions.clone().into_inner();

        info!(
            "Downstream {}: Received RequestExtensions: request_id={}, requested={:?}",
            self.downstream_id, msg.request_id, requested
        );

        // Get supported and required extensions from downstream data
        let (supported_extensions, required_extensions) =
            self.downstream_data.super_safe_lock(|data| {
                (
                    data.supported_extensions.clone(),
                    data.required_extensions.clone(),
                )
            });

        // Determine which requested extensions we support
        let mut supported: Vec<u16> = Vec::new();
        let mut unsupported: Vec<u16> = Vec::new();

        for ext in &requested {
            if supported_extensions.contains(ext) {
                supported.push(*ext);
            } else {
                unsupported.push(*ext);
            }
        }

        // Check which required extensions the client didn't request
        let missing_required: Vec<u16> = required_extensions
            .iter()
            .filter(|ext| !requested.contains(ext))
            .copied()
            .collect();

        // Determine response based on spec rules:
        // - Success: If at least one extension is supported AND all required extensions are present
        // - Error: If no extensions are supported OR required extensions are missing
        let should_send_error = supported.is_empty() || !missing_required.is_empty();

        if should_send_error {
            // Send error response
            error!(
                "Downstream {}: Extension negotiation error: requested={:?}, supported={:?}, unsupported={:?}, missing_required={:?}",
                self.downstream_id, requested, supported, unsupported, missing_required
            );

            let error = RequestExtensionsError {
                request_id: msg.request_id,
                unsupported_extensions: Seq064K::new(unsupported)
                    .map_err(|_| PoolError::InvalidUnsupportedExtensionsSequence)?,
                required_extensions: Seq064K::new(missing_required.clone())
                    .map_err(|_| PoolError::InvalidRequiredExtensionsSequence)?,
            };

            let frame: Sv2Frame = AnyMessage::Extensions(error.into_static().into()).try_into()?;
            self.downstream_channel
                .downstream_sender
                .send(frame)
                .await?;

            // If required extensions are missing, the server SHOULD disconnect the client
            if !missing_required.is_empty() {
                error!(
                    "Downstream {}: Client does not support required extensions {:?}. Server MUST disconnect.",
                    self.downstream_id, missing_required
                );
                // TODO: Disconnect the client
            }
        } else {
            // Send success response with the subset of extensions we both support
            info!(
                "Downstream {}: Extension negotiation success: requested={:?}, negotiated={:?}",
                self.downstream_id, requested, supported
            );

            // Store the negotiated extensions in the shared downstream data
            self.downstream_data.super_safe_lock(|data| {
                data.negotiated_extensions = supported.clone();
            });

            let success = RequestExtensionsSuccess {
                request_id: msg.request_id,
                supported_extensions: Seq064K::new(supported.clone())
                    .map_err(|_| PoolError::InvalidSupportedExtensionsSequence)?,
            };

            let frame: Sv2Frame =
                AnyMessage::Extensions(success.into_static().into()).try_into()?;
            self.downstream_channel
                .downstream_sender
                .send(frame)
                .await?;

            info!(
                "Downstream {}: Stored negotiated extensions: {:?}",
                self.downstream_id, supported
            );
        }

        Ok(())
    }
}
