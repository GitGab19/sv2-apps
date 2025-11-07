//! ## Error Module
//!
//! Defines [`Error`], the central error enum used throughout the Job Declarator Client (JDC).
//!
//! It unifies errors from:
//! - I/O operations
//! - Channels (send/recv)
//! - SV2 stack: Binary, Codec, Noise, Framing, RolesLogic
//! - Locking logic (PoisonError)
//! - Domain-specific issues
//!
//! This module ensures that all errors can be passed around consistently, including across async
//! boundaries.
use ext_config::ConfigError;
use std::fmt;
use stratum_apps::{
    network_helpers,
    stratum_core::{
        binary_sv2, bitcoin,
        channels_sv2::{
            client::error::ExtendedChannelError as ExtendedChannelClientError,
            server::error::{
                ExtendedChannelError as ExtendedChannelServerError, GroupChannelError,
                StandardChannelError,
            },
        },
        framing_sv2,
        handlers_sv2::HandlerErrorType,
        mining_sv2::ExtendedExtranonceError,
        noise_sv2,
        parsers_sv2::ParserError,
    },
    utils::types::{DownstreamId, JobId, RequestId, TemplateId, VardiffKey},
};
use tokio::{sync::broadcast, time::error::Elapsed};

#[derive(Debug)]
pub enum ChannelSv2Error {
    ExtendedChannelClientSide(ExtendedChannelClientError),
    ExtendedChannelServerSide(ExtendedChannelServerError),
    StandardChannelServerSide(StandardChannelError),
    GroupChannelServerSide(GroupChannelError),
}

#[derive(Debug)]
pub enum JDCError {
    /// Errors on bad CLI argument input.
    BadCliArgs,
    /// Errors on bad `config` TOML deserialize.
    BadConfigDeserialize(ConfigError),
    /// Errors from `binary_sv2` crate.
    BinarySv2(binary_sv2::Error),
    /// Errors on bad noise handshake.
    CodecNoise(noise_sv2::Error),
    /// Errors from `framing_sv2` crate.
    FramingSv2(framing_sv2::Error),
    /// Errors on bad `TcpStream` connection.
    Io(std::io::Error),
    /// Errors on bad `String` to `int` conversion.
    ParseInt(std::num::ParseIntError),
    Parser(ParserError),
    /// Channel receiver error
    ChannelErrorReceiver(async_channel::RecvError),
    /// Channel sender error
    ChannelErrorSender,
    /// Broadcast channel receiver error
    BroadcastChannelErrorReceiver(broadcast::error::RecvError),
    /// Shutdown
    Shutdown,
    /// Network helpers error
    NetworkHelpersError(network_helpers::Error),
    /// Unexpected message
    UnexpectedMessage(u16, u8),
    /// Invalid user identity
    InvalidUserIdentity(String),
    /// Bitcoin encode error
    BitcoinEncodeError(bitcoin::consensus::encode::Error),
    /// Invalid socket address
    InvalidSocketAddress(String),
    /// Timeout error
    Timeout,
    /// Declared job corresponding to request Id not found.
    LastDeclareJobNotFound(RequestId),
    /// No active job with job id
    ActiveJobNotFound(JobId),
    /// No active token
    TokenNotFound,
    /// Template not found with template ID
    TemplateNotFound(TemplateId),
    /// Downstream not found with downstream ID
    DownstreamNotFound(DownstreamId),
    /// Future template not present
    FutureTemplateNotPresent,
    /// Last new prevhash not found
    LastNewPrevhashNotFound,
    /// Vardiff not found
    VardiffNotFound(VardiffKey),
    /// Tx data error
    TxDataError,
    /// Frame conversion error
    FrameConversionError,
    /// Failed to create custom Job
    FailedToCreateCustomJob,
    /// Allocate Mining job token coinbase output error
    AllocateMiningJobTokenSuccessCoinbaseOutputsError,
    /// Channel manager has bad coinbase outputs.
    ChannelManagerHasBadCoinbaseOutputs,
    /// Declared job has bad coinbase outputs.
    DeclaredJobHasBadCoinbaseOutputs,
    /// Extranonce size is too large
    ExtranonceSizeTooLarge,
    /// Could not create group channel
    FailedToCreateGroupChannel(GroupChannelError),
    ///Channel Errors
    ChannelSv2(ChannelSv2Error),
    /// Extranonce prefix error
    ExtranoncePrefixFactoryError(ExtendedExtranonceError),
    /// Invalid unsupported extensions sequence (exceeds maximum length)
    InvalidUnsupportedExtensionsSequence,
    /// Invalid required extensions sequence (exceeds maximum length)
    InvalidRequiredExtensionsSequence,
    /// Invalid supported extensions sequence (exceeds maximum length)
    InvalidSupportedExtensionsSequence,
    /// Server does not support required extensions
    RequiredExtensionsNotSupported(Vec<u16>),
    /// Server requires extensions that the translator doesn't support
    ServerRequiresUnsupportedExtensions(Vec<u16>),
}

impl std::error::Error for JDCError {}

impl fmt::Display for JDCError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use JDCError::*;
        match self {
            BadCliArgs => write!(f, "Bad CLI arg input"),
            BadConfigDeserialize(ref e) => write!(f, "Bad `config` TOML deserialize: `{e:?}`"),
            BinarySv2(ref e) => write!(f, "Binary SV2 error: `{e:?}`"),
            CodecNoise(ref e) => write!(f, "Noise error: `{e:?}"),
            FramingSv2(ref e) => write!(f, "Framing SV2 error: `{e:?}`"),
            Io(ref e) => write!(f, "I/O error: `{e:?}"),
            ParseInt(ref e) => write!(f, "Bad convert from `String` to `int`: `{e:?}`"),
            ChannelErrorReceiver(ref e) => write!(f, "Channel receive error: `{e:?}`"),
            Parser(ref e) => write!(f, "Parser error: `{e:?}`"),
            BroadcastChannelErrorReceiver(ref e) => {
                write!(f, "Broadcast channel receive error: {e:?}")
            }
            ChannelErrorSender => write!(f, "Sender error"),
            Shutdown => write!(f, "Shutdown"),
            NetworkHelpersError(ref e) => write!(f, "Network error: {e:?}"),
            UnexpectedMessage(extension_type, message_type) => {
                write!(f, "Unexpected Message: {extension_type} {message_type}")
            }
            InvalidUserIdentity(_) => write!(f, "User ID is invalid"),
            BitcoinEncodeError(_) => write!(f, "Error generated during encoding"),
            InvalidSocketAddress(ref s) => write!(f, "Invalid socket address: {s}"),
            Timeout => write!(f, "Time out error"),
            LastDeclareJobNotFound(request_id) => {
                write!(f, "last declare job not found for request id: {request_id}")
            }
            ActiveJobNotFound(request_id) => {
                write!(f, "Active Job not found for request_id: {request_id}")
            }
            TokenNotFound => {
                write!(f, "Token Not found")
            }
            TemplateNotFound(template_id) => {
                write!(f, "Template not found, template_id: {template_id}")
            }
            DownstreamNotFound(downstream_id) => {
                write!(
                    f,
                    "Downstream not found with downstream_id: {downstream_id}"
                )
            }
            FutureTemplateNotPresent => {
                write!(f, "Future template not present")
            }
            LastNewPrevhashNotFound => {
                write!(f, "Last new prevhash not found")
            }
            VardiffNotFound(vardiff_key) => {
                write!(f, "Vardiff not found for vardiff key: {vardiff_key:?}")
            }
            TxDataError => {
                write!(f, "Transaction data error")
            }
            FrameConversionError => {
                write!(f, "Could not convert message to frame")
            }
            FailedToCreateCustomJob => {
                write!(f, "failed to create custom job")
            }
            AllocateMiningJobTokenSuccessCoinbaseOutputsError => {
                write!(
                    f,
                    "AllocateMiningJobToken.Success coinbase outputs are not deserializable"
                )
            }
            ChannelManagerHasBadCoinbaseOutputs => {
                write!(f, "Channel Manager coinbase outputs are not deserializable")
            }
            DeclaredJobHasBadCoinbaseOutputs => {
                write!(f, "Declared job coinbase outputs are not deserializable")
            }
            ExtranonceSizeTooLarge => {
                write!(f, "Extranonce size too large")
            }
            FailedToCreateGroupChannel(ref e) => {
                write!(f, "Failed to create group channel: {e:?}")
            }
            ExtranoncePrefixFactoryError(e) => {
                write!(f, "Failed to create ExtranoncePrefixFactory: {e:?}")
            }
            ChannelSv2(channel_error) => {
                write!(f, "Channel error: {channel_error:?}")
            }
            InvalidUnsupportedExtensionsSequence => {
                write!(
                    f,
                    "Invalid unsupported extensions sequence (exceeds maximum length)"
                )
            }
            InvalidRequiredExtensionsSequence => {
                write!(
                    f,
                    "Invalid required extensions sequence (exceeds maximum length)"
                )
            }
            InvalidSupportedExtensionsSequence => {
                write!(
                    f,
                    "Invalid supported extensions sequence (exceeds maximum length)"
                )
            }
            RequiredExtensionsNotSupported(extensions) => {
                write!(
                    f,
                    "Server does not support required extensions: {extensions:?}"
                )
            }
            ServerRequiresUnsupportedExtensions(extensions) => {
                write!(f, "Server requires extensions that the translator doesn't support: {extensions:?}")
            }
        }
    }
}

impl JDCError {
    fn is_non_critical_variant(&self) -> bool {
        matches!(
            self,
            JDCError::LastNewPrevhashNotFound
                | JDCError::FutureTemplateNotPresent
                | JDCError::LastDeclareJobNotFound(_)
                | JDCError::ActiveJobNotFound(_)
                | JDCError::TokenNotFound
                | JDCError::TemplateNotFound(_)
                | JDCError::DownstreamNotFound(_)
                | JDCError::VardiffNotFound(_)
                | JDCError::TxDataError
                | JDCError::FrameConversionError
                | JDCError::FailedToCreateCustomJob
                | JDCError::RequiredExtensionsNotSupported(_)
                | JDCError::ServerRequiresUnsupportedExtensions(_)
        )
    }

    /// Adds basic priority to error types:
    /// todo: design a better error priority system.
    pub fn is_critical(&self) -> bool {
        if self.is_non_critical_variant() {
            tracing::error!("Non-critical error: {self}");
            return false;
        }

        true
    }
}

impl From<ParserError> for JDCError {
    fn from(e: ParserError) -> Self {
        JDCError::Parser(e)
    }
}

impl From<binary_sv2::Error> for JDCError {
    fn from(e: binary_sv2::Error) -> Self {
        JDCError::BinarySv2(e)
    }
}

impl From<noise_sv2::Error> for JDCError {
    fn from(e: noise_sv2::Error) -> Self {
        JDCError::CodecNoise(e)
    }
}

impl From<framing_sv2::Error> for JDCError {
    fn from(e: framing_sv2::Error) -> Self {
        JDCError::FramingSv2(e)
    }
}

impl From<std::io::Error> for JDCError {
    fn from(e: std::io::Error) -> Self {
        JDCError::Io(e)
    }
}

impl From<std::num::ParseIntError> for JDCError {
    fn from(e: std::num::ParseIntError) -> Self {
        JDCError::ParseInt(e)
    }
}

impl From<ConfigError> for JDCError {
    fn from(e: ConfigError) -> Self {
        JDCError::BadConfigDeserialize(e)
    }
}

impl From<async_channel::RecvError> for JDCError {
    fn from(e: async_channel::RecvError) -> Self {
        JDCError::ChannelErrorReceiver(e)
    }
}

impl From<network_helpers::Error> for JDCError {
    fn from(value: network_helpers::Error) -> Self {
        JDCError::NetworkHelpersError(value)
    }
}

impl From<stratum_apps::stratum_core::bitcoin::consensus::encode::Error> for JDCError {
    fn from(value: stratum_apps::stratum_core::bitcoin::consensus::encode::Error) -> Self {
        JDCError::BitcoinEncodeError(value)
    }
}

impl From<Elapsed> for JDCError {
    fn from(_value: Elapsed) -> Self {
        Self::Timeout
    }
}

impl HandlerErrorType for JDCError {
    fn parse_error(error: ParserError) -> Self {
        JDCError::Parser(error)
    }

    fn unexpected_message(extension_type: u16, message_type: u8) -> Self {
        JDCError::UnexpectedMessage(extension_type, message_type)
    }
}

impl From<ExtendedChannelClientError> for JDCError {
    fn from(value: ExtendedChannelClientError) -> Self {
        JDCError::ChannelSv2(ChannelSv2Error::ExtendedChannelClientSide(value))
    }
}

impl From<ExtendedChannelServerError> for JDCError {
    fn from(value: ExtendedChannelServerError) -> Self {
        JDCError::ChannelSv2(ChannelSv2Error::ExtendedChannelServerSide(value))
    }
}

impl From<StandardChannelError> for JDCError {
    fn from(value: StandardChannelError) -> Self {
        JDCError::ChannelSv2(ChannelSv2Error::StandardChannelServerSide(value))
    }
}
