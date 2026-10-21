// gmd: Google Messages for gmessages-tui, through libgm (the client of mautrix-gmessages).
//
//	gmd --session FILE [--log FILE]
//
// Speaks JSON lines: events on stdout, commands on stdin. When FILE holds no session, says
// "login" and pairs as told: with the Google account of the phone (cookies of a browser
// signed in to it, then an emoji to tap on the phone), or by QR code. Keeps the session in
// FILE.
package main

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/rs/zerolog"
	"go.mau.fi/mautrix-gmessages/pkg/libgm"
	"go.mau.fi/mautrix-gmessages/pkg/libgm/events"
	"go.mau.fi/mautrix-gmessages/pkg/libgm/gmproto"
	"go.mau.fi/mautrix-gmessages/pkg/libgm/util"
	"go.mau.fi/util/exhttp"
	"google.golang.org/protobuf/encoding/protowire"
)

// Version of the protocol spoken on stdin/stdout, checked by gmessages-tui.
const protocol = 9

// Conversations listed when the session becomes active.
const conversationCount = 100

// Messages fetched per conversation when asked, and conversations fetched at once.
const (
	historyCount   = 30
	historyWorkers = 3
)

// Message is a message as gmessages-tui shows it.
type Message struct {
	ID          string     `json:"id"`
	TimestampMS int64      `json:"timestamp_ms"`
	FromMe      bool       `json:"from_me"`
	SenderID    string     `json:"sender_id"`
	Sender      string     `json:"sender"`
	Text        string     `json:"text"`
	Quote       string     `json:"quote,omitempty"`
	Media       []Medium   `json:"media,omitempty"`
	Reactions   []Reaction `json:"reactions,omitempty"`
	Deleted     bool       `json:"deleted,omitempty"`
}

// Medium is a medium of a message, as the phone describes it: what downloads it. Kept by
// gmessages-tui (in its encrypted store) and given back to download it.
type Medium struct {
	Mime   string `json:"mime"`
	Name   string `json:"name,omitempty"`
	Size   int64  `json:"size,omitempty"`
	Width  int64  `json:"width,omitempty"`
	Height int64  `json:"height,omitempty"`
	// Its part of the message, to ask the phone for it in full.
	Part     string `json:"part,omitempty"`
	MediaID  string `json:"media_id,omitempty"`
	Key      []byte `json:"key,omitempty"`
	ThumbID  string `json:"thumb_id,omitempty"`
	ThumbKey []byte `json:"thumb_key,omitempty"`
	// A small preview, in the message itself.
	Inline []byte `json:"inline,omitempty"`
}

func medium(m *gmproto.MediaContent, part string) Medium {
	mime := m.GetMimeType()
	if mime == "" {
		mime = "application/octet-stream"
	}
	return Medium{
		Mime: mime, Name: m.GetMediaName(), Size: m.GetSize(),
		Width: m.GetDimensions().GetWidth(), Height: m.GetDimensions().GetHeight(),
		Part: part, MediaID: m.GetMediaID(), Key: m.GetDecryptionKey(),
		ThumbID: m.GetThumbnailMediaID(), ThumbKey: m.GetThumbnailDecryptionKey(),
		Inline: m.GetMediaData(),
	}
}

// Reaction is one person's emoji on a message.
type Reaction struct {
	Emoji    string `json:"emoji"`
	SenderID string `json:"sender_id"`
	Sender   string `json:"sender"`
	Mine     bool   `json:"mine"`
}

// Conversation is a chat as gmessages-tui shows it.
type Conversation struct {
	ID          string  `json:"id"`
	Name        string  `json:"name"`
	Group       bool    `json:"group"`
	TimestampMS int64   `json:"timestamp_ms"`
	Latest      *Latest `json:"latest,omitempty"`
	// Out of the inbox of the phone: "archive", "spam" (or blocked), "trash".
	Folder string `json:"folder,omitempty"`
	// Deleted for good.
	Deleted bool `json:"deleted,omitempty"`
	// Read on the phone up to its last message: false.
	Unread bool `json:"unread"`
}

// Latest is the last message of a conversation, as the phone summarizes it.
type Latest struct {
	ID     string `json:"id"`
	Text   string `json:"text"`
	FromMe bool   `json:"from_me"`
	Sender string `json:"sender"`
}

// Command is what gmessages-tui asks.
type Command struct {
	Cmd string `json:"cmd"`
	// For "pair_google": the cookies of a browser signed in to the Google account.
	Cookies map[string]string `json:"cookies,omitempty"`
	// For "download": the request, answered by a "downloaded" event of the same id ("done"
	// for "archive", "restore" and "delete_chat", of the conversation below), the
	// message and its media to download.
	ID      uint64   `json:"id,omitempty"`
	Message string   `json:"message,omitempty"`
	Media   []Medium `json:"media,omitempty"`
	// For "fetch": the conversations whose last messages to fetch; for "lookup", those to
	// ask about.
	Conversations []string `json:"conversations,omitempty"`
	// For "send" (answered by a "sent" event of the same id): the conversation, the text,
	// the message it replies to and the attachments.
	Conversation string       `json:"conversation,omitempty"`
	Text         string       `json:"text,omitempty"`
	Reply        string       `json:"reply,omitempty"`
	Attachments  []Attachment `json:"attachments,omitempty"`
	// For "read": the conversation (above) is read up to this message.
	UpTo string `json:"up_to,omitempty"`
}

// Attachment is a downloaded medium.
type Attachment struct {
	ContentType string `json:"content_type"`
	Name        string `json:"name,omitempty"`
	Data        []byte `json:"data"`
	Width       int64  `json:"width,omitempty"`
	Height      int64  `json:"height,omitempty"`
}

var (
	out     = json.NewEncoder(os.Stdout)
	outLock sync.Mutex
)

// emit writes one event line.
func emit(kind string, fields map[string]any) {
	if fields == nil {
		fields = map[string]any{}
	}
	fields["type"] = kind
	outLock.Lock()
	defer outLock.Unlock()
	_ = out.Encode(fields)
}

func main() {
	sessionPath := flag.String("session", "", "file holding the session")
	logPath := flag.String("log", "", "log file (none by default)")
	flag.Parse()
	if *sessionPath == "" {
		fmt.Fprintln(os.Stderr, "usage: gmd --session FILE [--log FILE]")
		os.Exit(2)
	}
	if err := run(*sessionPath, *logPath); err != nil {
		emit("fatal", map[string]any{"message": err.Error()})
		os.Exit(1)
	}
}

// gmd is the state: one client at a time (a failed pairing starts over with a new one).
type gmd struct {
	sessionPath string
	logger      zerolog.Logger
	ctx         context.Context

	client  *libgm.Client
	pairing atomic.Bool
	ready   atomic.Bool
	// Retrying to make the session active.
	retrying atomic.Bool

	lock sync.Mutex
	// Our participant ids: in conversations, and of the phone's SIMs.
	self map[string]bool
	// The conversations listed, for the names of their participants.
	convs map[string]*gmproto.Conversation
	// Messages whose media were asked in full, waiting for their update.
	waiting map[string]chan *gmproto.Message
	// The phone's SIMs, by their participant id.
	sims map[string]*gmproto.SIMCard
	// Messages sent, by temporary id, waiting for their echo (which gives their id).
	echoes map[string]chan *gmproto.Message
}

// echoWait is how long a message sent has to come back from the phone with its id.
const echoWait = 10 * time.Second

// sendRetries are the waits before sending again after a transient failure.
var sendRetries = []time.Duration{3 * time.Second, 8 * time.Second}

// fullMediumWait is how long the phone has to send a medium in full, once asked.
const fullMediumWait = 30 * time.Second

func (g *gmd) addSelf(id string) {
	if id == "" {
		return
	}
	g.lock.Lock()
	defer g.lock.Unlock()
	g.self[id] = true
}

func (g *gmd) isSelf(id string) bool {
	g.lock.Lock()
	defer g.lock.Unlock()
	return g.self[id]
}

func run(sessionPath, logPath string) error {
	logger := zerolog.Nop()
	if logPath != "" {
		file, err := os.OpenFile(logPath, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o600)
		if err != nil {
			return err
		}
		defer file.Close()
		// Not trace: it logs the bodies, messages included.
		logger = zerolog.New(file).Level(zerolog.DebugLevel).With().Timestamp().Logger()
	}
	emit("hello", map[string]any{"protocol": protocol})
	// The name of this device on the phone (Device pairing), given when pairing.
	util.BrowserDetailsMessage.OS = "gmessages-tui"

	auth, err := loadSession(sessionPath)
	if err != nil {
		return err
	}
	ctx, cancel := context.WithCancel(logger.WithContext(context.Background()))
	defer cancel()
	g := &gmd{
		sessionPath: sessionPath, logger: logger, ctx: ctx,
		self:    map[string]bool{"1": true},
		convs:   map[string]*gmproto.Conversation{},
		waiting: map[string]chan *gmproto.Message{},
		sims:    map[string]*gmproto.SIMCard{},
		echoes:  map[string]chan *gmproto.Message{},
	}
	if auth != nil {
		g.newClient(auth)
		if err := g.client.Connect(ctx); err != nil && g.ended(err) {
			g.expired(err)
		} else if err != nil {
			return fmt.Errorf("cannot connect: %w", err)
		}
	} else {
		emit("login", nil)
	}
	defer func() {
		if g.client != nil {
			g.client.Disconnect()
			if g.client.IsLoggedIn() {
				g.save()
			}
		}
	}()
	return g.commands(os.Stdin)
}

func (g *gmd) newClient(auth *libgm.AuthData) {
	if g.client != nil {
		g.client.Disconnect()
	}
	g.client = libgm.NewClient(auth, nil, g.logger, exhttp.SensibleClientSettings)
	g.client.SetEventHandler(g.handle)
}

func (g *gmd) save() {
	if err := saveSession(g.sessionPath, g.client.AuthData); err != nil {
		emit("status", map[string]any{"message": "cannot save the session: " + err.Error()})
	}
}

// handle gets the events of libgm. It is called synchronously: requests go in goroutines.
func (g *gmd) handle(evt any) {
	switch evt := evt.(type) {
	case *events.PairSuccessful:
		g.save()
		emit("paired", nil)
	case *events.AuthTokenRefreshed:
		g.save()
	case *gmproto.UserAlertEvent:
		// The session is active: the phone answers requests now.
		if evt.GetAlertType() == gmproto.AlertType_BROWSER_ACTIVE && g.ready.CompareAndSwap(false, true) {
			emit("connected", nil)
			go g.listConversations()
		}
	case *events.HackySetActiveMayFail:
		// The phone may miss the first request to make this session the active one.
		go func() {
			time.Sleep(7 * time.Second)
			if !g.ready.Load() {
				if err := g.client.SetActiveSession(g.ctx); err != nil {
					g.logger.Err(err).Msg("cannot set the session active")
				}
			}
		}()
	case *libgm.WrappedMessage:
		// Updated, e.g. once the phone has sent a medium in full.
		g.lock.Lock()
		waiting := g.waiting[evt.GetMessageID()]
		if echo := g.echoes[evt.GetTmpID()]; evt.GetTmpID() != "" && echo != nil {
			select {
			case echo <- evt.Message:
			default:
			}
		}
		g.lock.Unlock()
		if waiting != nil {
			select {
			case waiting <- evt.Message:
			default:
			}
		}
		g.live(evt)
	case *gmproto.Conversation:
		// New, or changed (name, last message, folder…).
		g.logger.Debug().Str("conversation", evt.GetConversationID()).
			Str("status", evt.GetStatus().String()).Msg("conversation updated")
		g.lock.Lock()
		g.convs[evt.GetConversationID()] = evt
		for _, p := range evt.GetParticipants() {
			if p.GetIsMe() && p.GetID().GetParticipantID() != "" {
				g.self[p.GetID().GetParticipantID()] = true
			}
		}
		g.lock.Unlock()
		emit("conversation", map[string]any{"conversation": conversations([]*gmproto.Conversation{evt})[0]})
	case *gmproto.TypingData:
		g.typing(evt)
	case *events.PingFailed:
		// Making the session active failed (network, Google): again until it is.
		if !g.ready.Load() && g.retrying.CompareAndSwap(false, true) {
			emit("status", map[string]any{"message": "Google Messages is not answering, retrying\u2026"})
			go g.retryActive()
		}
	case *gmproto.Settings:
		for _, sim := range evt.GetSIMCards() {
			id := sim.GetSIMParticipant().GetID()
			g.addSelf(id)
			g.lock.Lock()
			g.sims[id] = sim
			g.lock.Unlock()
		}
	case *events.PhoneNotResponding:
		emit("status", map[string]any{"message": "The phone is not responding"})
	case *events.PhoneRespondingAgain:
		emit("status", map[string]any{"message": "The phone is back"})
	case *events.ListenFatalError:
		if g.ended(evt.Error) {
			go g.expired(evt.Error)
			return
		}
		emit("fatal", map[string]any{"message": evt.Error.Error()})
		os.Exit(1)
	case *events.GaiaLoggedOut, *gmproto.RevokePairData:
		_ = os.Remove(g.sessionPath)
		emit("fatal", map[string]any{"message": "unpaired from the phone: run again to pair"})
		os.Exit(1)
	}
}

// live says a message received, sent from the phone, or updated (reactions, status…):
// "message", `old` if it is not news (synced from before).
func (g *gmd) live(evt *libgm.WrappedMessage) {
	g.lock.Lock()
	names := participantNames(g.convs[evt.GetConversationID()])
	g.lock.Unlock()
	if msg, ok := g.message(evt.Message, names); ok {
		emit("message", map[string]any{
			"conversation": evt.GetConversationID(),
			"message":      msg,
			"old":          evt.IsOld,
		})
	}
}

// typing says someone started or stopped typing: "typing".
func (g *gmd) typing(evt *gmproto.TypingData) {
	number := evt.GetUser().GetNumber()
	g.lock.Lock()
	defer g.lock.Unlock()
	for _, p := range g.convs[evt.GetConversationID()].GetParticipants() {
		if p.GetIsMe() || (p.GetID().GetNumber() != number && p.GetFormattedNumber() != number) {
			continue
		}
		emit("typing", map[string]any{
			"conversation": evt.GetConversationID(),
			"author":       p.GetID().GetParticipantID(),
			"name":         participantName(p),
			"started":      evt.GetType() == gmproto.TypingTypes_STARTED_TYPING,
		})
		return
	}
}

// retryActive asks again, every few seconds, to make the session active, until it is.
func (g *gmd) retryActive() {
	defer g.retrying.Store(false)
	for !g.ready.Load() {
		select {
		case <-time.After(10 * time.Second):
		case <-g.ctx.Done():
			return
		}
		if g.ready.Load() {
			return
		}
		if err := g.client.SetActiveSession(g.ctx); err != nil {
			g.logger.Err(err).Msg("cannot set the session active")
		}
	}
}

// ended tells whether err means the Google session of a Google account pairing has ended:
// signed out, or ended by Google.
func (g *gmd) ended(err error) bool {
	return g.client.AuthData.IsGoogleAccount() &&
		(errors.Is(err, events.ErrInvalidCredentials) || strings.Contains(err.Error(), "401"))
}

// expired says the Google session has ended: new cookies of the account sign in again,
// keeping the pairing (see reauth).
func (g *gmd) expired(err error) {
	g.logger.Warn().Err(err).Msg("Google session ended")
	g.client.Disconnect()
	g.pairing.Store(false)
	emit("expired", map[string]any{"message": "the Google session has ended (signed out?)"})
}

// listConversations says the conversations of the inbox, then those archived, in spam or
// in the trash: "ready", with `since` the time from which every inbox conversation is listed
// (0: all of them are).
func (g *gmd) listConversations() {
	resp, err := g.client.ListConversations(g.ctx, &gmproto.ListConversationsRequest{
		Count:  conversationCount,
		Folder: gmproto.ListConversationsRequest_INBOX,
	})
	if err != nil {
		g.ready.Store(false)
		emit("status", map[string]any{"message": "cannot list the conversations: " + err.Error()})
		return
	}
	convs := resp.GetConversations()
	var since int64
	if len(convs) >= conversationCount {
		since = convs[0].GetLastMessageTimestamp()
		for _, c := range convs {
			since = min(since, c.GetLastMessageTimestamp())
		}
		since /= 1000
	}
	for _, folder := range []gmproto.ListConversationsRequest_Folder{
		gmproto.ListConversationsRequest_ARCHIVE,
		gmproto.ListConversationsRequest_SPAM_BLOCKED,
	} {
		resp, err := g.client.ListConversations(g.ctx, &gmproto.ListConversationsRequest{
			Count:  conversationCount,
			Folder: folder,
		})
		if err != nil {
			g.logger.Err(err).Str("folder", folder.String()).Msg("cannot list the conversations")
			continue
		}
		convs = append(convs, resp.GetConversations()...)
	}
	// The trash has no documented folder: those undocumented, keeping only what is in it.
	for _, f := range []gmproto.ListConversationsRequest_Folder{3, 4} {
		resp, err := g.client.ListConversations(g.ctx, &gmproto.ListConversationsRequest{
			Count:  conversationCount,
			Folder: f,
		})
		if err != nil {
			g.logger.Debug().Err(err).Int32("folder", int32(f)).Msg("no trash there")
			continue
		}
		for _, c := range resp.GetConversations() {
			if c.GetStatus() == gmproto.ConversationStatus_TRASH_FOLDER {
				convs = append(convs, c)
			}
		}
	}
	g.lock.Lock()
	for _, c := range convs {
		g.convs[c.GetConversationID()] = c
		for _, p := range c.GetParticipants() {
			if p.GetIsMe() && p.GetID().GetParticipantID() != "" {
				g.self[p.GetID().GetParticipantID()] = true
			}
		}
	}
	g.lock.Unlock()
	emit("ready", map[string]any{"conversations": conversations(convs), "since": since})
}

// lookup asks the phone about the conversations ids, missing from its lists (in the trash,
// or deleted): "conversation" for those it has, "gone" for those it has not.
func (g *gmd) lookup(ids []string) {
	for _, id := range ids {
		conv, err := g.client.GetConversation(g.ctx, id)
		switch {
		case err != nil:
			g.logger.Err(err).Str("conversation", id).Msg("cannot look the conversation up")
		case conv == nil || conv.GetConversationID() == "" ||
			conv.GetStatus() == gmproto.ConversationStatus_DELETED:
			emit("gone", map[string]any{"conversation": id})
		default:
			g.lock.Lock()
			g.convs[id] = conv
			g.lock.Unlock()
			emit("conversation", map[string]any{"conversation": conversations([]*gmproto.Conversation{conv})[0]})
		}
	}
}

// fetch fetches the last messages of the conversations ids, in order, a few at a time: one
// "history" event each.
func (g *gmd) fetch(ids []string) {
	queue := make(chan string)
	var wg sync.WaitGroup
	for range historyWorkers {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for id := range queue {
				resp, err := g.client.FetchMessages(g.ctx, id, historyCount, nil)
				if err != nil {
					g.logger.Err(err).Str("conversation", id).Msg("cannot fetch the messages")
					continue
				}
				g.lock.Lock()
				names := participantNames(g.convs[id])
				g.lock.Unlock()
				list := make([]Message, 0, len(resp.GetMessages()))
				for _, m := range resp.GetMessages() {
					if msg, ok := g.message(m, names); ok {
						list = append(list, msg)
					}
				}
				emit("history", map[string]any{"conversation": id, "messages": list})
			}
		}()
	}
	for _, id := range ids {
		select {
		case queue <- id:
		case <-g.ctx.Done():
		}
	}
	close(queue)
	wg.Wait()
}

// commands handles the commands from gmessages-tui until "quit" or the end of stdin.
func (g *gmd) commands(in io.Reader) error {
	lines := bufio.NewScanner(in)
	lines.Buffer(make([]byte, 64*1024), 1024*1024)
	for lines.Scan() {
		var cmd Command
		if err := json.Unmarshal(lines.Bytes(), &cmd); err != nil {
			emit("status", map[string]any{"message": "bad command: " + err.Error()})
			continue
		}
		switch cmd.Cmd {
		case "quit":
			return nil
		case "download":
			go g.download(cmd.ID, cmd.Message, cmd.Media)
		case "fetch":
			go g.fetch(cmd.Conversations)
		case "lookup":
			go g.lookup(cmd.Conversations)
		case "archive", "restore", "delete_chat":
			go func() {
				var err error
				switch cmd.Cmd {
				case "archive":
					err = g.setArchived(cmd.Conversation, true)
				case "restore":
					err = g.setArchived(cmd.Conversation, false)
				default:
					err = g.deleteChat(cmd.Conversation)
				}
				if err != nil {
					emit("done", map[string]any{"id": cmd.ID, "error": err.Error()})
				} else {
					emit("done", map[string]any{"id": cmd.ID})
				}
			}()
		case "read":
			go func() {
				if err := g.client.MarkRead(g.ctx, cmd.Conversation, cmd.UpTo); err != nil {
					g.logger.Err(err).Str("conversation", cmd.Conversation).Msg("cannot mark read")
				}
			}()
		case "send":
			go func() {
				msg, err := g.send(cmd)
				if err != nil {
					emit("sent", map[string]any{"id": cmd.ID, "error": err.Error()})
				} else {
					emit("sent", map[string]any{"id": cmd.ID, "conversation": cmd.Conversation, "message": msg})
				}
			}()
		case "pair_google", "pair_qr":
			if !g.pairing.CompareAndSwap(false, true) {
				emit("status", map[string]any{"message": "already pairing"})
				continue
			}
			if cmd.Cmd == "pair_google" && g.client != nil && g.client.AuthData.IsGoogleAccount() {
				go g.reauth(cmd.Cookies)
				continue
			}
			g.fresh(cmd.Cmd == "pair_google", cmd.Cookies)
		default:
			emit("status", map[string]any{"message": "unknown command " + cmd.Cmd})
		}
	}
	return lines.Err()
}

// download downloads media, of message, and says "downloaded" with them, or the error.
// Those the phone has sent only as previews are asked in full first.
func (g *gmd) download(id uint64, message string, media []Medium) {
	if len(media) == 0 {
		emit("downloaded", map[string]any{"id": id, "error": "no media for this message"})
		return
	}
	attachments := make([]Attachment, 0, len(media))
	for _, m := range media {
		m = g.full(message, m)
		data, err := g.downloadMedium(m)
		if err != nil {
			emit("downloaded", map[string]any{"id": id, "error": err.Error()})
			return
		}
		attachments = append(attachments, Attachment{
			ContentType: m.Mime, Name: m.Name, Data: data, Width: m.Width, Height: m.Height,
		})
	}
	emit("downloaded", map[string]any{"id": id, "attachments": attachments})
}

// full is m, a medium of message, in full if the phone sends it in time once asked.
func (g *gmd) full(message string, m Medium) Medium {
	if m.MediaID != "" || m.Part == "" {
		return m
	}
	updates := make(chan *gmproto.Message, 4)
	g.lock.Lock()
	g.waiting[message] = updates
	g.lock.Unlock()
	defer func() {
		g.lock.Lock()
		delete(g.waiting, message)
		g.lock.Unlock()
	}()
	if _, err := g.client.GetFullSizeImage(g.ctx, message, m.Part); err != nil {
		g.logger.Err(err).Msg("cannot ask the phone for a full medium")
		return m
	}
	timeout := time.After(fullMediumWait)
	for {
		select {
		case update := <-updates:
			for _, part := range update.GetMessageInfo() {
				data, ok := part.GetData().(*gmproto.MessageInfo_MediaContent)
				if ok && part.GetActionMessageID() == m.Part && data.MediaContent.GetMediaID() != "" {
					return medium(data.MediaContent, m.Part)
				}
			}
		case <-timeout:
			g.logger.Warn().Msg("the phone has not sent the full medium in time")
			return m
		}
	}
}

func (g *gmd) downloadMedium(m Medium) ([]byte, error) {
	g.logger.Debug().
		Bool("media_id", m.MediaID != "").
		Bool("thumbnail_id", m.ThumbID != "").
		Int("inline_bytes", len(m.Inline)).
		Int64("size", m.Size).
		Msg("downloading a medium")
	// The best there is: the medium, its preview, the small preview in the message.
	id, key := m.MediaID, m.Key
	if id == "" {
		id, key = m.ThumbID, m.ThumbKey
	}
	if id == "" {
		if len(m.Inline) > 0 {
			return m.Inline, nil
		}
		return nil, errors.New("not downloaded by the phone yet")
	}
	body, err := g.client.DownloadMedia(id, key)
	if err != nil {
		return nil, err
	}
	defer body.Close()
	return io.ReadAll(body)
}

// send sends cmd's text and attachments to its conversation, from the SIM the conversation
// uses, and returns the message as the phone echoes it (with its id).
func (g *gmd) send(cmd Command) (Message, error) {
	g.lock.Lock()
	conv := g.convs[cmd.Conversation]
	g.lock.Unlock()
	if conv == nil {
		// Not listed yet (the interface opens on what is stored before): asked.
		var err error
		if conv, err = g.client.GetConversation(g.ctx, cmd.Conversation); err != nil {
			return Message{}, fmt.Errorf("unknown conversation: %w", err)
		}
		g.lock.Lock()
		g.convs[cmd.Conversation] = conv
		g.lock.Unlock()
	}
	var infos []*gmproto.MessageInfo
	for _, a := range cmd.Attachments {
		media, err := g.client.UploadMedia(a.Data, a.Name, a.ContentType)
		if err != nil {
			return Message{}, fmt.Errorf("cannot upload %s: %w", a.ContentType, err)
		}
		infos = append(infos, &gmproto.MessageInfo{
			Data: &gmproto.MessageInfo_MediaContent{MediaContent: media},
		})
	}
	if cmd.Text != "" {
		infos = append(infos, &gmproto.MessageInfo{
			Data: &gmproto.MessageInfo_MessageContent{
				MessageContent: &gmproto.MessageContent{Content: cmd.Text},
			},
		})
	}
	if len(infos) == 0 {
		return Message{}, errors.New("nothing to send")
	}
	outgoing := conv.GetDefaultOutgoingID()
	g.lock.Lock()
	sim := g.sims[outgoing]
	g.lock.Unlock()
	tmp := util.GenerateTmpID()
	req := &gmproto.SendMessageRequest{
		ConversationID: cmd.Conversation,
		MessagePayload: &gmproto.MessagePayload{
			TmpID:          tmp,
			ConversationID: cmd.Conversation,
			ParticipantID:  outgoing,
			TmpID2:         tmp,
			MessageInfo:    infos,
		},
		SIMPayload: sim.GetSIMData().GetSIMPayload(),
		TmpID:      tmp,
	}
	if cmd.Reply != "" {
		req.Reply = &gmproto.ReplyPayload{MessageID: cmd.Reply}
	}

	echo := make(chan *gmproto.Message, 4)
	g.lock.Lock()
	g.echoes[tmp] = echo
	g.lock.Unlock()
	defer func() {
		g.lock.Lock()
		delete(g.echoes, tmp)
		g.lock.Unlock()
	}()
	resp, err := g.client.SendMessage(g.ctx, req)
	for attempt := 0; err == nil && transient(resp.GetStatus()) && attempt < len(sendRetries); attempt++ {
		time.Sleep(sendRetries[attempt])
		resp, err = g.client.SendMessage(g.ctx, req)
	}
	if err != nil {
		return Message{}, err
	}
	if resp.GetStatus() != gmproto.SendMessageResponse_SUCCESS {
		return Message{}, fmt.Errorf("not sent (%s)", resp.GetStatus())
	}

	names := participantNames(conv)
	select {
	case m := <-echo:
		if msg, ok := g.message(m, names); ok {
			return msg, nil
		}
	case <-time.After(echoWait):
		g.logger.Warn().Msg("no echo of the message sent")
	}
	// Sent, but its id is not known: the temporary one, until the conversation is fetched.
	return Message{
		ID: tmp, TimestampMS: time.Now().UnixMilli(), FromMe: true, SenderID: outgoing,
		Text: cmd.Text, Quote: cmd.Reply,
	}, nil
}

// transient tells whether sending may work if tried again.
func transient(status gmproto.SendMessageResponse_Status) bool {
	return status == gmproto.SendMessageResponse_FAILURE_2 ||
		status == gmproto.SendMessageResponse_FAILURE_3
}

// fresh pairs from scratch, with the cookies of the Google account or by QR code.
func (g *gmd) fresh(google bool, cookies map[string]string) {
	auth := libgm.NewAuthData()
	if google {
		auth.SetCookies(cookies)
	}
	g.newClient(auth)
	go g.pair(google)
}

// reauth signs the pairing in again with new cookies, as long as they are of the same
// account: no emoji to tap. Pairs from scratch otherwise.
func (g *gmd) reauth(cookies map[string]string) {
	auth := g.client.AuthData
	auth.SetCookies(cookies)
	err := g.client.FetchConfig(g.ctx)
	if err == nil {
		if email := g.client.Config.GetDeviceInfo().GetEmail(); email != auth.Mobile.GetSourceID() {
			err = fmt.Errorf("signed in to %s, paired with %s", email, auth.Mobile.GetSourceID())
		}
	}
	if err == nil {
		err = g.client.Connect(g.ctx)
	}
	if err != nil {
		g.logger.Warn().Err(err).Msg("cannot sign the pairing in again, pairing from scratch")
		g.fresh(true, cookies)
		return
	}
	g.save()
	emit("paired", nil)
}

// pair pairs the client: says "qr" or "emoji", then "paired", or "pair_failed".
func (g *gmd) pair(google bool) {
	pair := g.pairQR
	if google {
		pair = g.pairGoogle
	}
	if err := pair(); err != nil {
		emit("pair_failed", map[string]any{"message": err.Error()})
		g.pairing.Store(false)
	}
}

func (g *gmd) pairQR() error {
	url, err := g.client.StartLogin(g.ctx)
	if err != nil {
		return fmt.Errorf("cannot start pairing: %w", err)
	}
	emit("qr", map[string]any{"url": url})
	return nil
}

func (g *gmd) pairGoogle() error {
	if err := g.client.FetchConfig(g.ctx); err != nil {
		return fmt.Errorf("the cookies are not accepted (signed out, or expired?): %w", err)
	}
	err := g.client.DoGaiaPairing(g.ctx, func(emoji string) {
		emit("emoji", map[string]any{"emoji": emoji})
	})
	switch {
	case err == nil:
		return nil
	case errors.Is(err, libgm.ErrNoDevicesFound):
		return errors.New("no phone found for this Google account: in Google Messages, Device pairing, is it signed in?")
	case errors.Is(err, libgm.ErrPairingInitTimeout):
		return errors.New("the phone does not respond: is it online?")
	case errors.Is(err, libgm.ErrIncorrectEmoji):
		return errors.New("wrong emoji tapped on the phone")
	case errors.Is(err, libgm.ErrPairingCancelled):
		return errors.New("cancelled on the phone")
	case errors.Is(err, libgm.ErrPairingTimeout):
		return errors.New("no emoji tapped on the phone in time")
	case strings.Contains(err.Error(), "HTTP 401"):
		return errors.New("Google refused the cookies (signed out?)")
	default:
		return err
	}
}

// participantNames maps the participants of c to their names (their numbers if unnamed).
func participantNames(c *gmproto.Conversation) map[string]string {
	names := map[string]string{}
	for _, p := range c.GetParticipants() {
		names[p.GetID().GetParticipantID()] = participantName(p)
	}
	return names
}

func participantName(p *gmproto.Participant) string {
	if name := p.GetFullName(); name != "" {
		return name
	}
	if number := p.GetFormattedNumber(); number != "" {
		return number
	}
	return p.GetID().GetNumber()
}

// message converts m; false for what is not a message (protocol changes, group creation…).
func (g *gmd) message(m *gmproto.Message, names map[string]string) (Message, bool) {
	status := m.GetMessageStatus().GetStatus()
	if strings.Contains(status.String(), "TOMBSTONE") {
		return Message{}, false
	}
	sender := m.GetParticipantID()
	msg := Message{
		ID:          m.GetMessageID(),
		TimestampMS: m.GetTimestamp() / 1000,
		// Sent from the phone (or here): its status says so, whichever SIM sent it.
		FromMe: strings.HasPrefix(status.String(), "OUTGOING_") || g.isSelf(sender) ||
			m.GetSenderParticipant().GetIsMe(),
		SenderID: sender,
		Sender:   names[sender],
		Quote:    m.GetReplyMessage().GetMessageID(),
		Deleted:  status == gmproto.MessageStatusType_MESSAGE_DELETED,
	}
	if p := m.GetSenderParticipant(); msg.Sender == "" && p != nil {
		msg.Sender = participantName(p)
	}
	var text []string
	if subject := m.GetSubject(); subject != "" {
		text = append(text, subject)
	}
	for _, part := range m.GetMessageInfo() {
		switch data := part.GetData().(type) {
		case *gmproto.MessageInfo_MessageContent:
			if content := data.MessageContent.GetContent(); content != "" {
				text = append(text, content)
			}
		case *gmproto.MessageInfo_MediaContent:
			msg.Media = append(msg.Media, medium(data.MediaContent, part.GetActionMessageID()))
		}
	}
	msg.Text = strings.Join(text, "\n")
	for _, r := range m.GetReactions() {
		emoji := r.GetData().GetUnicode()
		if emoji == "" {
			continue
		}
		for _, id := range r.GetParticipantIDs() {
			msg.Reactions = append(msg.Reactions, Reaction{
				Emoji: emoji, SenderID: id, Sender: names[id], Mine: g.isSelf(id),
			})
		}
	}
	return msg, true
}

func conversations(convs []*gmproto.Conversation) []Conversation {
	list := make([]Conversation, 0, len(convs))
	for _, c := range convs {
		conv := Conversation{
			ID:          c.GetConversationID(),
			Name:        c.GetName(),
			Group:       c.GetIsGroupChat(),
			TimestampMS: c.GetLastMessageTimestamp() / 1000,
			Folder:      folder(c.GetStatus()),
			Deleted:     c.GetStatus() == gmproto.ConversationStatus_DELETED,
			Unread:      c.GetUnread(),
		}
		if latest := c.GetLatestMessage(); latest != nil {
			conv.Latest = &Latest{
				ID:     c.GetLatestMessageID(),
				Text:   latest.GetDisplayContent(),
				FromMe: latest.GetFromMe() != 0,
				Sender: latest.GetDisplayName(),
			}
		}
		list = append(list, conv)
	}
	return list
}

// Actions of an update of conversation unknown to libgm, as Messages for web sends them.
const (
	actionUnarchive gmproto.ConversationActionStatus = 3
	actionArchive   gmproto.ConversationActionStatus = 4
)

// setArchived moves the conversation id to the archive, or back to the inbox, as Messages
// for web does: an action, and the conversation named again in field 6 ({1: id}), unknown
// to libgm. A new status (ARCHIVED, ACTIVE) in the data is answered success, and ignored.
func (g *gmd) setArchived(id string, archive bool) error {
	action := actionUnarchive
	if archive {
		action = actionArchive
	}
	req := &gmproto.UpdateConversationRequest{Action: action, ConversationID: id}
	ref := protowire.AppendTag(nil, 1, protowire.BytesType)
	ref = protowire.AppendString(ref, id)
	field6 := protowire.AppendTag(nil, 6, protowire.BytesType)
	field6 = protowire.AppendBytes(field6, ref)
	req.ProtoReflect().SetUnknown(field6)
	resp, err := g.client.UpdateConversation(g.ctx, req)
	if err != nil {
		return err
	} else if !resp.GetSuccess() {
		return errors.New("refused by the phone")
	}
	return nil
}

// deleteChat deletes the conversation id and its messages on the phone. The phone wants
// the number of the other person of a one-to-one conversation.
func (g *gmd) deleteChat(id string) error {
	g.lock.Lock()
	conv := g.convs[id]
	g.lock.Unlock()
	if conv == nil {
		var err error
		if conv, err = g.client.GetConversation(g.ctx, id); err != nil {
			return err
		}
	}
	var phone *string
	if !conv.GetIsGroupChat() {
		for _, p := range conv.GetParticipants() {
			if p.GetIsVisible() && !p.GetIsMe() && p.GetID().GetNumber() != "" {
				number := p.GetID().GetNumber()
				phone = &number
				break
			}
		}
	}
	resp, err := g.client.UpdateConversation(g.ctx, &gmproto.UpdateConversationRequest{
		Action:         gmproto.ConversationActionStatus_DELETE,
		ConversationID: id,
		Data: &gmproto.UpdateConversationRequest_DeleteData{DeleteData: &gmproto.DeleteConversationData{
			ConversationID: id,
			Phone:          phone,
		}},
	})
	if err != nil {
		return err
	} else if !resp.GetSuccess() {
		return errors.New("refused by the phone")
	}
	return nil
}

// folder says where a conversation of this status is filed, out of the inbox; "" for the
// inbox.
func folder(status gmproto.ConversationStatus) string {
	switch status {
	case gmproto.ConversationStatus_ARCHIVED, gmproto.ConversationStatus_KEEP_ARCHIVED:
		return "archive"
	case gmproto.ConversationStatus_SPAM_FOLDER, gmproto.ConversationStatus_BLOCKED_FOLDER:
		return "spam"
	case gmproto.ConversationStatus_TRASH_FOLDER:
		return "trash"
	}
	return ""
}

// loadSession reads the session saved by a previous pairing; nil when there is none.
func loadSession(path string) (*libgm.AuthData, error) {
	data, err := os.ReadFile(path)
	if errors.Is(err, os.ErrNotExist) {
		return nil, nil
	} else if err != nil {
		return nil, err
	}
	auth := &libgm.AuthData{}
	if err := json.Unmarshal(data, auth); err != nil {
		return nil, fmt.Errorf("cannot read the session %s: %w", path, err)
	}
	return auth, nil
}

// saveSession writes the session, readable by the user only.
func saveSession(path string, auth *libgm.AuthData) error {
	data, err := json.Marshal(auth)
	if err != nil {
		return err
	}
	tmp := path + ".tmp"
	if err := os.WriteFile(tmp, data, 0o600); err != nil {
		return err
	}
	return os.Rename(tmp, path)
}
