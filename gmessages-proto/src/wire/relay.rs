//! Relay messages: calling the phone, the event stream, acknowledgements.

use super::*;

message! {
    /// `Messaging/SendMessage` request (pblite).
    pub struct SendMessageRequest {
        1 => phone: Msg<Device>,
        2 => routing: Msg<Routing>,
        3 => auth: Msg<CallAuth>,
        5 => ttl: I64,
        9 => destinations: Rep<B64Str>,
    }

    pub struct Routing {
        1 => request_id: Str,
        2 => route: I32,
        12 => call: Bytes,
        23 => class: Msg<MessageClass>,
    }

    pub struct MessageClass {
        1 => empty: Msg<Empty>,
        2 => class: I32,
    }

    pub struct CallAuth {
        1 => request_id: Str,
        6 => token: Bytes,
        7 => version: Msg<ClientVersion>,
    }

    /// The call itself (binary, inside `Routing.call`).
    pub struct Call {
        1 => request_id: Str,
        2 => action: Enum,
        3 => clear: Bytes,
        5 => encrypted: Bytes,
        6 => session: Str,
    }

    pub struct SendMessageResponse {
        1 => accepted: Msg<SendAccepted>,
        2 => timestamp: Opt<Str>,
    }

    pub struct SendAccepted {
        2 => value: Str,
    }

    /// `Messaging/ReceiveMessages` request (pblite).
    pub struct ReceiveMessagesRequest {
        1 => credentials: Msg<Credentials>,
        4 => unknown4: Msg<ReceiveUnknown>,
    }

    pub struct ReceiveUnknown {
        2 => empty: Msg<Empty>,
    }

    /// One item of the event stream.
    pub struct StreamItem {
        2 => envelope: Msg<IncomingEnvelope>,
        3 => heartbeat: Msg<Empty>,
        4 => backlog: Msg<Backlog>,
        5 => start: Msg<Empty>,
    }

    pub struct Backlog {
        1 => count: Opt<I32>,
    }

    pub struct IncomingEnvelope {
        1 => id: Str,
        2 => route: I32,
        3 => exec_start: U64,
        5 => class: I32,
        6 => exec_end: U64,
        7 => duration: U64,
        8 => phone: Msg<Device>,
        9 => browser: Msg<Device>,
        12 => payload: Bytes,
        17 => signature_id: Str,
        21 => timestamp: Str,
        23 => unknown23: Msg<UnknownStamp>,
    }

    /// Payload of route 19 items (binary).
    pub struct DataPayload {
        1 => request_id: Str,
        3 => timestamp: I64,
        4 => action: Enum,
        5 => clear: Bytes,
        6 => unknown6: Bool,
        7 => unknown7: Bool,
        8 => encrypted: Bytes,
        9 => unknown9: Bool,
        11 => encrypted_extra: Bytes,
    }

    /// Decrypted `DataPayload.encrypted_extra`.
    pub struct ExtraPayload {
        2 => account: Msg<AccountInfo>,
    }

    /// `Messaging/AckMessages` request (pblite).
    pub struct AckMessagesRequest {
        1 => credentials: Msg<Credentials>,
        2 => empty: Msg<Empty>,
        4 => acks: Msgs<Ack>,
    }

    pub struct Ack {
        1 => id: Str,
        2 => browser: Msg<Device>,
    }
}
