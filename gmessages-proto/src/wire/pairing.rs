//! Pairing messages: QR code, Google account (with UKEY2), unpairing.

use super::*;

message! {
    /// `Pairing/RegisterPhoneRelay` request (binary).
    pub struct RegisterPhoneRelayRequest {
        1 => credentials: Msg<Credentials>,
        3 => browser: Msg<BrowserDetails>,
        4 => keys: Msg<KeyRegistration>,
    }

    pub struct KeyRegistration {
        6 => key: Msg<PublicKeyEntry>,
    }

    pub struct PublicKeyEntry {
        1 => kind: I32,
        2 => public_key: Bytes,
    }

    pub struct RegisterPhoneRelayResponse {
        1 => unknown1: Msg<UnknownStamp>,
        2 => browser: Msg<Device>,
        3 => pairing_key: Bytes,
        4 => valid_for: I64,
        5 => token: Msg<TokenData>,
        6 => response_id: Str,
    }

    /// `Pairing/RefreshPhoneRelay` request (binary).
    pub struct RefreshPhoneRelayRequest {
        1 => credentials: Msg<Credentials>,
    }

    pub struct RefreshPhoneRelayResponse {
        1 => unknown1: Msg<UnknownStamp>,
        2 => pairing_key: Bytes,
        3 => valid_for: I64,
    }

    /// Content of the QR code (base64 of its binary encoding).
    pub struct QrCodeData {
        1 => pairing_key: Bytes,
        2 => aes_key: Bytes,
        3 => hmac_key: Bytes,
    }

    /// Payload of route 14 items.
    pub struct PairingEvent {
        4 => paired: Msg<Paired>,
        5 => revoked: Msg<Revoked>,
    }

    pub struct Paired {
        1 => phone: Msg<Device>,
        2 => token: Msg<TokenData>,
        3 => browser: Msg<Device>,
    }

    pub struct Revoked {
        1 => browser: Msg<Device>,
    }

    /// `Registration/SignInGaia` request (pblite).
    pub struct SignInGaiaRequest {
        1 => credentials: Msg<Credentials>,
        2 => body: Msg<SignInBody>,
        4 => network: Str,
    }

    pub struct SignInBody {
        1 => device: Msg<WebDeviceRef>,
        36 => key: B64Msg<SignInKey>,
    }

    pub struct SignInKey {
        3 => public_key: Bytes,
    }

    pub struct WebDeviceRef {
        1 => kind: I32,
        2 => id: Str,
    }

    pub struct SignInGaiaResponse {
        1 => header: Msg<SignInHeader>,
        2 => browser_uuid: B64Str,
        3 => devices: Msg<SignInDevices>,
        4 => token: Msg<TokenData>,
    }

    pub struct SignInHeader {
        2 => unknown2: U64,
        4 => timestamp: I64,
    }

    pub struct SignInDevices {
        1 => this: Msg<DeviceWrapper>,
        2 => registrations: Msgs<Registration>,
        3 => seen: Msgs<RegistrationSeen>,
    }

    pub struct DeviceWrapper {
        1 => device: Msg<Device>,
    }

    pub struct Registration {
        1 => id: B64Str,
        4 => role: I32,
        5 => language: Str,
        7 => unknown7: U64,
    }

    pub struct RegistrationSeen {
        1 => id: B64Str,
        3 => role: I32,
        4 => unknown4: I32,
        7 => seen: I64,
        8 => extra: B64Msg<RegistrationSeenExtra>,
    }

    pub struct RegistrationSeenExtra {
        1 => unknown1: I32,
        2 => unknown2: U64,
        3 => unknown3: Bytes,
        4 => unknown4: I32,
    }

    /// Payload (clear) of actions 44 and 45.
    pub struct PairingRequest {
        1 => attempt_id: Str,
        2 => browser: Msg<BrowserDetails>,
        3 => started: I64,
        4 => data: Bytes,
        5 => emoji_version: I32,
        6 => key_version: I32,
        8 => statement: Str,
    }

    /// Reply (clear) of actions 44 and 45.
    pub struct PairingReply {
        1 => error_kind: I32,
        2 => error: Enum,
        3 => unknown3: I32,
        4 => session: Str,
        5 => data: Bytes,
        6 => emoji_version: I32,
        7 => key_version: I32,
        8 => unknown8: Bytes,
    }

    /// Payload of action 46 (account unpair).
    pub struct UnpairRequest {
        1 => attempt_id: Str,
    }

    /// `Pairing/RevokeRelayPairing` request (binary).
    pub struct RevokeRelayPairingRequest {
        1 => credentials: Msg<Credentials>,
        2 => browser: Msg<Device>,
    }

    pub struct RevokeRelayPairingResponse {
        1 => unknown1: Msg<UnknownStamp>,
    }

    /// UKEY2 envelope.
    pub struct Ukey2Message {
        1 => kind: Enum,
        2 => data: Bytes,
    }

    pub struct Ukey2ClientInit {
        1 => version: I32,
        2 => random: Bytes,
        3 => commitments: Msgs<Ukey2Commitment>,
        4 => next_protocol: Str,
    }

    pub struct Ukey2Commitment {
        1 => cipher: Enum,
        2 => commitment: Bytes,
    }

    pub struct Ukey2ServerInit {
        1 => version: I32,
        2 => random: Bytes,
        3 => cipher: Enum,
        4 => public_key: Msg<Ukey2PublicKey>,
    }

    pub struct Ukey2ClientFinished {
        1 => public_key: Msg<Ukey2PublicKey>,
    }

    pub struct Ukey2PublicKey {
        1 => kind: Enum,
        2 => ec: Msg<Ukey2EcPoint>,
    }

    pub struct Ukey2EcPoint {
        1 => x: Bytes,
        2 => y: Bytes,
    }
}
