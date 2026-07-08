// Package nats is the NATS JetStream message-broker backend for blkit:
// subject-filtered pull consumers carry the job queue, per-instance subjects
// in an events stream carry instance events with latest-event replay, and a
// JetStream key-value bucket holds the worker registry. See
// specs/message-brokers/nats-message-broker.spec.md.
package nats

import (
	"context"
	"crypto/sha256"
	"crypto/tls"
	"encoding/base64"
	"encoding/hex"
	"errors"
	"fmt"
	"sort"
	"strings"
	"sync"
	"time"

	natsgo "github.com/nats-io/nats.go"
	"github.com/nats-io/nats.go/jetstream"

	bl "github.com/friendly-business-machines/blkit/core"
)

// Config holds the construction parameters for the NATS backend.
type Config struct {
	URL         string      // NATS server URL(s); required
	Credentials string      // optional path to a .creds file
	TLS         *tls.Config // nil = plaintext (development)

	// SubjectPrefix isolates deployments sharing a server; it prefixes every
	// subject, stream, and bucket this broker creates. Default "blkit". Must
	// contain only alphanumerics, '-' and '_'.
	SubjectPrefix string

	// Cipher optionally encrypts envelope payloads end to end; default nil.
	Cipher bl.PayloadCipher

	// RegistrationTTL is how long registrations outlive their last heartbeat
	// (default 90s — 3x the default 30s heartbeat interval).
	RegistrationTTL time.Duration

	// InFlightTimeout is how long a delivered job may stay unsettled before
	// JetStream redelivers it (the consumer AckWait; default 150s).
	InFlightTimeout time.Duration

	// EventRetention is how long instance events stay replayable to late
	// subscribers (the events stream MaxAge; default 1h).
	EventRetention time.Duration
}

// Broker implements bl.MessageBroker on NATS JetStream.
type Broker struct {
	cfg      Config
	nc       *natsgo.Conn
	js       jetstream.JetStream
	jobs     jetstream.Stream
	events   jetstream.Stream
	registry jetstream.KeyValue
	instMeta jetstream.KeyValue

	rootCtx context.Context
	stop    context.CancelFunc

	mu       sync.Mutex
	closed   bool
	inFlight map[string][]*inFlightJob // delivered, unsettled jobs per instance
	timers   map[string]*time.Timer    // pending JobResume timers per instance
}

var _ bl.MessageBroker = (*Broker)(nil)

const (
	opTimeout       = 10 * time.Second
	eventBufferSize = 64

	subjLifecycle = "lifecycle"
	subjTerminal  = "terminal"
	subjInputReq  = "inputreq"
	subjNode      = "node"
	subjErr       = "err"

	kindInstanceMeta = "inst.meta"

	tombstoneRemoved       = "removed"
	tombstoneHeartbeatLost = "heartbeat-lost"
)

// New connects to NATS, provisions the jobs stream, the events stream, and
// the registry / instance-metadata KV buckets, and starts the TTL sweeper.
func New(cfg Config) (*Broker, error) {
	if cfg.URL == "" {
		return nil, fmt.Errorf("nats message broker: URL is required")
	}
	if cfg.SubjectPrefix == "" {
		cfg.SubjectPrefix = "blkit"
	}
	if !validPrefix(cfg.SubjectPrefix) {
		return nil, fmt.Errorf("nats message broker: invalid SubjectPrefix %q", cfg.SubjectPrefix)
	}
	if cfg.RegistrationTTL <= 0 {
		cfg.RegistrationTTL = 90 * time.Second
	}
	if cfg.InFlightTimeout <= 0 {
		cfg.InFlightTimeout = 150 * time.Second
	}
	if cfg.EventRetention <= 0 {
		cfg.EventRetention = time.Hour
	}

	var opts []natsgo.Option
	if cfg.Credentials != "" {
		opts = append(opts, natsgo.UserCredentials(cfg.Credentials))
	}
	if cfg.TLS != nil {
		opts = append(opts, natsgo.Secure(cfg.TLS))
	}
	nc, err := natsgo.Connect(cfg.URL, opts...)
	if err != nil {
		return nil, fmt.Errorf("nats message broker: connect %s: %w", cfg.URL, err)
	}
	js, err := jetstream.New(nc)
	if err != nil {
		nc.Close()
		return nil, fmt.Errorf("nats message broker: jetstream: %w", err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), opTimeout)
	defer cancel()
	jobs, err := js.CreateOrUpdateStream(ctx, jetstream.StreamConfig{
		Name:      cfg.SubjectPrefix + "-jobs",
		Subjects:  []string{cfg.SubjectPrefix + ".jobs.>"},
		Retention: jetstream.LimitsPolicy, // settled jobs are removed with DeleteMsg
	})
	if err != nil {
		nc.Close()
		return nil, fmt.Errorf("nats message broker: jobs stream: %w", err)
	}
	events, err := js.CreateOrUpdateStream(ctx, jetstream.StreamConfig{
		Name:      cfg.SubjectPrefix + "-events",
		Subjects:  []string{cfg.SubjectPrefix + ".inst.>"},
		Retention: jetstream.LimitsPolicy,
		MaxAge:    cfg.EventRetention,
	})
	if err != nil {
		nc.Close()
		return nil, fmt.Errorf("nats message broker: events stream: %w", err)
	}
	registry, err := js.CreateOrUpdateKeyValue(ctx, jetstream.KeyValueConfig{
		Bucket:  cfg.SubjectPrefix + "-registry",
		History: 1,
	})
	if err != nil {
		nc.Close()
		return nil, fmt.Errorf("nats message broker: registry bucket: %w", err)
	}
	instMeta, err := js.CreateOrUpdateKeyValue(ctx, jetstream.KeyValueConfig{
		Bucket:  cfg.SubjectPrefix + "-instmeta",
		History: 1,
	})
	if err != nil {
		nc.Close()
		return nil, fmt.Errorf("nats message broker: instance-metadata bucket: %w", err)
	}

	rootCtx, stop := context.WithCancel(context.Background())
	b := &Broker{
		cfg:      cfg,
		nc:       nc,
		js:       js,
		jobs:     jobs,
		events:   events,
		registry: registry,
		instMeta: instMeta,
		rootCtx:  rootCtx,
		stop:     stop,
		inFlight: map[string][]*inFlightJob{},
		timers:   map[string]*time.Timer{},
	}
	go b.sweep()
	return b, nil
}

// Close stops the sweeper, resume timers, and every consumer and watcher, and
// closes the NATS connection. Idempotent.
func (b *Broker) Close() error {
	b.mu.Lock()
	if b.closed {
		b.mu.Unlock()
		return nil
	}
	b.closed = true
	for _, t := range b.timers {
		t.Stop()
	}
	b.timers = map[string]*time.Timer{}
	b.mu.Unlock()
	b.stop()
	b.nc.Close()
	return nil
}

func (b *Broker) isClosed() bool {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.closed
}

// ===== Wire helpers =====

// regEntry is the registry bucket's per-worker value: the worker's stamped
// registrations plus the coordination data the broker needs (TTL deadline,
// RegisteredAt preservation, re-registration generation, tombstones).
type regEntry struct {
	WorkerID string                   `cbor:"workerId"`
	Gen      uint64                   `cbor:"gen"` // bumped on RegisterProcesses, not Heartbeat
	Deadline time.Time                `cbor:"deadline"`
	Regs     []bl.ProcessRegistration `cbor:"regs,omitempty"`
	FirstReg map[string]time.Time     `cbor:"firstReg,omitempty"` // RegisteredAt per process key
	Tomb     string                   `cbor:"tomb,omitempty"`     // "removed" | "heartbeat-lost"
}

// instMetaRec is the instance-metadata bucket's per-instance value: the
// routing key and correlation key Submit recorded, plus the finish marker
// that starts the retention clock.
type instMetaRec struct {
	Key            bl.ProcessKey `cbor:"key"`
	CorrelationKey *string       `cbor:"correlationKey,omitempty"`
	Finished       bool          `cbor:"finished,omitempty"`
	FinishedAt     time.Time     `cbor:"finishedAt,omitempty"`
}

var jobKindNames = map[bl.JobKind]string{
	bl.JobStart:          bl.KindJobStart,
	bl.JobRespondToInput: bl.KindJobRespondToInput,
	bl.JobCancel:         bl.KindJobCancel,
	bl.JobTerminate:      bl.KindJobTerminate,
	bl.JobResume:         bl.KindJobResume,
}

func (b *Broker) decodeBody(data []byte, into any) error {
	env, err := bl.DecodeEnvelope(data, b.cfg.Cipher)
	if err != nil {
		return err
	}
	return env.DecodePayload(into)
}

// ===== Subject mapping =====

// encToken encodes one process-key part as a single NATS subject token —
// namespaces contain '/' and '.', which are illegal in tokens.
func encToken(s string) string {
	return base64.RawURLEncoding.EncodeToString([]byte(s))
}

func decToken(s string) string {
	raw, err := base64.RawURLEncoding.DecodeString(s)
	if err != nil {
		return s
	}
	return string(raw)
}

func (b *Broker) jobSubject(key bl.ProcessKey, instanceID string) string {
	return b.cfg.SubjectPrefix + ".jobs." + encToken(key.Namespace) + "." + encToken(key.ProcessID) + "." + encToken(key.Version) + "." + instanceID
}

func (b *Broker) jobFilter(key bl.ProcessKey) string {
	return b.cfg.SubjectPrefix + ".jobs." + encToken(key.Namespace) + "." + encToken(key.ProcessID) + "." + encToken(key.Version) + ".*"
}

func (b *Broker) instSubject(instanceID, suffix string) string {
	return b.cfg.SubjectPrefix + ".inst." + instanceID + "." + suffix
}

func (b *Broker) instFilter(instanceID string) string {
	return b.cfg.SubjectPrefix + ".inst." + instanceID + ".>"
}

func validPrefix(s string) bool {
	for _, r := range s {
		switch {
		case r >= 'a' && r <= 'z', r >= 'A' && r <= 'Z', r >= '0' && r <= '9', r == '-', r == '_':
		default:
			return false
		}
	}
	return s != ""
}

// validSubjectToken reports whether s is usable as a single subject token.
// Broker-generated instance ids (UUIDs) always are.
func validSubjectToken(s string) bool {
	if s == "" {
		return false
	}
	return !strings.ContainsAny(s, ". *>\t\r\n")
}

func keyString(k bl.ProcessKey) string {
	return k.Namespace + "\x1f" + k.ProcessID + "\x1f" + k.Version
}

// subjectMatches reports whether a concrete subject matches a filter pattern
// ('*' one token, '>' the rest).
func subjectMatches(pattern, subject string) bool {
	pt := strings.Split(pattern, ".")
	st := strings.Split(subject, ".")
	for i, p := range pt {
		if p == ">" {
			return true
		}
		if i >= len(st) {
			return false
		}
		if p != "*" && p != st[i] {
			return false
		}
	}
	return len(pt) == len(st)
}

// ===== Registry storage =====

func regKey(workerID string) string { return encToken(workerID) }

func (b *Broker) getRegEntry(ctx context.Context, workerID string) (*regEntry, error) {
	entry, err := b.registry.Get(ctx, regKey(workerID))
	if err != nil {
		if errors.Is(err, jetstream.ErrKeyNotFound) {
			return nil, nil
		}
		return nil, err
	}
	var rec regEntry
	if err := b.decodeBody(entry.Value(), &rec); err != nil {
		return nil, err
	}
	return &rec, nil
}

func (b *Broker) putRegEntry(ctx context.Context, workerID string, rec *regEntry) error {
	data, err := bl.EncodeEnvelope(bl.KindRegistration, "", nil, rec, b.cfg.Cipher)
	if err != nil {
		return err
	}
	_, err = b.registry.Put(ctx, regKey(workerID), data)
	return err
}

// liveRegistration reads through the registry bucket and returns one live
// (non-tombstoned, non-lapsed) registration for the key, from any worker.
// The bucket is authoritative and immediately consistent, so Submit needs no
// cold-start snapshot wait.
func (b *Broker) liveRegistration(ctx context.Context, key bl.ProcessKey) (*bl.ProcessRegistration, error) {
	keys, err := b.registry.Keys(ctx)
	if err != nil {
		if errors.Is(err, jetstream.ErrNoKeysFound) {
			return nil, nil
		}
		return nil, err
	}
	now := time.Now()
	for _, k := range keys {
		entry, err := b.registry.Get(ctx, k)
		if err != nil {
			if errors.Is(err, jetstream.ErrKeyNotFound) {
				continue // swept between list and get
			}
			return nil, err
		}
		var rec regEntry
		if err := b.decodeBody(entry.Value(), &rec); err != nil {
			continue // foreign or unreadable entry; not ours to interpret
		}
		if rec.Tomb != "" || now.After(rec.Deadline) {
			continue
		}
		for i := range rec.Regs {
			if rec.Regs[i].Key() == key {
				r := rec.Regs[i]
				return &r, nil
			}
		}
	}
	return nil, nil
}

// ===== Instance metadata =====

func (b *Broker) getInstMeta(ctx context.Context, instanceID string) (*instMetaRec, error) {
	entry, err := b.instMeta.Get(ctx, instanceID)
	if err != nil {
		if errors.Is(err, jetstream.ErrKeyNotFound) || errors.Is(err, jetstream.ErrInvalidKey) {
			return nil, nil
		}
		return nil, err
	}
	var rec instMetaRec
	if err := b.decodeBody(entry.Value(), &rec); err != nil {
		return nil, err
	}
	return &rec, nil
}

func (b *Broker) putInstMeta(ctx context.Context, instanceID string, rec *instMetaRec) error {
	data, err := bl.EncodeEnvelope(kindInstanceMeta, instanceID, rec.CorrelationKey, rec, b.cfg.Cipher)
	if err != nil {
		return err
	}
	_, err = b.instMeta.Put(ctx, instanceID, data)
	return err
}

// ===== Sweeper =====

// sweep expires worker registrations whose TTL lapsed (tombstone + delete, so
// registry watchers can translate the loss into HeartbeatLost updates with
// the old registrations attached) and purges finished instance metadata past
// the event-retention window.
func (b *Broker) sweep() {
	tick := min(b.cfg.RegistrationTTL, b.cfg.EventRetention) / 4
	tick = max(tick, 5*time.Millisecond)
	tick = min(tick, 5*time.Second)
	ticker := time.NewTicker(tick)
	defer ticker.Stop()
	for {
		select {
		case <-b.rootCtx.Done():
			return
		case <-ticker.C:
			b.sweepOnce()
		}
	}
}

func (b *Broker) sweepOnce() {
	ctx, cancel := context.WithTimeout(b.rootCtx, opTimeout)
	defer cancel()
	now := time.Now()
	if keys, err := b.registry.Keys(ctx); err == nil {
		for _, k := range keys {
			entry, err := b.registry.Get(ctx, k)
			if err != nil {
				continue
			}
			var rec regEntry
			if err := b.decodeBody(entry.Value(), &rec); err != nil {
				continue
			}
			switch {
			case rec.Tomb != "": // stale tombstone whose delete never landed
				_ = b.registry.Delete(ctx, k)
			case now.After(rec.Deadline):
				rec.Tomb = tombstoneHeartbeatLost
				rec.Gen++
				if err := b.putRegEntry(ctx, decToken(k), &rec); err == nil {
					_ = b.registry.Delete(ctx, k)
				}
			}
		}
	}
	if keys, err := b.instMeta.Keys(ctx); err == nil {
		for _, k := range keys {
			entry, err := b.instMeta.Get(ctx, k)
			if err != nil {
				continue
			}
			var rec instMetaRec
			if err := b.decodeBody(entry.Value(), &rec); err != nil {
				continue
			}
			if rec.Finished && now.After(rec.FinishedAt.Add(b.cfg.EventRetention)) {
				_ = b.instMeta.Purge(ctx, k)
			}
		}
	}
}

// ===== Producer side =====

// Submit validates the request against the registry bucket, records the
// instance's routing metadata, publishes the JobStart, and publishes the
// Pending lifecycle event.
func (b *Broker) Submit(ctx context.Context, req bl.StartRequest) (string, error) {
	if err := ctx.Err(); err != nil {
		return "", err
	}
	if b.isClosed() {
		return "", bl.ErrBrokerClosed
	}
	reg, err := b.liveRegistration(ctx, req.Key())
	if err != nil {
		return "", err
	}
	if reg == nil {
		return "", bl.ErrUnknownProcess
	}
	var start *bl.StartEventInfo
	for i := range reg.StartEvents {
		if reg.StartEvents[i].Id == req.StartID {
			start = &reg.StartEvents[i]
			break
		}
	}
	if start == nil {
		return "", bl.ErrUnknownStartID
	}
	if start.InputContract != nil {
		if err := start.InputContract.Validate(req.Input); err != nil {
			return "", err
		}
	}

	instanceID := bl.NewProcessInstanceID()
	if err := b.putInstMeta(ctx, instanceID, &instMetaRec{
		Key:            req.Key(),
		CorrelationKey: req.CorrelationKey,
	}); err != nil {
		return "", err
	}
	if err := b.publishJob(ctx, bl.Job{
		Kind: bl.JobStart,
		Key:  req.Key(),
		Start: &bl.StartJob{
			InstanceID:     instanceID,
			StartID:        req.StartID,
			Input:          req.Input,
			CorrelationKey: req.CorrelationKey,
		},
	}); err != nil {
		return "", err
	}
	if err := b.publishEvent(ctx, instanceID, subjLifecycle, bl.InstanceEvent{
		Kind:      bl.InstanceEventLifecycle,
		Lifecycle: &bl.LifecycleChange{Phase: bl.ProcessStatusPending},
	}); err != nil {
		return "", err
	}
	return instanceID, nil
}

// Cancel removes a still-queued JobStart from the jobs stream (publishing the
// terminal Cancelled event itself; no opt-in needed), or publishes a
// JobCancel when the job was already delivered — which requires
// AllowExternalCancel.
func (b *Broker) Cancel(ctx context.Context, req bl.CancelRequest) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if b.isClosed() {
		return bl.ErrBrokerClosed
	}
	reg, err := b.liveRegistration(ctx, req.Key())
	if err != nil {
		return err
	}
	if reg == nil {
		return bl.ErrUnknownProcess
	}
	subject := b.jobSubject(req.Key(), req.InstanceID)
	if raw, err := b.jobs.GetLastMsgForSubject(ctx, subject); err == nil {
		if env, derr := bl.DecodeEnvelope(raw.Data, b.cfg.Cipher); derr == nil && env.Kind == bl.KindJobStart &&
			!b.jobDelivered(ctx, subject, raw.Sequence, req.InstanceID) &&
			b.jobs.DeleteMsg(ctx, raw.Sequence) == nil {
			return b.finish(ctx, req.InstanceID, bl.ProcessStatusCancelled, nil)
		}
	}
	if !reg.AllowExternalCancel {
		return bl.ErrCancelNotAllowed
	}
	return b.publishJob(ctx, bl.Job{
		Kind:   bl.JobCancel,
		Key:    req.Key(),
		Cancel: &bl.CancelJob{InstanceID: req.InstanceID, Reason: req.Reason},
	})
}

// jobDelivered reports (best-effort) whether the queued message was already
// delivered to some worker: locally tracked in-flight, or some consumer whose
// filter covers the subject has a delivered floor at or past the sequence.
func (b *Broker) jobDelivered(ctx context.Context, subject string, seq uint64, instanceID string) bool {
	b.mu.Lock()
	held := len(b.inFlight[instanceID]) > 0
	b.mu.Unlock()
	if held {
		return true
	}
	lister := b.jobs.ListConsumers(ctx)
	for info := range lister.Info() {
		filters := info.Config.FilterSubjects
		if len(filters) == 0 && info.Config.FilterSubject != "" {
			filters = []string{info.Config.FilterSubject}
		}
		for _, f := range filters {
			if subjectMatches(f, subject) && info.Delivered.Stream >= seq {
				return true
			}
		}
	}
	return false
}

// Terminate publishes a JobTerminate. Requires AllowExternalTerminate.
func (b *Broker) Terminate(ctx context.Context, req bl.TerminateRequest) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if b.isClosed() {
		return bl.ErrBrokerClosed
	}
	reg, err := b.liveRegistration(ctx, req.Key())
	if err != nil {
		return err
	}
	if reg == nil {
		return bl.ErrUnknownProcess
	}
	if !reg.AllowExternalTerminate {
		return bl.ErrTerminateNotAllowed
	}
	return b.publishJob(ctx, bl.Job{
		Kind:      bl.JobTerminate,
		Key:       req.Key(),
		Terminate: &bl.TerminateJob{InstanceID: req.InstanceID, Reason: req.Reason},
	})
}

// RespondToInputRequest routes the response to the instance's process key. An
// instance the broker no longer holds surfaces asynchronously on the
// instance's topic, per the error model.
func (b *Broker) RespondToInputRequest(ctx context.Context, instanceID, requestID string, payload map[string]any) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if b.isClosed() {
		return bl.ErrBrokerClosed
	}
	meta, err := b.getInstMeta(ctx, instanceID)
	if err != nil {
		return err
	}
	if meta == nil || meta.Finished {
		code := bl.ErrCodeInstanceNotFound
		if meta != nil {
			code = bl.ErrCodeAlreadyFinished
		}
		return b.publishEvent(ctx, instanceID, subjErr, bl.InstanceEvent{
			Kind:  bl.InstanceEventError,
			Error: &bl.InstanceError{Code: code, Message: "respond-to-input for an instance the broker does not hold"},
		})
	}
	return b.publishJob(ctx, bl.Job{
		Kind:           bl.JobRespondToInput,
		Key:            meta.Key,
		RespondToInput: &bl.RespondToInputJob{InstanceID: instanceID, RequestID: requestID, Payload: payload},
	})
}

// ===== Producer side: subscriptions =====

// SubscribeToInstance replays the instance's latest lifecycle event (and the
// terminal Result/Error event, if finished within retention) from the events
// stream, then follows live from the next stream sequence with an ordered
// consumer. An instance with no visible events gets INSTANCE_NOT_FOUND as its
// first event.
func (b *Broker) SubscribeToInstance(ctx context.Context, instanceID string) (<-chan bl.InstanceEvent, error) {
	if b.isClosed() {
		return nil, bl.ErrBrokerClosed
	}
	sub := newInstSub(eventBufferSize)
	if !validSubjectToken(instanceID) {
		// No such instance can ever have events on the stream.
		sub.send(notFoundEvent(instanceID))
		sub.close()
		return sub.ch, nil
	}

	opCtx, opCancel := context.WithTimeout(ctx, opTimeout)
	defer opCancel()
	var startSeq uint64
	awaitTerminalExtra := false
	lastLC, lcSeq, err := b.lastEvent(opCtx, instanceID, subjLifecycle)
	if err != nil {
		return nil, err
	}
	if lastLC == nil {
		meta, err := b.getInstMeta(opCtx, instanceID)
		if err != nil {
			return nil, err
		}
		if meta == nil {
			sub.send(notFoundEvent(instanceID))
		}
		// Follow live from the beginning of the instance's subjects.
	} else {
		sub.send(*lastLC)
		startSeq = lcSeq + 1
		if lastLC.Lifecycle != nil && lastLC.Lifecycle.Phase.Finished() {
			lastTerm, termSeq, err := b.lastEvent(opCtx, instanceID, subjTerminal)
			if err != nil {
				return nil, err
			}
			if lastTerm != nil && termSeq > lcSeq {
				sub.send(*lastTerm)
				sub.close()
				return sub.ch, nil
			}
			if lastLC.Lifecycle.Phase == bl.ProcessStatusCancelled {
				// Cancelled has no Result/Error extra: finished.
				sub.close()
				return sub.ch, nil
			}
			// Completed/Failed but the extra event is not visible yet:
			// follow live until it arrives.
			awaitTerminalExtra = true
		}
	}

	occfg := jetstream.OrderedConsumerConfig{
		FilterSubjects: []string{b.instFilter(instanceID)},
	}
	if startSeq > 0 {
		occfg.DeliverPolicy = jetstream.DeliverByStartSequencePolicy
		occfg.OptStartSeq = startSeq
	}
	cons, err := b.js.OrderedConsumer(opCtx, b.cfg.SubjectPrefix+"-events", occfg)
	if err != nil {
		return nil, err
	}
	subCtx, subCancel := context.WithCancel(ctx)
	await := awaitTerminalExtra
	cc, err := cons.Consume(func(msg jetstream.Msg) {
		var evt bl.InstanceEvent
		if b.decodeBody(msg.Data(), &evt) != nil {
			return
		}
		sub.send(evt)
		if evt.Kind == bl.InstanceEventLifecycle && evt.Lifecycle != nil && evt.Lifecycle.Phase.Finished() {
			if evt.Lifecycle.Phase == bl.ProcessStatusCancelled {
				subCancel() // no extra event follows
				return
			}
			await = true // deliver the Result/Error extra before closing
			return
		}
		if await && strings.HasSuffix(msg.Subject(), "."+subjTerminal) {
			subCancel()
		}
	})
	if err != nil {
		subCancel()
		return nil, err
	}
	go func() {
		select {
		case <-subCtx.Done():
		case <-b.rootCtx.Done():
		}
		cc.Stop()
		sub.close()
		subCancel()
	}()
	return sub.ch, nil
}

// lastEvent fetches and decodes the newest event on one of the instance's
// subjects; (nil, 0, nil) when the subject has no messages.
func (b *Broker) lastEvent(ctx context.Context, instanceID, suffix string) (*bl.InstanceEvent, uint64, error) {
	raw, err := b.events.GetLastMsgForSubject(ctx, b.instSubject(instanceID, suffix))
	if err != nil {
		if errors.Is(err, jetstream.ErrMsgNotFound) {
			return nil, 0, nil
		}
		return nil, 0, err
	}
	var evt bl.InstanceEvent
	if err := b.decodeBody(raw.Data, &evt); err != nil {
		return nil, 0, err
	}
	return &evt, raw.Sequence, nil
}

func notFoundEvent(instanceID string) bl.InstanceEvent {
	return bl.InstanceEvent{
		InstanceID: instanceID,
		Kind:       bl.InstanceEventError,
		OccurredAt: time.Now(),
		Error:      &bl.InstanceError{Code: bl.ErrCodeInstanceNotFound, Message: "no events visible for this instance"},
	}
}

// SubscribeToProcessRegistry watches the registry bucket: the watcher's
// initial values become the snapshot (then the SnapshotComplete sentinel),
// and subsequent puts/tombstones/deletes become Added / Removed /
// HeartbeatLost updates. Closes on ctx cancellation (or broker Close).
func (b *Broker) SubscribeToProcessRegistry(ctx context.Context) (<-chan bl.RegistryUpdate, error) {
	if b.isClosed() {
		return nil, bl.ErrBrokerClosed
	}
	watchCtx, watchCancel := context.WithCancel(ctx)
	watcher, err := b.registry.WatchAll(watchCtx)
	if err != nil {
		watchCancel()
		return nil, err
	}
	pump := newUpdatePump()
	go func() {
		select {
		case <-watchCtx.Done():
		case <-b.rootCtx.Done():
		}
		watchCancel()
		pump.stop()
	}()
	go func() {
		defer pump.stop()
		defer watchCancel()
		type cached struct {
			gen  uint64
			regs []bl.ProcessRegistration
		}
		cache := map[string]cached{}
		emit := func(kind bl.RegistryUpdateKind, regs []bl.ProcessRegistration) {
			for i := range regs {
				r := regs[i]
				pump.push(bl.RegistryUpdate{Kind: kind, Registration: &r})
			}
		}
		snapshot := true
		for entry := range watcher.Updates() {
			if entry == nil { // end of initial values
				if snapshot {
					snapshot = false
					pump.push(bl.RegistryUpdate{Kind: bl.RegistryUpdateSnapshotComplete})
				}
				continue
			}
			switch entry.Operation() {
			case jetstream.KeyValuePut:
				var rec regEntry
				if b.decodeBody(entry.Value(), &rec) != nil {
					continue
				}
				if rec.Tomb != "" {
					prev, ok := cache[rec.WorkerID]
					if !ok {
						continue // never advertised to this watcher
					}
					delete(cache, rec.WorkerID)
					if snapshot {
						continue
					}
					kind := bl.RegistryUpdateRemoved
					if rec.Tomb == tombstoneHeartbeatLost {
						kind = bl.RegistryUpdateHeartbeatLost
					}
					regs := rec.Regs
					if len(regs) == 0 {
						regs = prev.regs
					}
					emit(kind, regs)
					continue
				}
				if snapshot {
					if time.Now().After(rec.Deadline) {
						continue // lapsed; the sweeper will tombstone it
					}
					cache[rec.WorkerID] = cached{rec.Gen, rec.Regs}
					emit(bl.RegistryUpdateSnapshot, rec.Regs)
					continue
				}
				prev, ok := cache[rec.WorkerID]
				if ok && prev.gen == rec.Gen {
					// Heartbeat refresh: same registration set, no updates.
					cache[rec.WorkerID] = cached{rec.Gen, rec.Regs}
					continue
				}
				if ok {
					emit(bl.RegistryUpdateRemoved, prev.regs)
				}
				cache[rec.WorkerID] = cached{rec.Gen, rec.Regs}
				emit(bl.RegistryUpdateAdded, rec.Regs)
			case jetstream.KeyValueDelete, jetstream.KeyValuePurge:
				// Normal removals are announced by the preceding tombstone
				// put; a bare delete (external interference) still clears.
				workerID := decToken(entry.Key())
				if prev, ok := cache[workerID]; ok {
					delete(cache, workerID)
					if !snapshot {
						emit(bl.RegistryUpdateRemoved, prev.regs)
					}
				}
			}
		}
	}()
	return pump.out, nil
}

// ===== Worker side: registration =====

// RegisterProcesses replaces the worker's registration set and stamps the
// registry metadata (WorkerID, RegisteredAt preserved per key across
// re-registration, LastHeartbeat).
func (b *Broker) RegisterProcesses(ctx context.Context, workerID string, regs []bl.ProcessRegistration) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if b.isClosed() {
		return bl.ErrBrokerClosed
	}
	prev, err := b.getRegEntry(ctx, workerID)
	if err != nil {
		return err
	}
	now := time.Now()
	entry := regEntry{
		WorkerID: workerID,
		Gen:      1,
		Deadline: now.Add(b.cfg.RegistrationTTL),
		FirstReg: map[string]time.Time{},
	}
	if prev != nil && prev.Tomb == "" {
		entry.Gen = prev.Gen + 1
		if prev.FirstReg != nil {
			entry.FirstReg = prev.FirstReg
		}
	}
	stamped := make([]bl.ProcessRegistration, len(regs))
	for i, r := range regs {
		r.WorkerID = workerID
		first, ok := entry.FirstReg[keyString(r.Key())]
		if !ok {
			first = now
			entry.FirstReg[keyString(r.Key())] = first
		}
		r.RegisteredAt = first
		r.LastHeartbeat = now
		stamped[i] = r
	}
	entry.Regs = stamped
	return b.putRegEntry(ctx, workerID, &entry)
}

// Heartbeat refreshes the worker's registration TTL.
func (b *Broker) Heartbeat(ctx context.Context, workerID string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if b.isClosed() {
		return bl.ErrBrokerClosed
	}
	entry, err := b.getRegEntry(ctx, workerID)
	if err != nil {
		return err
	}
	if entry == nil || entry.Tomb != "" {
		return bl.ErrUnknownWorker
	}
	now := time.Now()
	entry.Deadline = now.Add(b.cfg.RegistrationTTL)
	for i := range entry.Regs {
		entry.Regs[i].LastHeartbeat = now
	}
	return b.putRegEntry(ctx, workerID, entry) // same Gen: watchers stay quiet
}

// Unregister removes the worker's registrations immediately: a "removed"
// tombstone put (so watchers see why, with the old registrations attached)
// followed by the key's deletion.
func (b *Broker) Unregister(ctx context.Context, workerID string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if b.isClosed() {
		return bl.ErrBrokerClosed
	}
	entry, err := b.getRegEntry(ctx, workerID)
	if err != nil {
		return err
	}
	if entry == nil || entry.Tomb != "" {
		return bl.ErrUnknownWorker
	}
	entry.Tomb = tombstoneRemoved
	entry.Gen++
	if err := b.putRegEntry(ctx, workerID, entry); err != nil {
		return err
	}
	return b.registry.Delete(ctx, regKey(workerID))
}

// ===== Worker side: job queue =====

type inFlightJob struct {
	msg jetstream.Msg
	seq uint64 // jobs-stream sequence, for DeleteMsg on settle
}

// FetchJobs consumes the jobs stream through a durable pull consumer filtered
// to exactly the requested keys, with AckWait = InFlightTimeout. The durable
// name is derived from the key set, so workers fetching the same keys compete
// on one consumer and a dead worker's unacked jobs are redelivered to the
// rest. Delivered jobs are tracked in-flight per instance until a Report*
// verb settles them (ack + removal from the stream).
func (b *Broker) FetchJobs(ctx context.Context, keys []bl.ProcessKey) (<-chan bl.Job, error) {
	if b.isClosed() {
		return nil, bl.ErrBrokerClosed
	}
	out := make(chan bl.Job)
	if len(keys) == 0 {
		go func() {
			defer close(out)
			select {
			case <-ctx.Done():
			case <-b.rootCtx.Done():
			}
		}()
		return out, nil
	}
	filters := make([]string, len(keys))
	for i, k := range keys {
		filters[i] = b.jobFilter(k)
	}
	sort.Strings(filters)
	sum := sha256.Sum256([]byte(strings.Join(filters, "\x00")))
	durable := "fj-" + hex.EncodeToString(sum[:12])

	opCtx, opCancel := context.WithTimeout(ctx, opTimeout)
	defer opCancel()
	cons, err := b.js.CreateOrUpdateConsumer(opCtx, b.cfg.SubjectPrefix+"-jobs", jetstream.ConsumerConfig{
		Durable:        durable,
		FilterSubjects: filters,
		AckPolicy:      jetstream.AckExplicitPolicy,
		AckWait:        b.cfg.InFlightTimeout,
		MaxDeliver:     -1,
	})
	if err != nil {
		return nil, err
	}
	iter, err := cons.Messages(jetstream.PullMaxMessages(1))
	if err != nil {
		return nil, err
	}
	go func() {
		select {
		case <-ctx.Done():
		case <-b.rootCtx.Done():
		}
		iter.Stop()
	}()
	go func() {
		defer close(out)
		for {
			msg, err := iter.Next()
			if err != nil {
				return
			}
			var job bl.Job
			if b.decodeBody(msg.Data(), &job) != nil {
				_ = msg.Term() // poison message; never redeliver
				continue
			}
			meta, err := msg.Metadata()
			if err != nil {
				_ = msg.Nak()
				continue
			}
			fl := &inFlightJob{msg: msg, seq: meta.Sequence.Stream}
			instanceID := job.InstanceID()
			b.mu.Lock()
			b.inFlight[instanceID] = append(b.inFlight[instanceID], fl)
			b.mu.Unlock()
			select {
			case out <- job:
			case <-ctx.Done():
				b.untrack(instanceID, fl)
				_ = msg.Nak()
				return
			case <-b.rootCtx.Done():
				return
			}
		}
	}()
	return out, nil
}

func (b *Broker) untrack(instanceID string, fl *inFlightJob) {
	b.mu.Lock()
	defer b.mu.Unlock()
	jobs := b.inFlight[instanceID]
	for i, x := range jobs {
		if x == fl {
			b.inFlight[instanceID] = append(jobs[:i], jobs[i+1:]...)
			break
		}
	}
	if len(b.inFlight[instanceID]) == 0 {
		delete(b.inFlight, instanceID)
	}
}

// settleInstance settles every in-flight job for the instance: ack the
// delivery and remove the message from the jobs stream, so no consumer —
// current or future — sees it again.
func (b *Broker) settleInstance(ctx context.Context, instanceID string) {
	b.mu.Lock()
	jobs := b.inFlight[instanceID]
	delete(b.inFlight, instanceID)
	b.mu.Unlock()
	for _, fl := range jobs {
		_ = fl.msg.Ack()
		_ = b.jobs.DeleteMsg(ctx, fl.seq) // best-effort; ack already settled delivery
	}
}

func (b *Broker) publishJob(ctx context.Context, job bl.Job) error {
	data, err := bl.EncodeEnvelope(jobKindNames[job.Kind], job.InstanceID(), nil, job, b.cfg.Cipher)
	if err != nil {
		return err
	}
	_, err = b.js.Publish(ctx, b.jobSubject(job.Key, job.InstanceID()), data)
	return err
}

// ===== Worker side: lifecycle reports and instance topic =====

// publishEvent stamps and publishes one instance event on the instance's
// topic, mirroring the correlation key recorded at Submit.
func (b *Broker) publishEvent(ctx context.Context, instanceID, suffix string, evt bl.InstanceEvent) error {
	evt.InstanceID = instanceID
	evt.OccurredAt = time.Now()
	if evt.CorrelationKey == nil {
		if meta, err := b.getInstMeta(ctx, instanceID); err == nil && meta != nil {
			evt.CorrelationKey = meta.CorrelationKey
		}
	}
	data, err := bl.EncodeEnvelope(bl.KindInstanceEvent, instanceID, evt.CorrelationKey, evt, b.cfg.Cipher)
	if err != nil {
		return err
	}
	_, err = b.js.Publish(ctx, b.instSubject(instanceID, suffix), data)
	return err
}

// ReportRunning publishes the Running lifecycle event. Does not settle the
// in-flight job.
func (b *Broker) ReportRunning(ctx context.Context, instanceID string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if b.isClosed() {
		return bl.ErrBrokerClosed
	}
	return b.publishEvent(ctx, instanceID, subjLifecycle, bl.InstanceEvent{
		Kind:      bl.InstanceEventLifecycle,
		Lifecycle: &bl.LifecycleChange{Phase: bl.ProcessStatusRunning},
	})
}

// ReportSuspended publishes the Suspended lifecycle event, settles the
// in-flight job, and — given a resumeAt — schedules the JobResume. The timer
// is in-process: a broker restart before the fire-time loses it (durable
// timers are pending; see the spec's delayed-delivery note).
func (b *Broker) ReportSuspended(ctx context.Context, instanceID string, resumeAt *time.Time) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if b.isClosed() {
		return bl.ErrBrokerClosed
	}
	if err := b.publishEvent(ctx, instanceID, subjLifecycle, bl.InstanceEvent{
		Kind:      bl.InstanceEventLifecycle,
		Lifecycle: &bl.LifecycleChange{Phase: bl.ProcessStatusSuspended},
	}); err != nil {
		return err
	}
	b.settleInstance(ctx, instanceID)
	if resumeAt == nil {
		return nil // external-input wait: the response arrives as its own job
	}
	meta, err := b.getInstMeta(ctx, instanceID)
	if err != nil {
		return err
	}
	if meta == nil {
		return nil // nowhere to route a resume for an instance the broker does not hold
	}
	key := meta.Key
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.closed {
		return bl.ErrBrokerClosed
	}
	if t := b.timers[instanceID]; t != nil {
		t.Stop()
	}
	b.timers[instanceID] = time.AfterFunc(time.Until(*resumeAt), func() {
		b.mu.Lock()
		delete(b.timers, instanceID)
		closed := b.closed
		b.mu.Unlock()
		if closed {
			return
		}
		tctx, cancel := context.WithTimeout(b.rootCtx, opTimeout)
		defer cancel()
		if meta, err := b.getInstMeta(tctx, instanceID); err != nil || meta == nil || meta.Finished {
			return
		}
		_ = b.publishJob(tctx, bl.Job{
			Kind:   bl.JobResume,
			Key:    key,
			Resume: &bl.ResumeJob{InstanceID: instanceID},
		})
	})
	return nil
}

// ReportCompleted publishes the Completed lifecycle event and the Result
// event, settles the job, and starts the retention clock.
func (b *Broker) ReportCompleted(ctx context.Context, instanceID string, result bl.EvaluationResult) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if b.isClosed() {
		return bl.ErrBrokerClosed
	}
	return b.finish(ctx, instanceID, bl.ProcessStatusCompleted, &bl.InstanceEvent{
		Kind:   bl.InstanceEventResult,
		Result: &result,
	})
}

// ReportFailed publishes the Failed lifecycle event and the Error event,
// settles the job, and starts the retention clock.
func (b *Broker) ReportFailed(ctx context.Context, instanceID string, instErr bl.InstanceError) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if b.isClosed() {
		return bl.ErrBrokerClosed
	}
	return b.finish(ctx, instanceID, bl.ProcessStatusFailed, &bl.InstanceEvent{
		Kind:  bl.InstanceEventError,
		Error: &instErr,
	})
}

// ReportCancelled publishes the Cancelled lifecycle event, settles the job,
// and starts the retention clock.
func (b *Broker) ReportCancelled(ctx context.Context, instanceID string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if b.isClosed() {
		return bl.ErrBrokerClosed
	}
	return b.finish(ctx, instanceID, bl.ProcessStatusCancelled, nil)
}

// finish drives the instance to a terminal phase: settle in-flight jobs, stop
// any resume timer, publish the terminal lifecycle event, then the extra
// Result/Error event, then mark the instance finished (subscribers close on
// the events themselves; retention runs from now). Idempotent per instance.
func (b *Broker) finish(ctx context.Context, instanceID string, phase bl.ProcessStatus, extra *bl.InstanceEvent) error {
	b.settleInstance(ctx, instanceID)
	b.mu.Lock()
	if t := b.timers[instanceID]; t != nil {
		t.Stop()
		delete(b.timers, instanceID)
	}
	b.mu.Unlock()
	meta, err := b.getInstMeta(ctx, instanceID)
	if err != nil {
		return err
	}
	if meta != nil && meta.Finished {
		return nil
	}
	if err := b.publishEvent(ctx, instanceID, subjLifecycle, bl.InstanceEvent{
		Kind:      bl.InstanceEventLifecycle,
		Lifecycle: &bl.LifecycleChange{Phase: phase},
	}); err != nil {
		return err
	}
	if extra != nil {
		if err := b.publishEvent(ctx, instanceID, subjTerminal, *extra); err != nil {
			return err
		}
	}
	if meta == nil {
		meta = &instMetaRec{}
	}
	meta.Finished = true
	meta.FinishedAt = time.Now()
	return b.putInstMeta(ctx, instanceID, meta)
}

// PostError publishes a non-terminal error event.
func (b *Broker) PostError(ctx context.Context, instanceID string, instErr bl.InstanceError) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if b.isClosed() {
		return bl.ErrBrokerClosed
	}
	return b.publishEvent(ctx, instanceID, subjErr, bl.InstanceEvent{
		Kind:  bl.InstanceEventError,
		Error: &instErr,
	})
}

// PostInputRequest publishes a RequestInputTask's request for input.
func (b *Broker) PostInputRequest(ctx context.Context, instanceID string, req bl.InputRequest) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if b.isClosed() {
		return bl.ErrBrokerClosed
	}
	return b.publishEvent(ctx, instanceID, subjInputReq, bl.InstanceEvent{
		Kind:         bl.InstanceEventInputRequest,
		InputRequest: &req,
	})
}

// PostNodeCompleted publishes a node-completion event.
func (b *Broker) PostNodeCompleted(ctx context.Context, instanceID string, nc bl.NodeCompleted) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if b.isClosed() {
		return bl.ErrBrokerClosed
	}
	return b.publishEvent(ctx, instanceID, subjNode, bl.InstanceEvent{
		Kind:          bl.InstanceEventNodeCompleted,
		NodeCompleted: &nc,
	})
}

// ===== Subscriber plumbing =====

// instSub is one SubscribeToInstance channel. A full buffer drops events
// rather than blocking the consumer callback; recovery is signalled with a
// synthetic BACKPRESSURE_DROP event.
type instSub struct {
	mu      sync.Mutex
	ch      chan bl.InstanceEvent
	dropped bool
	closed  bool
}

func newInstSub(buf int) *instSub {
	return &instSub{ch: make(chan bl.InstanceEvent, max(buf, 2))}
}

func (s *instSub) send(evt bl.InstanceEvent) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		return
	}
	if s.dropped {
		synthetic := bl.InstanceEvent{
			InstanceID: evt.InstanceID,
			Kind:       bl.InstanceEventError,
			OccurredAt: time.Now(),
			Error:      &bl.InstanceError{Code: bl.ErrCodeBackpressureDrop, Message: "subscriber buffer overflowed; events were dropped"},
		}
		select {
		case s.ch <- synthetic:
			s.dropped = false
		default:
			return // still full; this event drops too
		}
	}
	select {
	case s.ch <- evt:
	default:
		s.dropped = true
	}
}

func (s *instSub) close() {
	s.mu.Lock()
	defer s.mu.Unlock()
	if !s.closed {
		s.closed = true
		close(s.ch)
	}
}

// updatePump is a lossless per-watcher delivery queue for registry updates:
// they must not drop, so each watcher gets an unbounded buffer drained by its
// own goroutine.
type updatePump struct {
	mu     sync.Mutex
	queue  []bl.RegistryUpdate
	signal chan struct{}
	done   chan struct{}
	once   sync.Once
	out    chan bl.RegistryUpdate
}

func newUpdatePump() *updatePump {
	p := &updatePump{
		signal: make(chan struct{}, 1),
		done:   make(chan struct{}),
		out:    make(chan bl.RegistryUpdate),
	}
	go p.run()
	return p
}

func (p *updatePump) push(u bl.RegistryUpdate) {
	p.mu.Lock()
	p.queue = append(p.queue, u)
	p.mu.Unlock()
	select {
	case p.signal <- struct{}{}:
	default:
	}
}

func (p *updatePump) stop() {
	p.once.Do(func() { close(p.done) })
}

func (p *updatePump) run() {
	defer close(p.out)
	for {
		p.mu.Lock()
		if len(p.queue) == 0 {
			p.mu.Unlock()
			select {
			case <-p.signal:
				continue
			case <-p.done:
				return
			}
		}
		u := p.queue[0]
		p.queue = p.queue[1:]
		p.mu.Unlock()
		select {
		case p.out <- u:
		case <-p.done:
			return
		}
	}
}
