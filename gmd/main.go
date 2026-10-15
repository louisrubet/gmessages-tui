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
	g := &gmd{sessionPath: sessionPath, logger: logger, ctx: ctx}
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
