// gmd: Google Messages for gmessages-tui, through libgm (the client of mautrix-gmessages).
//
//	gmd --session FILE [--log FILE]
//
// Speaks JSON lines: events on stdout, commands on stdin. Pairs with the phone by QR code
// when FILE holds no session, and keeps the session in FILE.
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
	"sync"

	"github.com/rs/zerolog"
	"go.mau.fi/mautrix-gmessages/pkg/libgm"
	"go.mau.fi/mautrix-gmessages/pkg/libgm/events"
	"go.mau.fi/mautrix-gmessages/pkg/libgm/gmproto"
	"go.mau.fi/util/exhttp"
)

// Version of the protocol spoken on stdin/stdout, checked by gmessages-tui.
const protocol = 1

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

func run(sessionPath, logPath string) error {
	logger := zerolog.Nop()
	if logPath != "" {
		file, err := os.OpenFile(logPath, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o600)
		if err != nil {
			return err
		}
		defer file.Close()
		logger = zerolog.New(file).With().Timestamp().Logger()
	}
	emit("hello", map[string]any{"protocol": protocol})

	auth, err := loadSession(sessionPath)
	if err != nil {
		return err
	}
	paired := auth != nil
	if !paired {
		auth = libgm.NewAuthData()
	}
	client := libgm.NewClient(auth, nil, logger, exhttp.SensibleClientSettings)
	save := func() {
		if err := saveSession(sessionPath, client.AuthData); err != nil {
			emit("status", map[string]any{"message": "cannot save the session: " + err.Error()})
		}
	}
	client.SetEventHandler(func(evt any) {
		switch evt := evt.(type) {
		case *events.PairSuccessful:
			save()
			emit("paired", nil)
		case *events.AuthTokenRefreshed:
			save()
		case *events.ClientReady:
			emit("ready", map[string]any{"conversations": conversations(evt.Conversations)})
		case *events.PhoneNotResponding:
			emit("status", map[string]any{"message": "The phone is not responding"})
		case *events.PhoneRespondingAgain:
			emit("status", map[string]any{"message": "The phone is back"})
		case *events.ListenFatalError:
			emit("fatal", map[string]any{"message": evt.Error.Error()})
			os.Exit(1)
		case *events.GaiaLoggedOut, *gmproto.RevokePairData:
			_ = os.Remove(sessionPath)
			emit("fatal", map[string]any{"message": "unpaired from the phone: run again to pair"})
			os.Exit(1)
		}
	})

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	if paired {
		if err := client.Connect(ctx); err != nil {
			return fmt.Errorf("cannot connect: %w", err)
		}
	} else {
		url, err := client.StartLogin(ctx)
		if err != nil {
			return fmt.Errorf("cannot start pairing: %w", err)
		}
		emit("qr", map[string]any{"url": url})
	}
	defer client.Disconnect()
	return commands(os.Stdin)
}

// commands handles the commands from gmessages-tui until "quit" or the end of stdin.
func commands(in io.Reader) error {
	lines := bufio.NewScanner(in)
	for lines.Scan() {
		var cmd struct {
			Cmd string `json:"cmd"`
		}
		if err := json.Unmarshal(lines.Bytes(), &cmd); err != nil {
			emit("status", map[string]any{"message": "bad command: " + err.Error()})
			continue
		}
		switch cmd.Cmd {
		case "quit":
			return nil
		default:
			emit("status", map[string]any{"message": "unknown command " + cmd.Cmd})
		}
	}
	return lines.Err()
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
