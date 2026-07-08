// Package redis implements bl.MessageBroker against Redis or Valkey
// (specs/message-brokers/redis-message-broker.spec.md). Only commands present
// in both servers are used.
//
// Mapping to primitives:
//   - Jobs: one Stream per ProcessKey (<prefix>:jobs:<encoded key>) with a
//     single consumer group. Submit and resume timers XADD; workers
//     XREADGROUP; terminal reports and ReportSuspended XACK+XDEL the
//     delivered entry; an XAUTOCLAIM loop redelivers entries pending longer
//     than the in-flight timeout.
//   - Registry: one hash per worker (<prefix>:reg:<workerID>) holding
//     envelope-encoded registrations plus a heartbeat-refreshed deadline; a
//     sweeper detects lapsed deadlines and broadcasts HeartbeatLost on the
//     Pub/Sub feed channel (<prefix>:reg-feed).
//   - Instance events: one Stream per instance (<prefix>:inst:<id>, MAXLEN ~
//     capped) so late subscribers replay the latest lifecycle event and the
//     terminal event; the stream is EXPIREd after the terminal event.
//   - Delayed resume: a sorted set (<prefix>:timers, score = fire time) plus
//     a mover loop that atomically claims due members and XADDs the
//     JobResume.
//   - Cancel of queued jobs: a Lua script scans the key's stream for the
//     instance's JobStart, checks it is not in the group's pending list, and
//     XDELs it atomically; otherwise Cancel falls through to the JobCancel
//     route.
package redis

import (
	"context"
	"crypto/tls"
	"encoding/base64"
	"errors"
	"fmt"
	"strconv"
	"strings"
	"sync"
	"time"

	goredis "github.com/redis/go-redis/v9"

	bl "github.com/friendly-business-machines/blkit/core"
)

// Config configures New.
type Config struct {
	Addr     string      // e.g. "localhost:6379"
	Username string      // optional (Redis ACLs)
	Password string      // optional
	TLS      *tls.Config // nil = plaintext (development)

	// KeyPrefix isolates deployments sharing a server. Default "blkit".
	KeyPrefix string

	// Cipher enables optional end-to-end payload encryption. Default nil.
	Cipher bl.PayloadCipher

	// RegistrationTTL is how long registrations outlive their last
	// heartbeat. Zero means 90s (3x the default 30s heartbeat interval).
	RegistrationTTL time.Duration

	// InFlightTimeout is how long a delivered job may stay unsettled before
	// it is reclaimed and redelivered. Zero means 150s (5x the default
	// heartbeat interval).
	InFlightTimeout time.Duration

	// EventRetention is how long after an instance finishes its event
	// stream stays replayable to late subscribers. Zero means 1h.
	EventRetention time.Duration
}

const (
	jobsGroup        = "workers"
	eventBufferSize  = 64
	instStreamMaxLen = 256
)

// Broker is the Redis/Valkey bl.MessageBroker backend. Create one with New;
// release its connections and goroutines with Close.
type Broker struct {
	cfg    Config
	client *goredis.Client
	ctx    context.Context // cancelled by Close; unblocks every loop
	cancel context.CancelFunc
	wg     sync.WaitGroup

	mu         sync.Mutex
	closed     bool
	groups     map[string]bool       // streams whose consumer group exists
	deliveries map[string][]delivery // delivered, unsettled entries per instance
}

// delivery is one delivered-but-unsettled stream entry.
type delivery struct {
	stream string
	id     string
}

var _ bl.MessageBroker = (*Broker)(nil)

// New connects to the Redis/Valkey server and starts the broker's background
// loops (registry sweeper, timer mover).
func New(cfg Config) (*Broker, error) {
	if cfg.Addr == "" {
		return nil, errors.New("redis message broker: Config.Addr is required")
	}
	if cfg.KeyPrefix == "" {
		cfg.KeyPrefix = "blkit"
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
	client := goredis.NewClient(&goredis.Options{
		Addr:      cfg.Addr,
		Username:  cfg.Username,
		Password:  cfg.Password,
		TLSConfig: cfg.TLS,
	})
	pingCtx, pingCancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer pingCancel()
	if err := client.Ping(pingCtx).Err(); err != nil {
		_ = client.Close()
		return nil, fmt.Errorf("redis message broker: connect %s: %w", cfg.Addr, err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	b := &Broker{
		cfg:        cfg,
		client:     client,
		ctx:        ctx,
		cancel:     cancel,
		groups:     map[string]bool{},
		deliveries: map[string][]delivery{},
	}
	b.wg.Add(2)
	go func() { defer b.wg.Done(); b.sweepRegistry() }()
	go func() { defer b.wg.Done(); b.moveTimers() }()
	return b, nil
}

// Close stops the background loops, closes every subscriber and fetch
// channel, and releases the client. Idempotent.
func (b *Broker) Close() error {
	b.mu.Lock()
	if b.closed {
		b.mu.Unlock()
		return nil
	}
	b.closed = true
	b.mu.Unlock()
	b.cancel()
	b.wg.Wait()
	return b.client.Close()
}

func (b *Broker) checkOpen() error {
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.closed {
		return bl.ErrBrokerClosed
	}
	return nil
}

// goRun starts fn on the broker's waitgroup unless the broker is closed.
func (b *Broker) goRun(fn func()) bool {
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.closed {
		return false
	}
	b.wg.Add(1)
	go func() { defer b.wg.Done(); fn() }()
	return true
}

// ===== Keys =====

func (b *Broker) k(parts ...string) string {
	return b.cfg.KeyPrefix + ":" + strings.Join(parts, ":")
}

func (b *Broker) jobsKey(key bl.ProcessKey) string { return b.k("jobs", encodeKey(key)) }
func (b *Broker) instKey(id string) string         { return b.k("inst", id) }
func (b *Broker) metaKey(id string) string         { return b.k("instmeta", id) }
func (b *Broker) regKey(workerID string) string    { return b.k("reg", workerID) }
func (b *Broker) timersKey() string                { return b.k("timers") }
func (b *Broker) feedChannel() string              { return b.k("reg-feed") }

// encodeKey makes a ProcessKey safe for use inside a Redis key name
// (namespaces contain '/' and '.').
func encodeKey(k bl.ProcessKey) string {
	return base64.RawURLEncoding.EncodeToString([]byte(k.Namespace + "\x00" + k.ProcessID + "\x00" + k.Version))
}

func decodeKey(s string) (bl.ProcessKey, bool) {
	raw, err := base64.RawURLEncoding.DecodeString(s)
	if err != nil {
		return bl.ProcessKey{}, false
	}
	parts := strings.SplitN(string(raw), "\x00", 3)
	if len(parts) != 3 {
		return bl.ProcessKey{}, false
	}
	return bl.ProcessKey{Namespace: parts[0], ProcessID: parts[1], Version: parts[2]}, true
}

func jobKindWire(k bl.JobKind) string {
	switch k {
	case bl.JobStart:
		return bl.KindJobStart
	case bl.JobRespondToInput:
		return bl.KindJobRespondToInput
	case bl.JobCancel:
		return bl.KindJobCancel
	case bl.JobTerminate:
		return bl.KindJobTerminate
	case bl.JobResume:
		return bl.KindJobResume
	default:
		return ""
	}
}

func eventKindName(k bl.InstanceEventKind) string {
	switch k {
	case bl.InstanceEventLifecycle:
		return "lifecycle"
	case bl.InstanceEventInputRequest:
		return "input"
	case bl.InstanceEventNodeCompleted:
		return "node"
	case bl.InstanceEventError:
		return "error"
	case bl.InstanceEventResult:
		return "result"
	default:
		return ""
	}
}

// ===== Lua scripts =====

// sweepScript atomically claims a worker hash whose deadline lapsed: it
// deletes the hash and returns its fields so exactly one sweeper broadcasts
// the HeartbeatLost updates. KEYS[1] = reg hash, ARGV[1] = now (unix ms).
var sweepScript = goredis.NewScript(`
local d = redis.call('HGET', KEYS[1], 'deadline')
if not d or tonumber(d) > tonumber(ARGV[1]) then return {} end
local all = redis.call('HGETALL', KEYS[1])
redis.call('DEL', KEYS[1])
return all
`)

// heartbeatScript refreshes the worker's deadline only if it is still
// registered. KEYS[1] = reg hash, ARGV[1] = deadline (ms), ARGV[2] = now (ms).
var heartbeatScript = goredis.NewScript(`
if redis.call('EXISTS', KEYS[1]) == 0 then return 0 end
redis.call('HSET', KEYS[1], 'deadline', ARGV[1], 'hb', ARGV[2])
return 1
`)

// unregisterScript deletes the worker hash and returns its fields, or nil
// when the worker is unknown. KEYS[1] = reg hash.
var unregisterScript = goredis.NewScript(`
if redis.call('EXISTS', KEYS[1]) == 0 then return false end
local all = redis.call('HGETALL', KEYS[1])
redis.call('DEL', KEYS[1])
return all
`)

// dueTimersScript atomically claims due resume timers. KEYS[1] = timers zset,
// ARGV[1] = now (unix ms).
var dueTimersScript = goredis.NewScript(`
local due = redis.call('ZRANGEBYSCORE', KEYS[1], '-inf', ARGV[1], 'LIMIT', 0, 16)
if #due > 0 then redis.call('ZREM', KEYS[1], unpack(due)) end
return due
`)

// cancelQueuedScript removes the instance's JobStart from the jobs stream
// only if no consumer holds it (it is not in the group's pending list) — the
// scan, the pending check, and the delete are one atomic step. KEYS[1] = jobs
// stream, ARGV[1] = instance id, ARGV[2] = the job.start kind, ARGV[3] =
// group name. Returns 1 when the entry was removed.
var cancelQueuedScript = goredis.NewScript(`
local entries = redis.call('XRANGE', KEYS[1], '-', '+')
for _, e in ipairs(entries) do
  local id = e[1]
  local f = e[2]
  local kind, inst
  for i = 1, #f, 2 do
    if f[i] == 'kind' then kind = f[i+1] end
    if f[i] == 'instance' then inst = f[i+1] end
  end
  if kind == ARGV[2] and inst == ARGV[1] then
    local pend = redis.pcall('XPENDING', KEYS[1], ARGV[3], id, id, 1)
    if type(pend) == 'table' and pend['err'] == nil and #pend == 0 then
      redis.call('XDEL', KEYS[1], id)
      return 1
    end
    return 0
  end
end
return 0
`)

// ===== Producer side =====

// Submit validates the request against the live registry (a direct snapshot
// read — no cold-start wait is needed because the registry is always
// current), records the instance's routing metadata, queues a JobStart, and
// publishes the Pending lifecycle event.
func (b *Broker) Submit(ctx context.Context, req bl.StartRequest) (string, error) {
	if err := ctx.Err(); err != nil {
		return "", err
	}
	if err := b.checkOpen(); err != nil {
		return "", err
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
	meta := map[string]any{"key": encodeKey(req.Key())}
	if req.CorrelationKey != nil {
		meta["corr"] = *req.CorrelationKey
	}
	if err := b.client.HSet(ctx, b.metaKey(instanceID), meta).Err(); err != nil {
		return "", err
	}
	stream := b.jobsKey(req.Key())
	if err := b.ensureGroup(ctx, stream); err != nil {
		return "", err
	}
	if err := b.addJob(ctx, stream, bl.Job{
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
	if err := b.publishInstanceEvent(ctx, instanceID, bl.InstanceEvent{
		Kind:      bl.InstanceEventLifecycle,
		Lifecycle: &bl.LifecycleChange{Phase: bl.ProcessStatusPending},
	}); err != nil {
		return "", err
	}
	return instanceID, nil
}

// Cancel atomically removes a still-queued JobStart (publishing the terminal
// Cancelled event itself — no opt-in needed), or publishes a JobCancel when a
// worker already holds the instance — which requires AllowExternalCancel.
func (b *Broker) Cancel(ctx context.Context, req bl.CancelRequest) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if err := b.checkOpen(); err != nil {
		return err
	}
	reg, err := b.liveRegistration(ctx, req.Key())
	if err != nil {
		return err
	}
	if reg == nil {
		return bl.ErrUnknownProcess
	}
	stream := b.jobsKey(req.Key())
	if err := b.ensureGroup(ctx, stream); err != nil {
		return err
	}
	res, err := cancelQueuedScript.Run(ctx, b.client, []string{stream}, req.InstanceID, bl.KindJobStart, jobsGroup).Result()
	if err != nil {
		return err
	}
	if removed, _ := res.(int64); removed == 1 {
		return b.finish(ctx, req.InstanceID, bl.ProcessStatusCancelled, nil)
	}
	if !reg.AllowExternalCancel {
		return bl.ErrCancelNotAllowed
	}
	return b.addJob(ctx, stream, bl.Job{
		Kind:   bl.JobCancel,
		Key:    req.Key(),
		Cancel: &bl.CancelJob{InstanceID: req.InstanceID, Reason: req.Reason},
	})
}

// Terminate publishes a JobTerminate. Requires AllowExternalTerminate.
func (b *Broker) Terminate(ctx context.Context, req bl.TerminateRequest) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if err := b.checkOpen(); err != nil {
		return err
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
	stream := b.jobsKey(req.Key())
	if err := b.ensureGroup(ctx, stream); err != nil {
		return err
	}
	return b.addJob(ctx, stream, bl.Job{
		Kind:      bl.JobTerminate,
		Key:       req.Key(),
		Terminate: &bl.TerminateJob{InstanceID: req.InstanceID, Reason: req.Reason},
	})
}

// RespondToInputRequest queues a JobRespondToInput for the instance's process
// key, resolved from the instance's routing metadata. An instance the broker
// no longer holds surfaces asynchronously as INSTANCE_NOT_FOUND (or
// ALREADY_FINISHED) on the instance's topic, per the error model.
func (b *Broker) RespondToInputRequest(ctx context.Context, instanceID, requestID string, payload map[string]any) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if err := b.checkOpen(); err != nil {
		return err
	}
	meta, err := b.client.HGetAll(ctx, b.metaKey(instanceID)).Result()
	if err != nil {
		return err
	}
	if len(meta) == 0 || meta["finished"] == "1" {
		code := bl.ErrCodeInstanceNotFound
		if len(meta) > 0 {
			code = bl.ErrCodeAlreadyFinished
		}
		return b.publishInstanceEvent(ctx, instanceID, bl.InstanceEvent{
			Kind:  bl.InstanceEventError,
			Error: &bl.InstanceError{Code: code, Message: "respond-to-input for an instance the broker does not hold"},
		})
	}
	key, ok := decodeKey(meta["key"])
	if !ok {
		return fmt.Errorf("redis message broker: corrupt process key for instance %s", instanceID)
	}
	stream := b.jobsKey(key)
	if err := b.ensureGroup(ctx, stream); err != nil {
		return err
	}
	return b.addJob(ctx, stream, bl.Job{
		Kind:           bl.JobRespondToInput,
		Key:            key,
		RespondToInput: &bl.RespondToInputJob{InstanceID: instanceID, RequestID: requestID, Payload: payload},
	})
}

// instSub is one SubscribeToInstance subscriber. A full buffer drops events;
// recovery is signalled with a synthetic BACKPRESSURE_DROP.
type instSub struct {
	ch      chan bl.InstanceEvent
	dropped bool
}

func (s *instSub) send(evt bl.InstanceEvent) {
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

// SubscribeToInstance replays the instance's latest lifecycle event (and the
// terminal event, if it already finished within the retention window) from
// the instance's stream, then follows it live. An instance with no visible
// events gets INSTANCE_NOT_FOUND as its first event. The channel closes when
// the instance finishes or ctx is cancelled.
func (b *Broker) SubscribeToInstance(ctx context.Context, instanceID string) (<-chan bl.InstanceEvent, error) {
	if err := b.checkOpen(); err != nil {
		return nil, err
	}
	sub := &instSub{ch: make(chan bl.InstanceEvent, eventBufferSize)}
	entries, err := b.client.XRange(ctx, b.instKey(instanceID), "-", "+").Result()
	if err != nil {
		return nil, err
	}
	lastID := "0"
	if len(entries) == 0 {
		exists, err := b.client.Exists(ctx, b.metaKey(instanceID)).Result()
		if err != nil {
			return nil, err
		}
		if exists == 0 {
			sub.ch <- bl.InstanceEvent{
				InstanceID: instanceID,
				Kind:       bl.InstanceEventError,
				OccurredAt: time.Now(),
				Error:      &bl.InstanceError{Code: bl.ErrCodeInstanceNotFound, Message: "no events visible for this instance"},
			}
		}
	} else {
		// Latest-event replay, selected on the cleartext routing fields so
		// only the replayed envelopes need decoding.
		var latest, terminal *goredis.XMessage
		latestPhase := bl.ProcessStatus(-1)
		for i := range entries {
			e := &entries[i]
			switch e.Values["kind"] {
			case "lifecycle":
				latest = e
				if s, _ := e.Values["phase"].(string); s != "" {
					if p, err := strconv.Atoi(s); err == nil {
						latestPhase = bl.ProcessStatus(p)
					}
				}
			case "result":
				terminal = e
			case "error":
				if latestPhase == bl.ProcessStatusFailed {
					terminal = e
				}
			}
		}
		lastID = entries[len(entries)-1].ID
		if latest != nil {
			if evt, ok := b.decodeInstanceEventEntry(*latest); ok {
				sub.ch <- evt
			}
		}
		if terminal != nil {
			if evt, ok := b.decodeInstanceEventEntry(*terminal); ok {
				sub.ch <- evt
			}
		}
		if latestPhase.Finished() {
			close(sub.ch)
			return sub.ch, nil
		}
	}
	fctx, fcancel := context.WithCancel(ctx)
	stopAfter := context.AfterFunc(b.ctx, fcancel)
	if !b.goRun(func() {
		defer stopAfter()
		defer fcancel()
		b.followInstance(fctx, instanceID, sub, lastID)
	}) {
		fcancel()
		stopAfter()
		close(sub.ch)
	}
	return sub.ch, nil
}

// followInstance live-follows the instance's stream from lastID, delivering
// each event and closing the channel after the terminal sequence (terminal
// lifecycle event, then the Result/Error event where one applies) or on ctx
// cancellation.
func (b *Broker) followInstance(ctx context.Context, instanceID string, sub *instSub, lastID string) {
	defer close(sub.ch)
	instK := b.instKey(instanceID)
	awaiting := bl.InstanceEventKind(-1)
	for {
		if ctx.Err() != nil {
			return
		}
		res, err := b.client.XRead(ctx, &goredis.XReadArgs{
			Streams: []string{instK, lastID},
			Count:   int64(eventBufferSize),
			Block:   250 * time.Millisecond,
		}).Result()
		if err != nil {
			if errors.Is(err, goredis.Nil) {
				continue
			}
			if ctx.Err() != nil {
				return
			}
			select { // transient server error: back off briefly
			case <-time.After(100 * time.Millisecond):
			case <-ctx.Done():
				return
			}
			continue
		}
		for _, stream := range res {
			for _, msg := range stream.Messages {
				lastID = msg.ID
				evt, ok := b.decodeInstanceEventEntry(msg)
				if !ok {
					continue
				}
				sub.send(evt)
				if evt.Kind == bl.InstanceEventLifecycle && evt.Lifecycle != nil && evt.Lifecycle.Phase.Finished() {
					switch evt.Lifecycle.Phase {
					case bl.ProcessStatusCompleted:
						awaiting = bl.InstanceEventResult
					case bl.ProcessStatusFailed:
						awaiting = bl.InstanceEventError
					default:
						return // Cancelled has no extra terminal event
					}
				} else if awaiting >= 0 && evt.Kind == awaiting {
					return
				}
			}
		}
	}
}

func (b *Broker) decodeInstanceEventEntry(m goredis.XMessage) (bl.InstanceEvent, bool) {
	raw, _ := m.Values["env"].(string)
	if raw == "" {
		return bl.InstanceEvent{}, false
	}
	env, err := bl.DecodeEnvelope([]byte(raw), b.cfg.Cipher)
	if err != nil {
		return bl.InstanceEvent{}, false
	}
	var evt bl.InstanceEvent
	if err := env.DecodePayload(&evt); err != nil {
		return bl.InstanceEvent{}, false
	}
	return evt, true
}

// SubscribeToProcessRegistry snapshots the registry via SCAN, then follows
// the Pub/Sub feed. The subscription is confirmed before the snapshot is
// taken so no update between the two is missed.
func (b *Broker) SubscribeToProcessRegistry(ctx context.Context) (<-chan bl.RegistryUpdate, error) {
	if err := b.checkOpen(); err != nil {
		return nil, err
	}
	ps := b.client.Subscribe(ctx, b.feedChannel())
	if _, err := ps.Receive(ctx); err != nil {
		_ = ps.Close()
		return nil, err
	}
	regs, err := b.snapshotRegistrations(ctx)
	if err != nil {
		_ = ps.Close()
		return nil, err
	}
	p := newPump()
	for i := range regs {
		p.push(bl.RegistryUpdate{Kind: bl.RegistryUpdateSnapshot, Registration: &regs[i]})
	}
	p.push(bl.RegistryUpdate{Kind: bl.RegistryUpdateSnapshotComplete})

	feed := ps.Channel()
	ok := b.goRun(p.run)
	ok = ok && b.goRun(func() { // feed relay
		for msg := range feed {
			env, err := bl.DecodeEnvelope([]byte(msg.Payload), b.cfg.Cipher)
			if err != nil {
				continue
			}
			var u bl.RegistryUpdate
			if env.DecodePayload(&u) != nil {
				continue
			}
			p.push(u)
		}
	})
	ok = ok && b.goRun(func() { // lifetime watcher
		select {
		case <-ctx.Done():
		case <-b.ctx.Done():
		}
		_ = ps.Close() // ends the feed relay
		p.stop()       // closes p.out
	})
	if !ok {
		_ = ps.Close()
		p.stop()
	}
	return p.out, nil
}

// ===== Worker side: registration =====

// RegisterProcesses replaces the worker's registration set and stamps
// registry metadata (WorkerID, LastHeartbeat; RegisteredAt is preserved per
// key across re-registration). Changes are broadcast on the feed channel.
func (b *Broker) RegisterProcesses(ctx context.Context, workerID string, regs []bl.ProcessRegistration) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if err := b.checkOpen(); err != nil {
		return err
	}
	regKey := b.regKey(workerID)
	existing, err := b.client.HGetAll(ctx, regKey).Result()
	if err != nil {
		return err
	}
	now := time.Now()
	firsts := map[string]time.Time{}
	for f, v := range existing {
		if enc, ok := strings.CutPrefix(f, "first:"); ok {
			if ms, err := strconv.ParseInt(v, 10, 64); err == nil {
				firsts[enc] = time.UnixMilli(ms)
			}
		}
	}
	removed := b.decodeRegFields(existing)
	fields := map[string]any{
		"deadline": strconv.FormatInt(now.Add(b.cfg.RegistrationTTL).UnixMilli(), 10),
		"hb":       strconv.FormatInt(now.UnixMilli(), 10),
	}
	stamped := make([]bl.ProcessRegistration, len(regs))
	for i, r := range regs {
		r.WorkerID = workerID
		enc := encodeKey(r.Key())
		first, ok := firsts[enc]
		if !ok {
			first = now
		}
		r.RegisteredAt = first
		r.LastHeartbeat = now
		data, err := bl.EncodeEnvelope(bl.KindRegistration, "", nil, r, b.cfg.Cipher)
		if err != nil {
			return err
		}
		fields["reg:"+enc] = data
		fields["first:"+enc] = strconv.FormatInt(first.UnixMilli(), 10)
		stamped[i] = r
	}
	pipe := b.client.TxPipeline()
	pipe.Del(ctx, regKey)
	pipe.HSet(ctx, regKey, fields)
	if _, err := pipe.Exec(ctx); err != nil {
		return err
	}
	for i := range removed {
		b.publishRegistryUpdate(ctx, bl.RegistryUpdateRemoved, removed[i])
	}
	for i := range stamped {
		b.publishRegistryUpdate(ctx, bl.RegistryUpdateAdded, stamped[i])
	}
	return nil
}

// Heartbeat refreshes the worker's registration deadline.
func (b *Broker) Heartbeat(ctx context.Context, workerID string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if err := b.checkOpen(); err != nil {
		return err
	}
	now := time.Now()
	res, err := heartbeatScript.Run(ctx, b.client, []string{b.regKey(workerID)},
		strconv.FormatInt(now.Add(b.cfg.RegistrationTTL).UnixMilli(), 10),
		strconv.FormatInt(now.UnixMilli(), 10),
	).Result()
	if err != nil {
		return err
	}
	if n, _ := res.(int64); n == 0 {
		return bl.ErrUnknownWorker
	}
	return nil
}

// Unregister removes the worker's registrations immediately and broadcasts
// Removed updates.
func (b *Broker) Unregister(ctx context.Context, workerID string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if err := b.checkOpen(); err != nil {
		return err
	}
	res, err := unregisterScript.Run(ctx, b.client, []string{b.regKey(workerID)}).Result()
	if errors.Is(err, goredis.Nil) {
		return bl.ErrUnknownWorker
	}
	if err != nil {
		return err
	}
	for _, reg := range b.decodeRegFields(flatToMap(res)) {
		b.publishRegistryUpdate(ctx, bl.RegistryUpdateRemoved, reg)
	}
	return nil
}

// sweepRegistry detects worker hashes whose heartbeat deadline lapsed,
// removes them, and broadcasts HeartbeatLost.
func (b *Broker) sweepRegistry() {
	interval := b.cfg.RegistrationTTL / 4
	interval = max(interval, 25*time.Millisecond)
	interval = min(interval, 5*time.Second)
	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	for {
		select {
		case <-b.ctx.Done():
			return
		case <-ticker.C:
		}
		iter := b.client.Scan(b.ctx, 0, b.k("reg", "*"), 100).Iterator()
		for iter.Next(b.ctx) {
			res, err := sweepScript.Run(b.ctx, b.client, []string{iter.Val()},
				strconv.FormatInt(time.Now().UnixMilli(), 10)).Result()
			if err != nil {
				continue
			}
			for _, reg := range b.decodeRegFields(flatToMap(res)) {
				b.publishRegistryUpdate(b.ctx, bl.RegistryUpdateHeartbeatLost, reg)
			}
		}
	}
}

// snapshotRegistrations reads every live worker's registrations via SCAN.
func (b *Broker) snapshotRegistrations(ctx context.Context) ([]bl.ProcessRegistration, error) {
	var regs []bl.ProcessRegistration
	now := time.Now().UnixMilli()
	iter := b.client.Scan(ctx, 0, b.k("reg", "*"), 100).Iterator()
	for iter.Next(ctx) {
		fields, err := b.client.HGetAll(ctx, iter.Val()).Result()
		if err != nil {
			return nil, err
		}
		deadline, err := strconv.ParseInt(fields["deadline"], 10, 64)
		if err != nil || now > deadline {
			continue // lapsed (sweeper will claim it) or malformed
		}
		regs = append(regs, b.decodeRegFields(fields)...)
	}
	if err := iter.Err(); err != nil {
		return nil, err
	}
	return regs, nil
}

// liveRegistration returns one live registration for the key (any worker),
// or nil when no live worker advertises it.
func (b *Broker) liveRegistration(ctx context.Context, key bl.ProcessKey) (*bl.ProcessRegistration, error) {
	regs, err := b.snapshotRegistrations(ctx)
	if err != nil {
		return nil, err
	}
	for i := range regs {
		if regs[i].Key() == key {
			return &regs[i], nil
		}
	}
	return nil, nil
}

// decodeRegFields decodes the reg:* envelopes of a worker hash, overlaying
// LastHeartbeat from the hash's hb field so heartbeats need not rewrite the
// envelopes.
func (b *Broker) decodeRegFields(fields map[string]string) []bl.ProcessRegistration {
	var hb time.Time
	if ms, err := strconv.ParseInt(fields["hb"], 10, 64); err == nil {
		hb = time.UnixMilli(ms)
	}
	var regs []bl.ProcessRegistration
	for f, v := range fields {
		if !strings.HasPrefix(f, "reg:") {
			continue
		}
		env, err := bl.DecodeEnvelope([]byte(v), b.cfg.Cipher)
		if err != nil {
			continue
		}
		var r bl.ProcessRegistration
		if env.DecodePayload(&r) != nil {
			continue
		}
		if !hb.IsZero() {
			r.LastHeartbeat = hb
		}
		regs = append(regs, r)
	}
	return regs
}

func (b *Broker) publishRegistryUpdate(ctx context.Context, kind bl.RegistryUpdateKind, reg bl.ProcessRegistration) {
	data, err := bl.EncodeEnvelope(bl.KindRegistryUpdate, "", nil, bl.RegistryUpdate{Kind: kind, Registration: &reg}, b.cfg.Cipher)
	if err != nil {
		return
	}
	_ = b.client.Publish(ctx, b.feedChannel(), data).Err()
}

// flatToMap converts a Lua HGETALL reply (flat field/value array) to a map.
func flatToMap(res any) map[string]string {
	flat, _ := res.([]any)
	m := make(map[string]string, len(flat)/2)
	for i := 0; i+1 < len(flat); i += 2 {
		f, _ := flat[i].(string)
		v, _ := flat[i+1].(string)
		m[f] = v
	}
	return m
}

// ===== Worker side: job queue =====

// ensureGroup creates the stream's consumer group at id 0 (so entries added
// before any fetcher attached are still delivered). Idempotent.
func (b *Broker) ensureGroup(ctx context.Context, stream string) error {
	b.mu.Lock()
	if b.groups[stream] {
		b.mu.Unlock()
		return nil
	}
	b.mu.Unlock()
	err := b.client.XGroupCreateMkStream(ctx, stream, jobsGroup, "0").Err()
	if err != nil && !strings.Contains(err.Error(), "BUSYGROUP") {
		return err
	}
	b.mu.Lock()
	b.groups[stream] = true
	b.mu.Unlock()
	return nil
}

// addJob envelope-encodes the job and XADDs it with cleartext routing fields.
func (b *Broker) addJob(ctx context.Context, stream string, job bl.Job) error {
	kind := jobKindWire(job.Kind)
	data, err := bl.EncodeEnvelope(kind, job.InstanceID(), nil, job, b.cfg.Cipher)
	if err != nil {
		return err
	}
	return b.client.XAdd(ctx, &goredis.XAddArgs{
		Stream: stream,
		Values: map[string]any{"env": data, "kind": kind, "instance": job.InstanceID()},
	}).Err()
}

// FetchJobs yields jobs for the given keys. Delivery leaves the entry in the
// consumer group's pending list until a Report* verb settles it; a fetcher
// that dies loses its pending entries to another fetcher's XAUTOCLAIM loop
// after the in-flight timeout.
func (b *Broker) FetchJobs(ctx context.Context, keys []bl.ProcessKey) (<-chan bl.Job, error) {
	if err := b.checkOpen(); err != nil {
		return nil, err
	}
	out := make(chan bl.Job)
	if len(keys) == 0 {
		if !b.goRun(func() {
			defer close(out)
			select {
			case <-ctx.Done():
			case <-b.ctx.Done():
			}
		}) {
			close(out)
		}
		return out, nil
	}
	streams := make([]string, len(keys))
	for i, k := range keys {
		streams[i] = b.jobsKey(k)
		if err := b.ensureGroup(ctx, streams[i]); err != nil {
			return nil, err
		}
	}
	consumer := "c-" + bl.NewProcessInstanceID()
	fctx, fcancel := context.WithCancel(ctx)
	stopAfter := context.AfterFunc(b.ctx, fcancel)
	if !b.goRun(func() {
		defer close(out)
		defer stopAfter()
		defer fcancel()
		b.fetchLoop(fctx, streams, consumer, out)
	}) {
		fcancel()
		stopAfter()
		close(out)
	}
	return out, nil
}

// fetchLoop alternates XREADGROUP for new entries with a periodic XAUTOCLAIM
// pass that reclaims entries pending longer than the in-flight timeout.
func (b *Broker) fetchLoop(ctx context.Context, streams []string, consumer string, out chan<- bl.Job) {
	claimEvery := max(b.cfg.InFlightTimeout/3, 50*time.Millisecond)
	readStreams := make([]string, 0, len(streams)*2)
	readStreams = append(readStreams, streams...)
	for range streams {
		readStreams = append(readStreams, ">")
	}
	var lastClaim time.Time
	for {
		if ctx.Err() != nil {
			return
		}
		if time.Since(lastClaim) >= claimEvery {
			lastClaim = time.Now()
			for _, s := range streams {
				msgs, _, err := b.client.XAutoClaim(ctx, &goredis.XAutoClaimArgs{
					Stream:   s,
					Group:    jobsGroup,
					Consumer: consumer,
					MinIdle:  b.cfg.InFlightTimeout,
					Start:    "0-0",
					Count:    16,
				}).Result()
				if err != nil {
					continue
				}
				for _, m := range msgs {
					if !b.deliverJob(ctx, s, m, out) {
						return
					}
				}
			}
		}
		res, err := b.client.XReadGroup(ctx, &goredis.XReadGroupArgs{
			Group:    jobsGroup,
			Consumer: consumer,
			Streams:  readStreams,
			Count:    16,
			Block:    250 * time.Millisecond,
		}).Result()
		if err != nil {
			if errors.Is(err, goredis.Nil) {
				continue
			}
			if ctx.Err() != nil {
				return
			}
			select { // transient server error: back off briefly
			case <-time.After(100 * time.Millisecond):
			case <-ctx.Done():
				return
			}
			continue
		}
		for _, sr := range res {
			for _, m := range sr.Messages {
				if !b.deliverJob(ctx, sr.Stream, m, out) {
					return
				}
			}
		}
	}
}

// deliverJob decodes one stream entry, records it as delivered-but-unsettled,
// and hands the job to the fetcher's channel. Undecodable entries are settled
// immediately so they cannot loop through redelivery forever.
func (b *Broker) deliverJob(ctx context.Context, stream string, m goredis.XMessage, out chan<- bl.Job) bool {
	raw, _ := m.Values["env"].(string)
	env, err := bl.DecodeEnvelope([]byte(raw), b.cfg.Cipher)
	var job bl.Job
	if err == nil {
		err = env.DecodePayload(&job)
	}
	if err != nil {
		b.client.XAck(ctx, stream, jobsGroup, m.ID)
		b.client.XDel(ctx, stream, m.ID)
		return true
	}
	instanceID := job.InstanceID()
	b.mu.Lock()
	b.deliveries[instanceID] = append(b.deliveries[instanceID], delivery{stream: stream, id: m.ID})
	b.mu.Unlock()
	select {
	case out <- job:
		return true
	case <-ctx.Done():
		// Not handed over and not settled: the pending entry is reclaimed
		// after the in-flight timeout.
		return false
	}
}

// settle acknowledges and deletes every delivered entry for the instance.
func (b *Broker) settle(ctx context.Context, instanceID string) {
	b.mu.Lock()
	ds := b.deliveries[instanceID]
	delete(b.deliveries, instanceID)
	b.mu.Unlock()
	for _, d := range ds {
		b.client.XAck(ctx, d.stream, jobsGroup, d.id)
		b.client.XDel(ctx, d.stream, d.id)
	}
}

// ===== Worker side: lifecycle reports and instance topic =====

func (b *Broker) report(ctx context.Context, instanceID string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if instanceID == "" {
		return bl.ErrUnknownProcess
	}
	return b.checkOpen()
}

// ReportRunning publishes the Running lifecycle event. Does not settle the
// in-flight job.
func (b *Broker) ReportRunning(ctx context.Context, instanceID string) error {
	if err := b.report(ctx, instanceID); err != nil {
		return err
	}
	return b.publishInstanceEvent(ctx, instanceID, bl.InstanceEvent{
		Kind:      bl.InstanceEventLifecycle,
		Lifecycle: &bl.LifecycleChange{Phase: bl.ProcessStatusRunning},
	})
}

// ReportSuspended publishes the Suspended lifecycle event, settles the
// in-flight job, and — given a resumeAt — schedules the JobResume via the
// timers sorted set.
func (b *Broker) ReportSuspended(ctx context.Context, instanceID string, resumeAt *time.Time) error {
	if err := b.report(ctx, instanceID); err != nil {
		return err
	}
	if err := b.publishInstanceEvent(ctx, instanceID, bl.InstanceEvent{
		Kind:      bl.InstanceEventLifecycle,
		Lifecycle: &bl.LifecycleChange{Phase: bl.ProcessStatusSuspended},
	}); err != nil {
		return err
	}
	b.settle(ctx, instanceID)
	if resumeAt == nil {
		return nil // external-input wait: RespondToInputRequest resumes it
	}
	return b.client.ZAdd(ctx, b.timersKey(), goredis.Z{
		Score:  float64(resumeAt.UnixMilli()),
		Member: instanceID,
	}).Err()
}

// ReportCompleted publishes the Completed lifecycle event and the Result
// event, settles the job, and lets subscriber channels close.
func (b *Broker) ReportCompleted(ctx context.Context, instanceID string, result bl.EvaluationResult) error {
	if err := b.report(ctx, instanceID); err != nil {
		return err
	}
	return b.finish(ctx, instanceID, bl.ProcessStatusCompleted, &bl.InstanceEvent{
		Kind:   bl.InstanceEventResult,
		Result: &result,
	})
}

// ReportFailed publishes the Failed lifecycle event and the Error event,
// settles the job, and lets subscriber channels close.
func (b *Broker) ReportFailed(ctx context.Context, instanceID string, instErr bl.InstanceError) error {
	if err := b.report(ctx, instanceID); err != nil {
		return err
	}
	return b.finish(ctx, instanceID, bl.ProcessStatusFailed, &bl.InstanceEvent{
		Kind:  bl.InstanceEventError,
		Error: &instErr,
	})
}

// ReportCancelled publishes the Cancelled lifecycle event, settles the job,
// and lets subscriber channels close.
func (b *Broker) ReportCancelled(ctx context.Context, instanceID string) error {
	if err := b.report(ctx, instanceID); err != nil {
		return err
	}
	return b.finish(ctx, instanceID, bl.ProcessStatusCancelled, nil)
}

// PostError publishes a non-terminal error event.
func (b *Broker) PostError(ctx context.Context, instanceID string, instErr bl.InstanceError) error {
	if err := b.report(ctx, instanceID); err != nil {
		return err
	}
	return b.publishInstanceEvent(ctx, instanceID, bl.InstanceEvent{Kind: bl.InstanceEventError, Error: &instErr})
}

// PostInputRequest publishes a RequestInputTask's request for input.
func (b *Broker) PostInputRequest(ctx context.Context, instanceID string, req bl.InputRequest) error {
	if err := b.report(ctx, instanceID); err != nil {
		return err
	}
	return b.publishInstanceEvent(ctx, instanceID, bl.InstanceEvent{Kind: bl.InstanceEventInputRequest, InputRequest: &req})
}

// PostNodeCompleted publishes a node-completion event.
func (b *Broker) PostNodeCompleted(ctx context.Context, instanceID string, nc bl.NodeCompleted) error {
	if err := b.report(ctx, instanceID); err != nil {
		return err
	}
	return b.publishInstanceEvent(ctx, instanceID, bl.InstanceEvent{Kind: bl.InstanceEventNodeCompleted, NodeCompleted: &nc})
}

// finish drives the instance to a terminal phase: settle the in-flight job,
// drop any pending resume timer, publish the terminal lifecycle event and the
// extra Result/Error event, and start the retention window on the instance's
// keys. The finished flag makes this idempotent.
func (b *Broker) finish(ctx context.Context, instanceID string, phase bl.ProcessStatus, extra *bl.InstanceEvent) error {
	b.settle(ctx, instanceID)
	b.client.ZRem(ctx, b.timersKey(), instanceID)
	set, err := b.client.HSetNX(ctx, b.metaKey(instanceID), "finished", "1").Result()
	if err != nil {
		return err
	}
	if !set {
		return nil // already finished
	}
	if err := b.publishInstanceEvent(ctx, instanceID, bl.InstanceEvent{
		Kind:      bl.InstanceEventLifecycle,
		Lifecycle: &bl.LifecycleChange{Phase: phase},
	}); err != nil {
		return err
	}
	if extra != nil {
		if err := b.publishInstanceEvent(ctx, instanceID, *extra); err != nil {
			return err
		}
	}
	b.client.PExpire(ctx, b.metaKey(instanceID), b.cfg.EventRetention)
	b.client.PExpire(ctx, b.instKey(instanceID), b.cfg.EventRetention)
	return nil
}

// publishInstanceEvent stamps the event (id, mirrored correlation key,
// occurred-at), envelope-encodes it, and XADDs it to the instance's stream
// with cleartext routing fields.
func (b *Broker) publishInstanceEvent(ctx context.Context, instanceID string, evt bl.InstanceEvent) error {
	var corr *string
	if v, err := b.client.HGet(ctx, b.metaKey(instanceID), "corr").Result(); err == nil {
		corr = &v
	}
	evt.InstanceID = instanceID
	evt.CorrelationKey = corr
	evt.OccurredAt = time.Now()
	data, err := bl.EncodeEnvelope(bl.KindInstanceEvent, instanceID, corr, evt, b.cfg.Cipher)
	if err != nil {
		return err
	}
	values := map[string]any{"env": data, "kind": eventKindName(evt.Kind)}
	if evt.Kind == bl.InstanceEventLifecycle && evt.Lifecycle != nil {
		values["phase"] = strconv.Itoa(int(evt.Lifecycle.Phase))
	}
	return b.client.XAdd(ctx, &goredis.XAddArgs{
		Stream: b.instKey(instanceID),
		MaxLen: instStreamMaxLen,
		Approx: true,
		Values: values,
	}).Err()
}

// ===== Delayed resume =====

// moveTimers periodically claims due members of the timers sorted set
// (atomically, via Lua) and publishes the JobResume to the instance's
// process-key stream.
func (b *Broker) moveTimers() {
	ticker := time.NewTicker(100 * time.Millisecond)
	defer ticker.Stop()
	for {
		select {
		case <-b.ctx.Done():
			return
		case <-ticker.C:
		}
		res, err := dueTimersScript.Run(b.ctx, b.client, []string{b.timersKey()},
			strconv.FormatInt(time.Now().UnixMilli(), 10)).Result()
		if err != nil {
			continue
		}
		due, _ := res.([]any)
		for _, v := range due {
			if id, _ := v.(string); id != "" {
				b.fireTimer(id)
			}
		}
	}
}

// fireTimer publishes the JobResume for one due timer, unless the instance
// meanwhile finished or fell out of retention.
func (b *Broker) fireTimer(instanceID string) {
	meta, err := b.client.HGetAll(b.ctx, b.metaKey(instanceID)).Result()
	if err != nil || len(meta) == 0 || meta["finished"] == "1" {
		return
	}
	key, ok := decodeKey(meta["key"])
	if !ok {
		return
	}
	stream := b.jobsKey(key)
	if b.ensureGroup(b.ctx, stream) != nil {
		return
	}
	_ = b.addJob(b.ctx, stream, bl.Job{
		Kind:   bl.JobResume,
		Key:    key,
		Resume: &bl.ResumeJob{InstanceID: instanceID},
	})
}

// ===== Registry pump =====

// pump is a lossless per-watcher delivery queue: registry updates must not
// drop, so each watcher gets an unbounded buffer drained by its own
// goroutine.
type pump struct {
	mu     sync.Mutex
	queue  []bl.RegistryUpdate
	signal chan struct{}
	done   chan struct{}
	once   sync.Once
	out    chan bl.RegistryUpdate
}

func newPump() *pump {
	return &pump{
		signal: make(chan struct{}, 1),
		done:   make(chan struct{}),
		out:    make(chan bl.RegistryUpdate),
	}
}

func (p *pump) push(u bl.RegistryUpdate) {
	p.mu.Lock()
	p.queue = append(p.queue, u)
	p.mu.Unlock()
	select {
	case p.signal <- struct{}{}:
	default:
	}
}

func (p *pump) stop() {
	p.once.Do(func() { close(p.done) })
}

func (p *pump) run() {
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
