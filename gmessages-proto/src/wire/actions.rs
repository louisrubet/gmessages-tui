//! Request and reply bodies of the calls to the phone (binary, encrypted).

use super::*;

message! {
    /// Action 1 request.
    pub struct ListConversationsRequest {
        2 => count: I64,
        4 => folder: Enum,
        5 => cursor: Msg<Cursor>,
    }

    pub struct Cursor {
        1 => last_id: Str,
        2 => timestamp: I64,
    }

    pub struct ListConversationsResponse {
        2 => conversations: Msgs<Conversation>,
        3 => unknown3: Bytes,
        5 => next: Msg<Cursor>,
    }

    /// Action 2 request.
    pub struct ListMessagesRequest {
        2 => conversation_id: Str,
        3 => count: I64,
        5 => cursor: Msg<Cursor>,
    }

    pub struct ListMessagesResponse {
        2 => messages: Msgs<Message>,
        3 => unknown3: Bytes,
        4 => total: I64,
        5 => next: Msg<Cursor>,
    }

    /// Action 3 request.
    pub struct SendMessagePayload {
        2 => conversation_id: Str,
        3 => draft: Msg<Draft>,
        4 => sim: Msg<SimSelector>,
        5 => tmp_id: Str,
        6 => force_rcs: Bool,
        8 => reply_to: Msg<IdRef>,
    }

    pub struct Draft {
        1 => tmp_id: Str,
        6 => alt_text: Msg<AltText>,
        7 => conversation_id: Str,
        9 => sender_id: Str,
        10 => parts: Msgs<MessagePart>,
        12 => tmp_id_again: Str,
    }

    pub struct AltText {
        1 => text: Msg<TextContent>,
    }

    pub struct SendMessageReply {
        2 => account: Msg<AccountInfo>,
        3 => result: Enum,
    }

    /// Action 6 request (constant).
    pub struct ListContactsRequest {
        5 => a: I32,
        6 => b: I32,
        7 => c: I32,
    }

    /// Reply of actions 6, 52, 53.
    pub struct ListContactsResponse {
        2 => contacts: Msgs<Contact>,
    }

    /// Action 52 request (unverified).
    pub struct PrefetchContactsRequest {
        2 => count: I32,
    }

    /// Action 9 request.
    pub struct GetOrCreateConversationRequest {
        2 => recipients: Msgs<Recipient>,
        3 => group_name: Opt<Str>,
        4 => create_group: Opt<Bool>,
    }

    pub struct Recipient {
        1 => kind: I32,
        2 => number: Str,
        3 => number_again: Str,
        4 => formatted: Opt<Str>,
    }

    pub struct GetOrCreateConversationResponse {
        2 => conversation: Msg<Conversation>,
        3 => status: Enum,
    }

    /// Action 10 request.
    pub struct MarkReadRequest {
        2 => conversation_id: Str,
        3 => message_id: Str,
    }

    /// Action 12 request.
    pub struct TypingRequest {
        2 => data: Msg<TypingData>,
        3 => sim: Msg<SimSelector>,
    }

    pub struct TypingData {
        1 => conversation_id: Str,
        3 => typing: Bool,
    }

    /// Action 13 request.
    pub struct SettingsUpdateRequest {
        3 => push: Msg<PushSetting>,
    }

    pub struct PushSetting {
        3 => enabled: Bool,
    }

    /// Action 15 request.
    pub struct UpdateConversationRequest {
        1 => change: Msg<ConversationChange>,
        2 => op: Enum,
        3 => conversation_id: Str,
        5 => block_flag: Msg<BlockFlag>,
        6 => target: Msg<ConversationRef>,
    }

    pub struct ConversationChange {
        1 => conversation_id: Str,
        7 => mute: I32,
        12 => status: Enum,
    }

    pub struct BlockFlag {
        2 => block: Bool,
    }

    pub struct ConversationRef {
        1 => conversation_id: Str,
        3 => number: Opt<Str>,
    }

    /// Reply of actions 15, 38 (field 1 `success`).
    pub struct SuccessReply {
        1 => success: Bool,
    }

    /// Action 21 request.
    pub struct ConversationTypeRequest {
        2 => conversation_id: Str,
    }

    pub struct ConversationTypeResponse {
        2 => conversation_id: Str,
        3 => kind: I32,
        5 => unknown5: Bool,
        6 => unknown6: I32,
    }

    /// Action 22 request.
    pub struct PingRequest {
        2 => value: Bool,
    }

    /// Actions 23 and 25 request (message id in field 2).
    pub struct MessageIdRequest {
        2 => message_id: Str,
    }

    /// Reply of action 23.
    pub struct DeleteMessageResponse {
        2 => success: Bool,
    }

    /// Action 28 request.
    pub struct TopContactsRequest {
        1 => count: I32,
    }

    pub struct TopContactsResponse {
        1 => contacts: Msgs<Contact>,
    }

    /// Actions 29 and 37 request.
    pub struct ThumbnailRequest {
        1 => ids: Rep<Str>,
    }

    pub struct ThumbnailResponse {
        1 => items: Msgs<ThumbnailEntry>,
    }

    pub struct ThumbnailEntry {
        1 => id: Str,
        2 => thumbnail: Msg<Thumbnail>,
    }

    pub struct Thumbnail {
        2 => hash: Msg<ThumbnailHash>,
        3 => image: Bytes,
        4 => unknown4: I32,
        5 => dimensions: Msg<Dimensions>,
    }

    pub struct ThumbnailHash {
        13 => value: Fx64,
    }

    /// Reply of action 31.
    pub struct DefaultSmsResponse {
        1 => is_default: Bool,
    }

    /// Action 36 request.
    pub struct FullSizeImageRequest {
        1 => message_id: Str,
        2 => part_id: Str,
    }

    /// Action 38 request.
    pub struct ReactionRequest {
        1 => message_id: Str,
        2 => reaction: Msg<Reaction>,
        3 => op: Enum,
        4 => sim: Msg<SimSelector>,
    }

    /// Action 43 request.
    pub struct GetConversationRequest {
        1 => conversation_id: Str,
    }

    pub struct GetConversationResponse {
        1 => conversation: Msg<Conversation>,
    }
}
