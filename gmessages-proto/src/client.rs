//! The connected client: events and calls to the phone (spec §5, §6, §8).

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::sync::mpsc::{self, UnboundedReceiver};
use tracing::{info, warn};
use uuid::Uuid;

use crate::config::Config;
use crate::conn::Conn;
use crate::error::{Error, Result};
use crate::event::Event;
use crate::media::{self, MediaDownload};
use crate::pairing;
use crate::relay::{CallOpts, Mode, Relay, action};
use crate::session::Session;
use crate::types::*;
use crate::wire;

/// Receiver of the client's [`Event`]s.
pub type EventReceiver = UnboundedReceiver<Event>;

struct Guard(Arc<Relay>);
impl Drop for Guard {
    fn drop(&mut self) {
        self.0.shutdown();
    }
}

/// A client connected to the phone through the relay.
///
/// Created by [`Client::connect`]. Cheap to clone; the connection stops when the last clone
/// is dropped, or on [`Client::disconnect`]. Calls return [`Error::Timeout`] when the phone
/// does not answer in time ([`Config::call_timeout`]), and [`Error::PhoneRefused`] when it
/// answers with a failure.
#[derive(Clone)]
pub struct Client {
    relay: Arc<Relay>,
    _guard: Arc<Guard>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").finish_non_exhaustive()
    }
}

fn refused_unless(ok: bool, what: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Error::PhoneRefused(format!("{what} failed")))
    }
}

impl Client {
    /// Connects with a paired session and starts the background tasks (event stream,
    /// acknowledgements, activation, pings, token refresh, reconnection).
    ///
    /// Returns the client and the receiver of its events. Fails at once if the session is
    /// incomplete or its token cannot be refreshed because the session is gone.
    pub async fn connect(config: Config, session: Session) -> Result<(Client, EventReceiver)> {
        if !session.is_complete() {
            return Err(Error::InvalidInput(
                "session is incomplete (keys, token or phone missing)".into(),
            ));
        }
        let (tx, rx) = mpsc::unbounded_channel();
        let conn = Arc::new(Conn::new(config, session, Some(tx.clone()))?);
        match conn.ensure_token(false).await {
            Ok(()) => {}
            Err(e @ Error::SessionGone(_)) => return Err(e),
            Err(e) => warn!(%e, "token refresh failed at connect; continuing"),
        }
        let relay = Relay::new(conn, Mode::Paired, tx);
        relay.start();
        info!("client started");
        Ok((
            Client {
                relay: relay.clone(),
                _guard: Arc::new(Guard(relay)),
            },
            rx,
        ))
    }

    /// A snapshot of the current session (to persist).
    pub fn session(&self) -> Session {
        self.relay.conn.snapshot()
    }

    /// The configuration in use.
    pub fn config(&self) -> &Config {
        &self.relay.conn.config
    }

    /// Takes the active role again (after an [`Alert::is_browser_inactive`] alert).
    pub async fn activate(&self) -> Result<()> {
        self.relay.activate().await
    }

    /// Drops the event stream and reconnects with a new stream id (and a new activation).
    pub fn reconnect(&self) {
        self.relay.force_reconnect();
    }

    /// Stops the client. Pending and later calls fail with [`Error::Closed`].
    pub fn disconnect(&self) {
        self.relay.shutdown();
    }

    /// Whether the client was stopped (by [`disconnect`](Self::disconnect) or a fatal
    /// error).
    pub fn is_closed(&self) -> bool {
        self.relay.is_shut()
    }

    /// Unpairs this session. The session cannot be used afterwards.
    pub async fn unpair(&self) -> Result<()> {
        pairing::unpair_with(&self.relay).await?;
        self.relay.shutdown();
        Ok(())
    }

    // ------------------------------------------------------------ conversations

    /// Lists one page of conversations of a folder (action 1), most recent first.
    ///
    /// Pass the previous page's `next` as `cursor` to continue.
    pub async fn list_conversations(
        &self,
        folder: Folder,
        count: i64,
        cursor: Option<&Cursor>,
    ) -> Result<ConversationPage> {
        let req = wire::ListConversationsRequest {
            count,
            folder: folder.value(),
            cursor: cursor.map(Cursor::to_wire),
        };
        let mut opts = CallOpts::normal(self.relay.conn.config.call_timeout);
        if !self.relay.first_list_done.swap(true, Ordering::SeqCst) {
            opts.class = 16;
        }
        let r: wire::ListConversationsResponse = self
            .relay
            .call_typed_with(action::LIST_CONVERSATIONS, Some(&req), opts)
            .await?;
        Ok(ConversationPage {
            conversations: r.conversations.into_iter().map(Into::into).collect(),
            next: Cursor::from_wire(r.next),
        })
    }

    /// Lists conversations of a folder page by page, up to `max`, stopping when a page is
    /// short, has no cursor, or brings nothing new.
    pub async fn list_all_conversations(
        &self,
        folder: Folder,
        page_size: i64,
        max: usize,
    ) -> Result<Vec<Conversation>> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut cursor: Option<Cursor> = None;
        loop {
            let page = self
                .list_conversations(folder, page_size, cursor.as_ref())
                .await?;
            let n = page.conversations.len();
            let mut new = 0;
            for c in page.conversations {
                if seen.insert(c.id.clone()) {
                    new += 1;
                    out.push(c);
                }
            }
            if new == 0 || (n as i64) < page_size || page.next.is_none() || out.len() >= max {
                break;
            }
            cursor = page.next;
        }
        out.truncate(max);
        Ok(out)
    }

    /// Gets one conversation (action 43).
    pub async fn get_conversation(&self, conversation_id: &str) -> Result<Conversation> {
        let req = wire::GetConversationRequest {
            conversation_id: conversation_id.to_string(),
        };
        let r: wire::GetConversationResponse = self
            .relay
            .call_typed(action::GET_CONVERSATION, Some(&req))
            .await?;
        r.conversation
            .map(Into::into)
            .ok_or_else(|| Error::PhoneRefused("conversation not found".into()))
    }

    /// Gets the type of a conversation (action 21).
    pub async fn conversation_type(&self, conversation_id: &str) -> Result<ConversationType> {
        let req = wire::ConversationTypeRequest {
            conversation_id: conversation_id.to_string(),
        };
        let r: wire::ConversationTypeResponse = self
            .relay
            .call_typed(action::CONVERSATION_TYPE, Some(&req))
            .await?;
        Ok(r.kind.into())
    }

    /// Finds or creates the conversation with these phone numbers (action 9).
    ///
    /// When several numbers would make an RCS group, the phone asks for confirmation:
    /// [`GetOrCreateOutcome::NeedsRcsGroupConfirmation`] is returned unless
    /// `confirm_rcs_group` is `Some(group name)` (`""` for none), in which case the creation
    /// is confirmed.
    pub async fn get_or_create_conversation(
        &self,
        numbers: &[&str],
        confirm_rcs_group: Option<&str>,
    ) -> Result<GetOrCreateOutcome> {
        let recipients: Vec<wire::Recipient> = numbers
            .iter()
            .map(|n| wire::Recipient {
                kind: 2,
                number: n.to_string(),
                number_again: n.to_string(),
                formatted: None,
            })
            .collect();
        let mut req = wire::GetOrCreateConversationRequest {
            recipients,
            group_name: None,
            create_group: None,
        };
        loop {
            let r: wire::GetOrCreateConversationResponse = self
                .relay
                .call_typed(action::GET_OR_CREATE_CONVERSATION, Some(&req))
                .await?;
            if r.status == 3 {
                match (confirm_rcs_group, req.create_group) {
                    (Some(name), None) => {
                        req.group_name = Some(name.to_string());
                        req.create_group = Some(true);
                        continue;
                    }
                    _ => return Ok(GetOrCreateOutcome::NeedsRcsGroupConfirmation),
                }
            }
            return r
                .conversation
                .map(|c| GetOrCreateOutcome::Conversation(Box::new(c.into())))
                .ok_or_else(|| Error::PhoneRefused(format!("get or create: status {}", r.status)));
        }
    }

    /// Applies an operation to a conversation (action 15). `other_number` is the other
    /// person's number, needed by delete, block and unblock of one-to-one conversations.
    pub async fn update_conversation(
        &self,
        op: ConversationOp,
        conversation_id: &str,
        other_number: Option<&str>,
    ) -> Result<()> {
        let req = wire::UpdateConversationRequest {
            change: None,
            op: op.value(),
            conversation_id: conversation_id.to_string(),
            block_flag: op.is_block().then_some(wire::BlockFlag { block: true }),
            target: Some(wire::ConversationRef {
                conversation_id: conversation_id.to_string(),
                number: if op.uses_number() {
                    other_number.map(str::to_string)
                } else {
                    None
                },
            }),
        };
        let r: wire::SuccessReply = self
            .relay
            .call_typed(action::UPDATE_CONVERSATION, Some(&req))
            .await?;
        refused_unless(r.success, "conversation update")
    }

    /// Archives a conversation.
    pub async fn archive(&self, c: &Conversation) -> Result<()> {
        self.update_conversation(ConversationOp::Archive, &c.id, None)
            .await
    }
    /// Moves a conversation back to the inbox.
    pub async fn unarchive(&self, c: &Conversation) -> Result<()> {
        self.update_conversation(ConversationOp::Unarchive, &c.id, None)
            .await
    }
    /// Moves a conversation to the trash (refused if already there).
    pub async fn move_to_trash(&self, c: &Conversation) -> Result<()> {
        self.update_conversation(ConversationOp::MoveToTrash, &c.id, None)
            .await
    }
    /// Deletes a conversation for good, with its messages.
    pub async fn delete_forever(&self, c: &Conversation) -> Result<()> {
        self.update_conversation(ConversationOp::DeleteForever, &c.id, c.other_number())
            .await
    }
    /// Blocks the other person of a conversation.
    pub async fn block(&self, c: &Conversation) -> Result<()> {
        self.update_conversation(ConversationOp::Block, &c.id, c.other_number())
            .await
    }
    /// Unblocks the other person of a conversation.
    pub async fn unblock(&self, c: &Conversation) -> Result<()> {
        self.update_conversation(ConversationOp::Unblock, &c.id, c.other_number())
            .await
    }
    /// Blocks the other person and reports spam.
    pub async fn block_and_report(&self, c: &Conversation) -> Result<()> {
        self.update_conversation(ConversationOp::BlockAndReport, &c.id, c.other_number())
            .await
    }

    // ------------------------------------------------------------ messages

    /// Lists one page of messages (action 2), sorted oldest first. Without a cursor, the
    /// latest messages; with one, older ones.
    pub async fn list_messages(
        &self,
        conversation_id: &str,
        count: i64,
        cursor: Option<&Cursor>,
    ) -> Result<MessagePage> {
        let req = wire::ListMessagesRequest {
            conversation_id: conversation_id.to_string(),
            count,
            cursor: cursor.map(Cursor::to_wire),
        };
        let r: wire::ListMessagesResponse = self
            .relay
            .call_typed(action::LIST_MESSAGES, Some(&req))
            .await?;
        let mut messages: Vec<Message> = r.messages.into_iter().map(Into::into).collect();
        messages.sort_by_key(|m| m.timestamp);
        Ok(MessagePage {
            messages,
            total: r.total,
            next: Cursor::from_wire(r.next),
        })
    }

    /// Sends a message (action 3).
    ///
    /// Returns once the phone has taken it (or refused it): the message itself then arrives
    /// as [`Event::MessageUpserted`] with the same `tmp_id`, again on each status change.
    /// Transient failures (results 2 and 3) are retried after 3, 8 and 20 s with the same
    /// `tmp_id`. A final failure is returned in [`SendOutcome::result`], not as an error.
    pub async fn send_message(&self, msg: OutgoingMessage) -> Result<SendOutcome> {
        let tmp_id = msg
            .tmp_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let mut parts: Vec<wire::MessagePart> = msg
            .media
            .iter()
            .map(|m| wire::MessagePart {
                part_id: None,
                text: None,
                media: Some(m.into()),
            })
            .collect();
        if !msg.text.is_empty() {
            parts.push(wire::MessagePart {
                part_id: None,
                text: Some(wire::TextContent {
                    text: msg.text.clone(),
                }),
                media: None,
            });
        }
        if parts.is_empty() {
            return Err(Error::InvalidInput("empty message".into()));
        }
        let req = wire::SendMessagePayload {
            conversation_id: msg.conversation_id.clone(),
            draft: Some(wire::Draft {
                tmp_id: tmp_id.clone(),
                alt_text: None,
                conversation_id: msg.conversation_id.clone(),
                sender_id: msg.sender_id.clone(),
                parts,
                tmp_id_again: tmp_id.clone(),
            }),
            sim: msg.sim.map(Into::into),
            tmp_id: tmp_id.clone(),
            force_rcs: msg.force_rcs,
            reply_to: msg.reply_to.clone().map(|id| wire::IdRef { id }),
        };
        let delays = [3u64, 8, 20];
        let mut attempt = 0;
        loop {
            let r: wire::SendMessageReply = self
                .relay
                .call_typed(action::SEND_MESSAGE, Some(&req))
                .await?;
            let result = SendResult::from(r.result);
            if result.is_transient() && attempt < delays.len() {
                warn!(?result, attempt, "send failed transiently; retrying");
                tokio::time::sleep(Duration::from_secs(delays[attempt])).await;
                attempt += 1;
                continue;
            }
            return Ok(SendOutcome {
                tmp_id,
                result,
                account: r.account.map(Into::into),
            });
        }
    }

    /// Marks a conversation read up to a message (action 10).
    pub async fn mark_read(&self, conversation_id: &str, message_id: &str) -> Result<()> {
        let req = wire::MarkReadRequest {
            conversation_id: conversation_id.to_string(),
            message_id: message_id.to_string(),
        };
        let _: wire::Empty = self.relay.call_typed(action::MARK_READ, Some(&req)).await?;
        Ok(())
    }

    /// Signals that the user is typing (action 12; no reply). Send it repeatedly while
    /// typing; there is no explicit stop.
    pub async fn set_typing(&self, conversation_id: &str, sim: Option<SimSelector>) -> Result<()> {
        let req = wire::TypingRequest {
            data: Some(wire::TypingData {
                conversation_id: conversation_id.to_string(),
                typing: true,
            }),
            sim: sim.map(Into::into),
        };
        self.relay.call_no_reply(action::TYPING, Some(&req)).await
    }

    /// Deletes a message on the phone (action 23).
    pub async fn delete_message(&self, message_id: &str) -> Result<()> {
        let req = wire::MessageIdRequest {
            message_id: message_id.to_string(),
        };
        let r: wire::DeleteMessageResponse = self
            .relay
            .call_typed(action::DELETE_MESSAGE, Some(&req))
            .await?;
        refused_unless(r.success, "message deletion")
    }

    /// Resends a failed message (action 25).
    ///
    /// **Unverified**: known from the web client's action list, not exercised by any known
    /// client. The reply shape is unknown; this waits for any reply.
    pub async fn resend_message(&self, message_id: &str) -> Result<()> {
        let req = wire::MessageIdRequest {
            message_id: message_id.to_string(),
        };
        let _: wire::Empty = self
            .relay
            .call_typed(action::RESEND_MESSAGE, Some(&req))
            .await?;
        Ok(())
    }

    /// Adds, removes or replaces our reaction on a message (action 38).
    pub async fn react(
        &self,
        message_id: &str,
        emoji: &str,
        op: ReactionOp,
        sim: Option<SimSelector>,
    ) -> Result<()> {
        let kind = ReactionKind::for_emoji(emoji);
        let req = wire::ReactionRequest {
            message_id: message_id.to_string(),
            reaction: Some(wire::Reaction {
                emoji: emoji.to_string(),
                kind: kind.0,
                custom: None,
            }),
            op: op.value(),
            sim: sim.map(Into::into),
        };
        let r: wire::SuccessReply = self.relay.call_typed(action::REACTION, Some(&req)).await?;
        refused_unless(r.success, "reaction")
    }

    /// Asks the phone for the full-size file of a media part that only has a preview
    /// (action 36). The phone then pushes a message update in which that part has a
    /// `media_id`.
    pub async fn request_full_size_image(&self, message_id: &str, part_id: &str) -> Result<()> {
        let req = wire::FullSizeImageRequest {
            message_id: message_id.to_string(),
            part_id: part_id.to_string(),
        };
        let _: wire::Empty = self
            .relay
            .call_typed(action::FULL_SIZE_IMAGE, Some(&req))
            .await?;
        Ok(())
    }

    // ------------------------------------------------------------ contacts

    /// The phone's address book, one entry per number (action 6).
    pub async fn list_contacts(&self) -> Result<Vec<Contact>> {
        let req = wire::ListContactsRequest {
            a: 1,
            b: 350,
            c: 50,
        };
        let r: wire::ListContactsResponse = self
            .relay
            .call_typed(action::LIST_CONTACTS, Some(&req))
            .await?;
        Ok(r.contacts.into_iter().map(Into::into).collect())
    }

    /// Top contacts (action 28).
    pub async fn top_contacts(&self, count: i32) -> Result<Vec<Contact>> {
        let req = wire::TopContactsRequest { count };
        let r: wire::TopContactsResponse = self
            .relay
            .call_typed(action::TOP_CONTACTS, Some(&req))
            .await?;
        Ok(r.contacts.into_iter().map(Into::into).collect())
    }

    /// Prefetched contacts (action 52). **Unverified** request (`{2: 350}`).
    pub async fn prefetch_contacts(&self) -> Result<Vec<Contact>> {
        let req = wire::PrefetchContactsRequest { count: 350 };
        let r: wire::ListContactsResponse = self
            .relay
            .call_typed(action::PREFETCH_CONTACTS, Some(&req))
            .await?;
        Ok(r.contacts.into_iter().map(Into::into).collect())
    }

    /// Blocked contacts (action 53). **Unverified** request (sent without a body).
    pub async fn blocked_contacts(&self) -> Result<Vec<Contact>> {
        let r: wire::ListContactsResponse = self
            .relay
            .call_typed::<wire::Empty, _>(action::BLOCKED_CONTACTS, None)
            .await?;
        Ok(r.contacts.into_iter().map(Into::into).collect())
    }

    async fn thumbnails(&self, act: i32, ids: &[String]) -> Result<Vec<Thumbnail>> {
        let req = wire::ThumbnailRequest { ids: ids.to_vec() };
        let r: wire::ThumbnailResponse = self.relay.call_typed(act, Some(&req)).await?;
        Ok(r.items
            .into_iter()
            .map(|e| {
                let t = e.thumbnail.unwrap_or_default();
                Thumbnail {
                    id: e.id,
                    image: t.image,
                    dimensions: t.dimensions.map(|d| Dimensions {
                        width: d.width,
                        height: d.height,
                    }),
                }
            })
            .collect())
    }

    /// Avatars of contacts, by contact id (action 29).
    pub async fn contact_thumbnails(&self, contact_ids: &[String]) -> Result<Vec<Thumbnail>> {
        self.thumbnails(action::CONTACT_THUMBNAILS, contact_ids)
            .await
    }

    /// Avatars of participants, by participant id (action 37).
    pub async fn participant_thumbnails(
        &self,
        participant_ids: &[String],
    ) -> Result<Vec<Thumbnail>> {
        self.thumbnails(action::PARTICIPANT_THUMBNAILS, participant_ids)
            .await
    }

    // ------------------------------------------------------------ phone

    /// Whether Google Messages is the phone's default SMS app (action 31).
    pub async fn is_default_sms_app(&self) -> Result<bool> {
        let r: wire::DefaultSmsResponse = self
            .relay
            .call_typed::<wire::Empty, _>(action::DEFAULT_SMS_APP, None)
            .await?;
        Ok(r.is_default)
    }

    /// Enables or disables web push on the phone (action 13; no reply). Registering a push
    /// subscription itself is not implemented.
    pub async fn set_web_push(&self, enabled: bool) -> Result<()> {
        let req = wire::SettingsUpdateRequest {
            push: Some(wire::PushSetting { enabled }),
        };
        self.relay
            .call_no_reply(action::SETTINGS_UPDATE, Some(&req))
            .await
    }

    // ------------------------------------------------------------ media

    /// Encrypts and uploads a file; returns the media to put in an [`OutgoingMessage`].
    pub async fn upload_media(&self, data: &[u8], file_name: &str, mime: &str) -> Result<Media> {
        media::upload(&self.relay.conn, data, file_name, mime).await
    }

    /// Downloads and decrypts a file by media id and key.
    pub async fn download_media(&self, media_id: &str, key: &[u8]) -> Result<Vec<u8>> {
        media::download(&self.relay.conn, media_id, key)
            .await?
            .bytes()
            .await
    }

    /// Starts a download decrypted as it arrives.
    pub async fn download_media_stream(&self, media_id: &str, key: &[u8]) -> Result<MediaDownload> {
        media::download(&self.relay.conn, media_id, key).await
    }

    /// Downloads the best available version of a media part: the full file, else the
    /// uploaded preview, else the inline preview. Fails with [`Error::InvalidInput`] if
    /// nothing is available yet.
    pub async fn download(&self, m: &Media) -> Result<Vec<u8>> {
        match m.availability() {
            MediaAvailability::Full => self.download_media(&m.media_id, &m.key).await,
            MediaAvailability::PreviewOnly => {
                self.download_media(&m.preview_id, &m.preview_key).await
            }
            MediaAvailability::InlinePreviewOnly => Ok(m.inline_preview.clone()),
            MediaAvailability::Pending => {
                Err(Error::InvalidInput("media not available yet".into()))
            }
        }
    }

    /// Fetches an RCS group picture ([`Conversation::group_avatar_url`]), up to 5 MiB.
    pub async fn fetch_group_avatar(&self, url: &str) -> Result<Vec<u8>> {
        media::fetch_plain(&self.relay.conn, url, 5 << 20).await
    }
}
