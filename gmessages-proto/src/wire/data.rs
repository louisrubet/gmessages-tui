//! Data model: conversations, messages, participants, contacts, settings, updates.

use super::*;

message! {
    pub struct Conversation {
        1 => id: Str,
        2 => name: Str,
        4 => preview: Msg<Preview>,
        5 => last_activity: I64,
        6 => unread: Bool,
        10 => group: Bool,
        11 => outgoing_id: Str,
        12 => status: Enum,
        13 => read_only: Bool,
        15 => colour: Str,
        17 => latest_message_id: Str,
        18 => send_mode: Enum,
        20 => participants: Msgs<Participant>,
        21 => others: Rep<Str>,
        22 => kind: Enum,
        24 => unknown24: Bool,
        26 => pinned: Bool,
        27 => sim_participant: Msg<IdRef>,
        28 => unknown28: I64,
        29 => unknown29: Bool,
        31 => group_ref: Msg<GroupRef>,
        32 => sim: Msg<SimCard>,
        33 => latest: Msg<LatestRef>,
        34 => group_avatar_url: Str,
        35 => unknown35: Msg<MessageRef>,
    }

    /// `{1: str}`.
    pub struct IdRef {
        1 => id: Str,
    }

    pub struct Preview {
        1 => text: Str,
        2 => from_me: I64,
        4 => sender_name: Str,
        5 => status: Msg<PreviewStatus>,
    }

    pub struct PreviewStatus {
        1 => unknown1: I64,
        2 => status: Enum,
    }

    pub struct GroupRef {
        1 => conversation_id: Str,
        2 => rcs: Msg<RcsGroupRef>,
        3 => unknown3: Str,
    }

    pub struct RcsGroupRef {
        1 => id: Str,
        2 => conference_uri: Str,
    }

    pub struct MessageRef {
        1 => message_id: Str,
        2 => other_id: Str,
    }

    pub struct LatestRef {
        1 => message: Msg<MessageRef>,
        2 => timestamp_ms: I64,
    }

    pub struct Participant {
        1 => id: Msg<Identifier>,
        2 => first_name: Str,
        3 => full_name: Str,
        5 => colour: Str,
        6 => is_me: Bool,
        7 => sim: Msg<SimSelector>,
        8 => visible: Bool,
        10 => contact_id: Str,
        14 => unknown14: I64,
        15 => formatted_number: Str,
        19 => unknown19: I64,
        20 => unknown20: I64,
    }

    pub struct Identifier {
        1 => kind: Enum,
        2 => number: Str,
        3 => participant_id: Str,
    }

    pub struct Message {
        1 => id: Str,
        3 => kind: Msg<MessageKind>,
        4 => status: Msg<MessageStatusInfo>,
        5 => timestamp: I64,
        7 => conversation_id: Str,
        9 => sender_id: Str,
        10 => parts: Msgs<MessagePart>,
        11 => transport: I64,
        12 => tmp_id: Str,
        14 => subject: Opt<Str>,
        16 => unknown16: I64,
        19 => reactions: Msgs<ReactionGroup>,
        21 => reply_to: Msg<ReplyRef>,
        23 => group_ref: Msg<GroupRef>,
        24 => message_ref: Msg<MessageRef>,
        25 => sender: Msg<Participant>,
    }

    pub struct MessageKind {
        1 => value: I64,
    }

    pub struct ReplyRef {
        1 => message_id: Str,
        2 => conversation_id: Str,
        3 => empty: Msg<Empty>,
    }

    pub struct MessagePart {
        1 => part_id: Opt<Str>,
        2 => text: Msg<TextContent>,
        3 => media: Msg<Media>,
    }

    pub struct TextContent {
        1 => text: Str,
    }

    pub struct Media {
        1 => format: Enum,
        2 => media_id: Str,
        4 => file_name: Str,
        5 => size: I64,
        6 => dimensions: Msg<Dimensions>,
        7 => inline_preview: Bytes,
        9 => preview_id: Str,
        11 => key: Bytes,
        12 => preview_key: Bytes,
        14 => mime: Str,
    }

    pub struct Dimensions {
        1 => width: I64,
        2 => height: I64,
    }

    pub struct MessageStatusInfo {
        2 => status: Enum,
        3 => sub_code: I64,
        4 => error: Str,
        5 => status_text: Str,
        6 => unknown6: I64,
    }

    pub struct ReactionGroup {
        1 => reaction: Msg<Reaction>,
        2 => participant_ids: Rep<Str>,
    }

    pub struct Reaction {
        1 => emoji: Str,
        2 => kind: Enum,
        101 => custom: Msg<CustomEmoji>,
    }

    pub struct CustomEmoji {
        1 => uuid: Str,
        4 => unknown4: Opt<I32>,
        100 => data: Msg<CustomEmojiData>,
    }

    pub struct CustomEmojiData {
        100 => unknown100: Opt<Str>,
        101 => image: Msg<CustomEmojiImage>,
        102 => animated: Msg<CustomEmojiAnimated>,
    }

    pub struct CustomEmojiImage {
        1 => mime: Str,
        2 => uri: Str,
        3 => unknown3: Opt<I32>,
        5 => unknown5: Opt<Str>,
    }

    pub struct CustomEmojiAnimated {
        1 => variant: Msg<CustomEmojiVariant>,
    }

    pub struct CustomEmojiVariant {
        1 => unknown1: I32,
        2 => mime: Str,
        3 => uri: Str,
        5 => width: I32,
        6 => height: I32,
        7 => unknown7: Opt<I32>,
    }

    pub struct Contact {
        1 => participant_id: Str,
        2 => name: Str,
        3 => number: Msg<ContactNumber>,
        7 => colour: Str,
        8 => name_again: Str,
        10 => unknown10: Bool,
        11 => contact_id: Str,
    }

    pub struct ContactNumber {
        1 => kind: I32,
        2 => number: Str,
        3 => number_again: Str,
        4 => formatted: Opt<Str>,
    }

    pub struct TypingInfo {
        1 => conversation_id: Str,
        2 => who: Msg<TypingWho>,
        3 => state: Enum,
        4 => group_ref: Msg<GroupRef>,
    }

    pub struct TypingWho {
        1 => unknown1: I64,
        2 => number: Str,
    }

    pub struct SimSelector {
        1 => unknown1: I32,
        2 => sim_number: I32,
    }

    pub struct SimCard {
        3 => rcs: Msg<RcsFlag>,
        5 => data: Msg<SimData>,
        6 => unknown6: Bool,
        7 => self_ref: Msg<IdRef>,
        8 => rcs_address_a: Str,
        9 => rcs_address_b: Str,
    }

    pub struct RcsFlag {
        1 => enabled: Bool,
    }

    pub struct SimData {
        1 => selector: Msg<SimSelector>,
        2 => is_default: Bool,
        3 => carrier: Str,
        4 => colour: Str,
        5 => unknown5: I64,
        6 => formatted_number: Str,
        7 => number: Str,
    }

    pub struct Settings {
        2 => sims: Msgs<SimCard>,
        3 => misc: Msg<MiscSettings>,
        4 => rcs: Msg<RcsSettings>,
        5 => app_version: Str,
        7 => unknown7: Bool,
        9 => unknown9: Bytes,
    }

    pub struct MiscSettings {
        3 => web_push: Opt<Bool>,
    }

    pub struct RcsSettings {
        1 => enabled: Bool,
        2 => read_receipts: Bool,
        3 => typing: Bool,
        4 => default_sms_app: Opt<Bool>,
        5 => unknown5: I32,
        6 => unknown6: Bytes,
        7 => unknown7: Str,
        8 => unknown8: Str,
    }

    pub struct AccountInfo {
        1 => account: Str,
        2 => unknown2: Bool,
    }

    /// Decrypted body of a pushed update (action 16).
    pub struct Update {
        2 => conversations: Msg<ConversationList>,
        3 => messages: Msg<MessageList>,
        4 => typing: Msg<TypingWrap>,
        5 => settings: Msg<Settings>,
        6 => alert: Msg<AlertWrap>,
        7 => presence_check: Msg<Empty>,
        15 => account: Msg<AccountInfo>,
    }

    pub struct ConversationList {
        2 => conversations: Msgs<Conversation>,
    }

    pub struct MessageList {
        2 => messages: Msgs<Message>,
    }

    pub struct TypingWrap {
        2 => typing: Msg<TypingInfo>,
    }

    pub struct AlertWrap {
        2 => alert: Enum,
    }
}
