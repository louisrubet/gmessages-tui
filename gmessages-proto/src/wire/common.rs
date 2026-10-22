//! Shared messages: devices, credentials, configuration, errors, token refresh, media.

use super::*;

message! {
    /// An empty message (`{}`; `[]` in pblite).
    pub struct Empty {}

    /// A party (phone or browser) known to the relay. Opaque: kept and sent back unchanged.
    pub struct Device {
        1 => user_id: I64,
        2 => source_id: Str,
        3 => network: Str,
    }

    /// Version of the web client being imitated.
    pub struct ClientVersion {
        3 => year: I32,
        4 => month: I32,
        5 => day: I32,
        7 => minor_a: I32,
        9 => minor_b: I32,
    }

    /// Credentials carried by most requests to the servers.
    pub struct Credentials {
        1 => request_id: Str,
        3 => network: Str,
        6 => token: Bytes,
        7 => version: Msg<ClientVersion>,
    }

    /// Describes the client at pairing.
    pub struct BrowserDetails {
        1 => user_agent: Str,
        2 => browser: Enum,
        3 => os: Str,
        6 => device: Enum,
    }

    /// Relay token and its validity (µs).
    pub struct TokenData {
        1 => token: Bytes,
        2 => ttl: I64,
    }

    /// Error object of a non-2xx response.
    pub struct RpcError {
        1 => code: I64,
        2 => message: Str,
        3 => details: Msgs<ErrorDetail>,
    }

    pub struct ErrorDetail {
        1 => type_url: Str,
    }

    /// `GET /web/config` response.
    pub struct WebConfig {
        1 => client_version: Str,
        2 => server_version: Str,
        3 => flags: Msgs<ConfigFlag>,
        4 => more_flags: Msg<FlagList>,
        5 => device: Msg<WebDevice>,
        7 => country: Str,
        8 => unknown8: Rep<U64>,
        9 => generated_at: I64,
    }

    pub struct ConfigFlag {
        1 => key: Str,
        2 => value: I32,
        4 => unknown4: Opt<Str>,
        5 => unknown5: Opt<Str>,
    }

    pub struct FlagList {
        1 => flags: Msgs<TypedFlag>,
    }

    pub struct TypedFlag {
        1 => key: Str,
        2 => value: Msg<TypedFlagValue>,
    }

    pub struct TypedFlagValue {
        2 => int: I32,
        3 => string: Str,
    }

    pub struct WebDevice {
        2 => email: Str,
        3 => unknown3: Str,
        4 => device_id: Str,
    }

    /// `Registration/RegisterRefresh` request.
    pub struct RegisterRefreshRequest {
        1 => credentials: Msg<Credentials>,
        2 => browser: Msg<Device>,
        3 => timestamp: I64,
        4 => signature: Bytes,
        13 => options: Msg<RefreshOptions>,
        16 => unknown16: I32,
    }

    pub struct RefreshOptions {
        9 => always: Msg<Empty>,
        23 => push: Msg<PushOptions>,
    }

    pub struct PushOptions {
        1 => kind: I32,
        102 => web_push: Msg<WebPush>,
    }

    pub struct WebPush {
        1 => app: Str,
        2 => endpoint: Str,
        3 => p256dh: Str,
        4 => auth: Str,
    }

    pub struct RegisterRefreshResponse {
        2 => token: Msg<TokenData>,
    }

    /// `{2: i64}` seen in several replies; meaning unknown.
    pub struct UnknownStamp {
        2 => value: I64,
    }

    /// Body (base64) of the upload start request.
    pub struct UploadStartRequest {
        1 => one: I64,
        2 => credentials: Msg<Credentials>,
        3 => phone: Msg<Device>,
    }

    pub struct UploadResponse {
        1 => uploaded: Msg<UploadedMedia>,
        2 => message: Str,
    }

    pub struct UploadedMedia {
        1 => media_id: Str,
        2 => number: I64,
    }

    /// `x-goog-download-metadata` header (base64).
    pub struct DownloadMetadata {
        1 => file: Msg<DownloadFile>,
        2 => credentials: Msg<Credentials>,
    }

    pub struct DownloadFile {
        1 => media_id: Str,
        2 => encrypted: Bool,
    }
}
