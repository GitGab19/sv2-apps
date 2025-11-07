use stratum_apps::stratum_core::{
    common_messages_sv2::{
        ChannelEndpointChanged, Reconnect, SetupConnectionError, SetupConnectionSuccess,
    },
    handlers_sv2::HandleCommonMessagesFromServerAsync,
    parsers_sv2::Tlv,
};
use tracing::{info, warn};

use crate::{
    error::JDCError,
    jd_mode::{set_jd_mode, JdMode},
    job_declarator::JobDeclarator,
};

impl HandleCommonMessagesFromServerAsync for JobDeclarator {
    type Error = JDCError;

    fn get_negotiated_extensions_with_server(&self, _server_id: Option<usize>) -> Vec<u16> {
        vec![]
    }

    async fn handle_setup_connection_success(
        &mut self,
        _server_id: Option<usize>,
        msg: SetupConnectionSuccess,
        _tlv_fields: Option<&[Tlv]>,
    ) -> Result<(), Self::Error> {
        info!("Received: {}", msg);

        let jd_mode = match msg.flags {
            0 => JdMode::CoinbaseOnly,
            1 => JdMode::FullTemplate,
            _ => JdMode::SoloMining,
        };
        set_jd_mode(jd_mode);

        if jd_mode == JdMode::SoloMining {
            return Err(JDCError::Shutdown);
        }

        Ok(())
    }

    async fn handle_channel_endpoint_changed(
        &mut self,
        _server_id: Option<usize>,
        msg: ChannelEndpointChanged,
        _tlv_fields: Option<&[Tlv]>,
    ) -> Result<(), Self::Error> {
        info!("Received: {}", msg);
        Ok(())
    }

    async fn handle_reconnect(
        &mut self,
        _server_id: Option<usize>,
        msg: Reconnect<'_>,
        _tlv_fields: Option<&[Tlv]>,
    ) -> Result<(), Self::Error> {
        info!("Received: {}", msg);
        Ok(())
    }

    async fn handle_setup_connection_error(
        &mut self,
        _server_id: Option<usize>,
        msg: SetupConnectionError<'_>,
        _tlv_fields: Option<&[Tlv]>,
    ) -> Result<(), Self::Error> {
        warn!("Received: {}", msg);
        Err(JDCError::Shutdown)
    }
}
