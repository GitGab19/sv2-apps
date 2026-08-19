use crate::{
    error::{self, TproxyError, TproxyErrorKind},
    sv2::Upstream,
};
use stratum_apps::stratum_core::{
    common_messages_sv2::{
        ChannelEndpointChangedOwned, ReconnectOwned, SetupConnectionErrorOwned,
        SetupConnectionSuccessOwned,
    },
    handlers_sv2::HandleCommonMessagesFromServerOwnedAsync,
    parsers_sv2::Tlv,
};
use tracing::{error, info};

#[cfg_attr(not(test), hotpath::measure_all)]
impl HandleCommonMessagesFromServerOwnedAsync for Upstream {
    type Error = TproxyError<error::Upstream>;

    fn get_negotiated_extensions_with_server(
        &self,
        _server_id: Option<usize>,
    ) -> Result<Vec<u16>, Self::Error> {
        self.negotiated_extensions
            .get()
            .map_err(TproxyError::shutdown)
    }

    async fn handle_setup_connection_error(
        &mut self,
        _server_id: Option<usize>,
        msg: SetupConnectionErrorOwned,
        _tlv_fields: Option<&[Tlv]>,
    ) -> Result<(), Self::Error> {
        error!("Received: {}", msg);
        Err(TproxyError::fallback(TproxyErrorKind::SetupConnectionError))
    }

    async fn handle_setup_connection_success(
        &mut self,
        _server_id: Option<usize>,
        msg: SetupConnectionSuccessOwned,
        _tlv_fields: Option<&[Tlv]>,
    ) -> Result<(), Self::Error> {
        info!("Received: {}", msg);
        Ok(())
    }

    async fn handle_channel_endpoint_changed(
        &mut self,
        _server_id: Option<usize>,
        msg: ChannelEndpointChangedOwned,
        _tlv_fields: Option<&[Tlv]>,
    ) -> Result<(), Self::Error> {
        info!("Received: {}", msg);
        let extensions_to_renegotiate = self
            .negotiated_extensions
            .with(|negotiated_extensions| {
                let mut extensions = self.required_extensions.clone();
                for extension in negotiated_extensions.drain(..) {
                    if !extensions.contains(&extension) {
                        extensions.push(extension);
                    }
                }
                extensions
            })
            .map_err(TproxyError::shutdown)?;

        if extensions_to_renegotiate.is_empty() {
            return Ok(());
        }

        self.request_extensions(extensions_to_renegotiate).await
    }

    async fn handle_reconnect(
        &mut self,
        _server_id: Option<usize>,
        msg: ReconnectOwned,
        _tlv_fields: Option<&[Tlv]>,
    ) -> Result<(), Self::Error> {
        info!("Received: {}", msg);
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{error::Action, sv2::upstream::UpstreamIo};
    use async_channel::unbounded;
    use std::sync::{Arc, atomic::AtomicU16};
    use stratum_apps::{
        stratum_core::{
            extensions_sv2::EXTENSION_TYPE_EXTENSIONS_NEGOTIATION,
            framing_sv2::SV2_FRAME_HEADER_SIZE,
            parsers_sv2::{Extensions, ExtensionsNegotiation},
        },
        sync::SharedLock,
        utils::types::Sv2Frame,
    };

    fn upstream() -> Upstream {
        upstream_with_extensions(vec![], vec![]).0
    }

    fn upstream_with_extensions(
        required_extensions: Vec<u16>,
        negotiated_extensions: Vec<u16>,
    ) -> (
        Upstream,
        async_channel::Receiver<Sv2Frame>,
        SharedLock<Vec<u16>>,
    ) {
        let (_upstream_inbound_sender, upstream_receiver) = unbounded();
        let (upstream_sender, upstream_outbound_receiver) = unbounded();
        let (channel_manager_sender, _channel_manager_receiver) = unbounded();
        let (_channel_manager_sender, channel_manager_receiver) = unbounded();
        let negotiated_extensions = SharedLock::new(negotiated_extensions);

        (
            Upstream {
                upstream_io: UpstreamIo::new(
                    upstream_receiver,
                    upstream_sender,
                    channel_manager_sender,
                    channel_manager_receiver,
                ),
                required_extensions,
                negotiated_extensions: negotiated_extensions.clone(),
                next_extension_request_id: Arc::new(AtomicU16::new(1)),
                address: "127.0.0.1:3333".parse().unwrap(),
            },
            upstream_outbound_receiver,
            negotiated_extensions,
        )
    }

    #[tokio::test]
    async fn channel_endpoint_changed_resets_and_renegotiates_extensions() {
        let (mut upstream, outbound_receiver, negotiated_extensions) =
            upstream_with_extensions(vec![2], vec![2]);

        upstream
            .handle_channel_endpoint_changed(
                None,
                ChannelEndpointChangedOwned { channel_id: 1 },
                None,
            )
            .await
            .unwrap();

        assert!(negotiated_extensions.get().unwrap().is_empty());

        let frame = outbound_receiver.recv().await.unwrap();
        let (extension_type, message_type) = {
            let header = frame.get_header().unwrap();
            (header.ext_type(), header.msg_type())
        };
        assert_eq!(extension_type, EXTENSION_TYPE_EXTENSIONS_NEGOTIATION);

        let mut encoded = vec![0; frame.encoded_length()];
        frame.serialize(&mut encoded).unwrap();
        let message = Extensions::try_from((
            extension_type,
            message_type,
            &mut encoded[SV2_FRAME_HEADER_SIZE..],
        ))
        .unwrap();
        let Extensions::ExtensionsNegotiation(ExtensionsNegotiation::RequestExtensions(request)) =
            message
        else {
            panic!("expected RequestExtensions");
        };
        assert_eq!(request.requested_extensions.into_inner(), vec![2]);
    }
}
