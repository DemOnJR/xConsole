// Command xconsole-whatsapp bridges a paired WhatsApp device to xConsole over stdio.
//
// # Why this exists
//
// xConsole is Rust, and no Rust library speaks WhatsApp's multi-device protocol —
// pairing by QR means a Noise handshake, the Signal double ratchet, and WhatsApp's
// protobuf dialect. whatsmeow does, so the smallest honest thing is this: a process
// that owns the WhatsApp session and nothing else.
//
// # Protocol
//
// Newline-delimited JSON, stdin for commands, stdout for events. Stdio rather than a
// socket on purpose: a listening socket is exactly the inbound surface remote control
// is designed not to have, and a pipe dies with its parent, so closing xConsole cannot
// leave a paired session running behind it. Logs go to stderr, never stdout, or they
// would be parsed as events.
//
// # What this deliberately does not know
//
// Nothing here decides who may command the agent. There is no allowlist, no prefix, no
// concept of a command. This process reports who said what; xConsole decides whether
// that person is allowed to be heard. Keeping the authorisation on the other side of
// the pipe means a swapped or compromised sidecar can deliver messages but cannot
// authorise one.
package main

import (
	"bufio"
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"os/signal"
	"strings"
	"sync"
	"syscall"
	"time"

	"go.mau.fi/whatsmeow"
	waBinary "go.mau.fi/whatsmeow/binary"
	"go.mau.fi/whatsmeow/proto/waE2E"
	"go.mau.fi/whatsmeow/store"
	"go.mau.fi/whatsmeow/store/sqlstore"
	"go.mau.fi/whatsmeow/types"
	"go.mau.fi/whatsmeow/types/events"
	waLog "go.mau.fi/whatsmeow/util/log"
	"google.golang.org/protobuf/proto"

	// Pure-Go SQLite. cgo would mean a C toolchain for every target we cross-compile
	// to, which is a heavy price for a store that holds one session.
	_ "modernc.org/sqlite"
)

// Event is one line of stdout. The Rust side matches on Type and ignores the rest, so
// adding a field here can never break an older host.
type Event struct {
	Type string `json:"type"`

	Code    string `json:"code,omitempty"`
	Message string `json:"message,omitempty"`

	JID      string `json:"jid,omitempty"`
	PushName string `json:"push_name,omitempty"`

	ID             string `json:"id,omitempty"`
	Chat           string `json:"chat,omitempty"`
	SenderID       string `json:"sender_id,omitempty"`
	SenderPhone    string `json:"sender_phone,omitempty"`
	SenderLID      string `json:"sender_lid,omitempty"`
	SenderUsername string `json:"sender_username,omitempty"`
	FromMe         bool   `json:"from_me,omitempty"`
	IsOurEcho      bool   `json:"is_our_echo,omitempty"`
	IsGroup        bool   `json:"is_group,omitempty"`
	Text           string `json:"text,omitempty"`

	Chats []Chat `json:"chats,omitempty"`
}

// Chat is one place the bridge could be restricted to.
//
// Without this the host could only offer a free-text box asking for a "group id", and a
// WhatsApp group id is an 18-digit number nobody has ever seen. So the answer was always
// to leave it blank, which means every conversation the linked account takes part in is
// read and evaluated.
type Chat struct {
	// Full JID, which is what the host matches against.
	ID string `json:"id"`
	// What it is called in WhatsApp.
	Name string `json:"name"`
	// "self" (the Note to Self chat) or "group".
	Kind string `json:"kind"`
}

// Command is one line of stdin.
type Command struct {
	Type string `json:"type"`
	Chat string `json:"chat"`
	Text string `json:"text"`
}

var (
	out   = json.NewEncoder(os.Stdout)
	outMu sync.Mutex
)

// emit writes one event. Serialised, because the message handler and the QR loop both
// call it from their own goroutines and interleaved JSON is unparseable.
func emit(ev Event) {
	outMu.Lock()
	defer outMu.Unlock()
	_ = out.Encode(ev)
}

func logf(format string, args ...any) {
	fmt.Fprintf(os.Stderr, format+"\n", args...)
}

// stderrLog is whatsmeow's logger, pointed at stderr.
//
// waLog.Stdout writes to stdout, which here is the event stream: one websocket warning
// lands in the middle of the JSON and the host stops understanding the bridge. Nothing
// but events may ever be written to stdout.
type stderrLog struct {
	module string
	debug  bool
}

func (l stderrLog) output(level, msg string, args ...any) {
	logf("[%s %s] %s", l.module, level, fmt.Sprintf(msg, args...))
}

func (l stderrLog) Errorf(msg string, args ...any) { l.output("ERROR", msg, args...) }
func (l stderrLog) Warnf(msg string, args ...any)  { l.output("WARN", msg, args...) }
func (l stderrLog) Infof(msg string, args ...any)  { l.output("INFO", msg, args...) }
func (l stderrLog) Debugf(msg string, args ...any) {
	if l.debug {
		l.output("DEBUG", msg, args...)
	}
}
func (l stderrLog) Sub(module string) waLog.Logger {
	return stderrLog{module: l.module + "/" + module, debug: l.debug}
}

func main() {
	storePath := flag.String("store", "", "path to the session database")
	flag.Parse()
	if *storePath == "" {
		logf("missing --store")
		os.Exit(2)
	}

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	// `_foreign_keys=on` is whatsmeow's expectation; `_pragma=busy_timeout` keeps a
	// concurrent read from failing outright while a write is in flight.
	dsn := fmt.Sprintf("file:%s?_pragma=foreign_keys(1)&_pragma=busy_timeout(5000)", *storePath)
	container, err := sqlstore.New(ctx, "sqlite", dsn, stderrLog{module: "db"})
	if err != nil {
		emit(Event{Type: "error", Message: "could not open the WhatsApp session store: " + err.Error()})
		os.Exit(1)
	}
	defer container.Close()

	device, err := container.GetFirstDevice(ctx)
	if err != nil {
		emit(Event{Type: "error", Message: "could not read the WhatsApp session: " + err.Error()})
		os.Exit(1)
	}
	if device == nil {
		device = container.NewDevice()
	}

	// The paired device's name, as it appears in WhatsApp's "Linked devices" list. The
	// user should be able to recognise and revoke it there without guessing.
	store.DeviceProps.Os = proto.String("xConsole")

	client := whatsmeow.NewClient(device, stderrLog{module: "wa"})
	b := &bridge{client: client, container: container, usernames: map[string]string{}}
	client.AddEventHandler(b.handle)

	if client.Store.ID == nil {
		// No session on disk: pair by QR. GetQRChannel must be called before Connect,
		// and yields a fresh code every ~20s until the phone scans one.
		qrChan, err := client.GetQRChannel(ctx)
		if err != nil {
			emit(Event{Type: "error", Message: "could not start pairing: " + err.Error()})
			os.Exit(1)
		}
		go func() {
			for item := range qrChan {
				switch item.Event {
				case whatsmeow.QRChannelEventCode:
					emit(Event{Type: "qr", Code: item.Code})
				case whatsmeow.QRChannelEventError:
					emit(Event{Type: "error", Message: item.Error.Error()})
				case "success":
					// The `paired` event is emitted from the connection handler, which
					// knows the JID; here we only know that scanning finished.
				case "timeout":
					emit(Event{Type: "error", Message: "the pairing code expired — start again"})
				}
			}
		}()
	}

	if err := client.Connect(); err != nil {
		emit(Event{Type: "error", Message: "could not connect to WhatsApp: " + err.Error()})
		os.Exit(1)
	}

	// Exiting when stdin closes is the whole lifecycle guarantee: xConsole going away
	// takes the WhatsApp session with it, with no orphaned process holding a linked
	// device open.
	done := make(chan struct{})
	go func() {
		defer close(done)
		b.readCommands(ctx)
	}()

	sig := make(chan os.Signal, 1)
	signal.Notify(sig, os.Interrupt, syscall.SIGTERM)
	select {
	case <-done:
	case <-sig:
	}
	client.Disconnect()
}

type bridge struct {
	client    *whatsmeow.Client
	container *sqlstore.Container

	// Usernames are resolved over the network and rarely change, so they are cached
	// for the life of the process. Without this, every inbound message would cost an
	// extra round trip before the agent could even be asked.
	mu        sync.Mutex
	usernames map[string]string
	sentIDs   map[string]time.Time
}

func (b *bridge) resolveSenderPhone(info *types.MessageInfo) string {
	if info == nil {
		return ""
	}
	if info.Sender.Server == types.DefaultUserServer && info.Sender.User != "" {
		return info.Sender.User
	}
	if info.SenderAlt.Server == types.DefaultUserServer && info.SenderAlt.User != "" {
		return info.SenderAlt.User
	}
	if info.IsFromMe && b.client.Store.ID != nil && b.client.Store.ID.User != "" {
		return b.client.Store.ID.User
	}
	if b.container != nil && b.container.LIDMap != nil && info.Sender.Server == types.HiddenUserServer {
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		if pn, err := b.container.LIDMap.GetPNForLID(ctx, info.Sender); err == nil && !pn.IsEmpty() {
			if pn.Server == types.DefaultUserServer && pn.User != "" {
				return pn.User
			}
		}
	}
	return ""
}

func (b *bridge) readCommands(ctx context.Context) {
	scanner := bufio.NewScanner(os.Stdin)
	// Replies are chunked to 4096 characters on the Rust side, but the default 64KB
	// scanner buffer is not worth relying on for something that would fail silently.
	scanner.Buffer(make([]byte, 0, 64*1024), 1024*1024)
	for scanner.Scan() {
		var cmd Command
		if err := json.Unmarshal(scanner.Bytes(), &cmd); err != nil {
			logf("unparseable command: %v", err)
			continue
		}
		switch cmd.Type {
		case "send":
			b.send(ctx, cmd)
		case "list_chats":
			b.listChats(ctx)
		case "logout":
			if err := b.client.Logout(ctx); err != nil {
				emit(Event{Type: "error", Message: "logout failed: " + err.Error()})
			}
			return
		default:
			logf("unknown command %q", cmd.Type)
		}
	}
}

func (b *bridge) send(ctx context.Context, cmd Command) {
	jid, err := parseChatJID(cmd.Chat)
	if err != nil {
		emit(Event{Type: "error", Message: "cannot reply: " + err.Error()})
		return
	}
	msg := &waE2E.Message{Conversation: proto.String(cmd.Text)}
	resp, err := b.client.SendMessage(ctx, jid, msg)
	if err != nil {
		emit(Event{Type: "error", Message: "could not send a reply: " + err.Error()})
		return
	}
	b.mu.Lock()
	if b.sentIDs == nil {
		b.sentIDs = make(map[string]time.Time)
	}
	b.sentIDs[resp.ID] = time.Now()
	now := time.Now()
	for id, t := range b.sentIDs {
		if now.Sub(t) > 10*time.Minute {
			delete(b.sentIDs, id)
		}
	}
	b.mu.Unlock()
}

// parseChatJID accepts either a full JID or the bare user part xConsole stores.
//
// The Rust side strips JIDs down to what a person would type, so the round trip has to
// put the server back. Group ids are long and numeric like phone numbers, so they are
// told apart by length rather than by shape.
func parseChatJID(chat string) (types.JID, error) {
	chat = strings.TrimSpace(chat)
	if chat == "" {
		return types.JID{}, fmt.Errorf("no chat given")
	}
	if strings.Contains(chat, "@") {
		return types.ParseJID(chat)
	}
	if len(chat) > 15 {
		// WhatsApp group ids are 18 digits; E.164 numbers are at most 15.
		return types.NewJID(chat, types.GroupServer), nil
	}
	return types.NewJID(chat, types.DefaultUserServer), nil
}

func (b *bridge) handle(rawEvt any) {
	switch evt := rawEvt.(type) {
	case *events.Connected, *events.PushNameSetting:
		jid := ""
		if b.client.Store.ID != nil {
			jid = b.client.Store.ID.String()
		}
		lid := ""
		if !b.client.Store.LID.IsEmpty() {
			lid = b.client.Store.LID.String()
		}
		emit(Event{Type: "connected", JID: jid, SenderLID: lid, PushName: b.client.Store.PushName})
	case *events.PairSuccess:
		lid := ""
		if !b.client.Store.LID.IsEmpty() {
			lid = b.client.Store.LID.String()
		}
		emit(Event{Type: "paired", JID: evt.ID.String(), SenderLID: lid, PushName: b.client.Store.PushName})
	case *events.Disconnected:
		emit(Event{Type: "disconnected"})
	case *events.LoggedOut:
		emit(Event{Type: "logged_out"})
	case *events.Message:
		b.onMessage(evt)
	}
}

func (b *bridge) onMessage(evt *events.Message) {
	text := extractText(evt.Message)
	if strings.TrimSpace(text) == "" {
		// Images, reactions, receipts. Reporting them would only give the host empty
		// messages to discard.
		return
	}
	b.mu.Lock()
	_, isOurEcho := b.sentIDs[evt.Info.ID]
	b.mu.Unlock()

	sender := evt.Info.Sender
	senderPhone := b.resolveSenderPhone(&evt.Info)
	senderLID := ""
	if sender.Server == types.HiddenUserServer {
		senderLID = sender.User
	}

	emit(Event{
		Type:           "message",
		ID:             evt.Info.ID,
		Chat:           evt.Info.Chat.String(),
		SenderID:       sender.String(),
		SenderPhone:    senderPhone,
		SenderLID:      senderLID,
		// Best effort: an unresolvable username simply means the allowlist has to name
		// the number instead. It must never fall back to the push name, which is a
		// display name anyone can set to anything.
		SenderUsername: b.username(sender),
		FromMe:         evt.Info.IsFromMe,
		IsOurEcho:      isOurEcho,
		IsGroup:        evt.Info.IsGroup,
		Text:           text,
	})
}

// extractText pulls the body out of the handful of message shapes that carry one.
//
// A plain message is `Conversation`; anything with formatting, a reply, or a link
// preview arrives as `ExtendedTextMessage` instead, and a user typing an instruction
// with a URL in it hits that path constantly.
func extractText(msg *waE2E.Message) string {
	if msg == nil {
		return ""
	}
	if c := msg.GetConversation(); c != "" {
		return c
	}
	if e := msg.GetExtendedTextMessage(); e != nil {
		return e.GetText()
	}
	// An edited message wraps the new content one level down.
	if p := msg.GetProtocolMessage(); p != nil && p.GetEditedMessage() != nil {
		return extractText(p.GetEditedMessage())
	}
	return ""
}

// listChats reports the chats the bridge can be restricted to: the account's own
// Note-to-Self chat, and every group it has joined.
//
// Deliberately not the full contact list. The point of restricting is to stop the agent
// reading conversations with other people; offering those as targets would invite
// exactly that, and a one-to-one chat with somebody else is not a place an unattended
// agent should be answering anyway.
func (b *bridge) listChats(ctx context.Context) {
	chats := []Chat{}
	if b.client.Store.ID != nil {
		self := b.client.Store.ID.ToNonAD()
		name := b.client.Store.PushName
		if name == "" {
			name = self.User
		}
		chats = append(chats, Chat{
			ID:   self.String(),
			Name: name + " (only me)",
			Kind: "self",
		})
	}

	groups, err := b.client.GetJoinedGroups(ctx)
	if err != nil {
		// The self chat is still worth returning: it is the safest option and the one
		// most people want, and it does not depend on this query.
		logf("could not list groups: %v", err)
	}
	for _, g := range groups {
		if g == nil {
			continue
		}
		name := g.Name
		if name == "" {
			name = g.JID.User
		}
		chats = append(chats, Chat{ID: g.JID.String(), Name: name, Kind: "group"})
	}
	emit(Event{Type: "chats", Chats: chats})
}

// username resolves a sender's WhatsApp username (the `@handle` form), or "".
//
// WhatsApp added usernames so people can be reached without sharing a phone number,
// and an allowlist that could not name one would force the user to write their number
// into a config field instead. whatsmeow asks for the `username` node in its usync
// queries but does not yet surface it on any public type, so this issues the query
// directly and reads the node.
//
// That means reaching through DangerousInternals, which is unstable by name. Every
// failure path returns "" rather than propagating: a username that cannot be resolved
// degrades to number-only matching, which still works.
func (b *bridge) username(jid types.JID) string {
	key := jid.User
	b.mu.Lock()
	if cached, ok := b.usernames[key]; ok {
		b.mu.Unlock()
		return cached
	}
	b.mu.Unlock()

	name := b.lookupUsername(jid)

	b.mu.Lock()
	b.usernames[key] = name
	b.mu.Unlock()
	return name
}

func (b *bridge) lookupUsername(jid types.JID) (name string) {
	defer func() {
		// DangerousInternals is explicitly not a stable API. A panic from a shape change
		// must cost a username, not the whole bridge.
		if r := recover(); r != nil {
			logf("username lookup panicked: %v", r)
			name = ""
		}
	}()

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	resp, err := b.client.DangerousInternals().Usync(
		ctx,
		[]types.JID{jid.ToNonAD()},
		"query", "interactive",
		[]waBinary.Node{{Tag: "username"}},
	)
	if err != nil || resp == nil {
		if err != nil {
			logf("username lookup failed: %v", err)
		}
		return ""
	}
	for _, child := range resp.GetChildren() {
		if child.Tag != "user" {
			continue
		}
		node := child.GetChildByTag("username")
		switch content := node.Content.(type) {
		case []byte:
			return strings.TrimPrefix(strings.TrimSpace(string(content)), "@")
		case string:
			return strings.TrimPrefix(strings.TrimSpace(content), "@")
		}
		// Some responses carry it as an attribute instead of node content.
		if v := node.AttrGetter().OptionalString("name"); v != "" {
			return strings.TrimPrefix(v, "@")
		}
	}
	return ""
}
