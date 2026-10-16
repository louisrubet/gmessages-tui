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
	"go.mau.fi/util/exhttp"
)

// Version of the protocol spoken on stdin/stdout, checked by gmessages-tui.
const protocol = 2

// Conversations listed when the session becomes active.
const conversationCount = 100

// Messages fetched per conversation at start, and conversations fetched at once.
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
	Media       []string   `json:"media,omitempty"`
	Reactions   []Reaction `json:"reactions,omitempty"`
	Deleted     bool       `json:"deleted,omitempty"`
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
	// For "download": the request, answered by a "downloaded" event of the same id, and the
	// message whose media to download.
	ID      uint64 `json:"id,omitempty"`
	Message string `json:"message,omitempty"`
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

	lock sync.Mutex
	// Our participant ids: in conversations, and of the phone's SIMs.
	self map[string]bool
	// The media of the messages shown, by message id, to download them.
	media map[string][]medium
}

// medium is a medium of a message, and its part id, to ask the phone for it in full.
type medium struct {
	content *gmproto.MediaContent
	part    string
}

// fullMediumWait is how long the phone has to send a medium in full, once asked.
const fullMediumWait = 30 * time.Second

// media is what is known of the media of message.
func (g *gmd) mediaOf(message string) []medium {
	g.lock.Lock()
	defer g.lock.Unlock()
	return g.media[message]
}

// keepMedia remembers the media of m, to download them.
func (g *gmd) keepMedia(m *gmproto.Message) {
	var media []medium
	for _, part := range m.GetMessageInfo() {
		if data, ok := part.GetData().(*gmproto.MessageInfo_MediaContent); ok {
			media = append(media, medium{data.MediaContent, part.GetActionMessageID()})
		}
	}
	g.lock.Lock()
	defer g.lock.Unlock()
	if len(media) == 0 {
		delete(g.media, m.GetMessageID())
	} else {
		g.media[m.GetMessageID()] = media
	}
}

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

	auth, err := loadSession(sessionPath)
	if err != nil {
		return err
	}
	ctx, cancel := context.WithCancel(logger.WithContext(context.Background()))
	defer cancel()
	g := &gmd{
		sessionPath: sessionPath, logger: logger, ctx: ctx,
		self:  map[string]bool{"1": true},
		media: map[string][]medium{},
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
		if len(g.mediaOf(evt.GetMessageID())) > 0 {
			g.keepMedia(evt.Message)
		}
	case *gmproto.Settings:
		for _, sim := range evt.GetSIMCards() {
			g.addSelf(sim.GetSIMParticipant().GetID())
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
	emit("ready", map[string]any{"conversations": conversations(resp.GetConversations())})
	go g.histories(resp.GetConversations())
}

// histories fetches the last messages of each conversation, the most recent ones first, a
// few at a time: one "history" event each.
func (g *gmd) histories(convs []*gmproto.Conversation) {
	for _, c := range convs {
		for _, p := range c.GetParticipants() {
			if p.GetIsMe() {
				g.addSelf(p.GetID().GetParticipantID())
			}
		}
	}
	queue := make(chan *gmproto.Conversation)
	var wg sync.WaitGroup
	for range historyWorkers {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for c := range queue {
				resp, err := g.client.FetchMessages(g.ctx, c.GetConversationID(), historyCount, nil)
				if err != nil {
					g.logger.Err(err).Str("conversation", c.GetConversationID()).Msg("cannot fetch the messages")
					continue
				}
				names := participantNames(c)
				list := make([]Message, 0, len(resp.GetMessages()))
				for _, m := range resp.GetMessages() {
					if msg, ok := g.message(m, names); ok {
						list = append(list, msg)
					}
				}
				emit("history", map[string]any{"conversation": c.GetConversationID(), "messages": list})
			}
		}()
	}
	for _, c := range convs {
		select {
		case queue <- c:
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
			go g.download(cmd.ID, cmd.Message)
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

// download downloads the media of message, and says "downloaded" with them, or the error.
// Those the phone has sent only as previews are asked in full first.
func (g *gmd) download(id uint64, message string) {
	media := g.mediaOf(message)
	if len(media) == 0 {
		emit("downloaded", map[string]any{"id": id, "error": "no media known for this message"})
		return
	}
	attachments := make([]Attachment, 0, len(media))
	for i := range media {
		m := g.full(message, i)
		data, err := g.downloadMedium(m)
		if err != nil {
			emit("downloaded", map[string]any{"id": id, "error": err.Error()})
			return
		}
		attachments = append(attachments, Attachment{
			ContentType: m.GetMimeType(),
			Name:        m.GetMediaName(),
			Data:        data,
			Width:       m.GetDimensions().GetWidth(),
			Height:      m.GetDimensions().GetHeight(),
		})
	}
	emit("downloaded", map[string]any{"id": id, "attachments": attachments})
}

// full is the i-th medium of message, in full if the phone sends it in time once asked.
func (g *gmd) full(message string, i int) *gmproto.MediaContent {
	m := g.mediaOf(message)[i]
	if m.content.GetMediaID() != "" || m.part == "" {
		return m.content
	}
	if _, err := g.client.GetFullSizeImage(g.ctx, message, m.part); err != nil {
		g.logger.Err(err).Msg("cannot ask the phone for a full medium")
		return m.content
	}
	deadline := time.Now().Add(fullMediumWait)
	for time.Now().Before(deadline) {
		time.Sleep(500 * time.Millisecond)
		if media := g.mediaOf(message); i < len(media) && media[i].content.GetMediaID() != "" {
			return media[i].content
		}
	}
	g.logger.Warn().Msg("the phone has not sent the full medium in time")
	return m.content
}

func (g *gmd) downloadMedium(m *gmproto.MediaContent) ([]byte, error) {
	g.logger.Debug().
		Bool("media_id", m.GetMediaID() != "").
		Bool("thumbnail_id", m.GetThumbnailMediaID() != "").
		Int("inline_bytes", len(m.GetMediaData())).
		Int64("size", m.GetSize()).
		Msg("downloading a medium")
	// The best there is: the medium, its preview, the small preview in the message.
	id, key := m.GetMediaID(), m.GetDecryptionKey()
	if id == "" {
		id, key = m.GetThumbnailMediaID(), m.GetThumbnailDecryptionKey()
	}
	if id == "" {
		if len(m.GetMediaData()) > 0 {
			return m.GetMediaData(), nil
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

// message converts m, and keeps its media to download them; false for what is not a
// message (protocol changes, group creation…).
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
	g.keepMedia(m)
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
			mime := data.MediaContent.GetMimeType()
			if mime == "" {
				mime = "application/octet-stream"
			}
			msg.Media = append(msg.Media, mime)
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
