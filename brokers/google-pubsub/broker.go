// Package gpubsub implements bl.MessageBroker on Google Cloud Pub/Sub
// (specs/message-brokers/google-pubsub-message-broker.spec.md).
//
// Mapping to primitives:
//
//   - Jobs ride one topic (<prefix>-jobs) with one pull subscription per
//     process key, created with a server-side attribute filter on the key.
//     Because the Pub/Sub emulator accepts but does not always enforce
//     filters, consumers additionally filter client-side (skipping and
//     acking mismatches — safe, since each key has its own subscription and
//     skipped messages are that subscription's own copies).
//   - Instance events ride one topic (<prefix>-inst-events) with one
//     filtered, ordered subscription per subscriber. Latest-event replay for
//     late subscribers comes from a last-event record in the RegistryStore
//     (see InstanceStore), not from topic retention + Seek — the emulator's
//     Seek support is unreliable and a record read is cheaper than a
//     retention replay in production too.
//   - The worker registry and durable timers live in a bl.RegistryStore
//     side-store (FirestoreRegistryStore ships with this module), since
//     Pub/Sub has no KV facility. Delayed JobResume delivery is a timer
//     record plus a broker-owned scheduler loop, as Pub/Sub has no native
//     delayed delivery.
//   - Cancel of a still-queued job is unsupported natively (Pub/Sub has no
//     message deletion): Cancel always takes the opt-in-checked JobCancel
//     route.
//   - Redelivery after a worker crash relies on the subscription's ack
//     deadline lapsing (min 10s on Pub/Sub); the client lease-extends
//     in-flight jobs up to Config.InFlightTimeout.
package gpubsub

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"net/url"
	"sync"
	"time"

	"cloud.google.com/go/pubsub"
	"golang.org/x/oauth2/google"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	bl "github.com/friendly-business-machines/blkit/core"
)

// Message attributes: cleartext routing metadata duplicated from the
// envelope, per the wire-format spec.
const (
	attrKey        = "key"        // process key, for job-subscription filters
	attrKind       = "kind"       // envelope kind
	attrInstanceID = "instanceID" // for event-subscription filters
	attrEventID    = "eventID"    // replay duplicate suppression
)

const (
	eventBufferSize   = 64               // per-subscriber buffer; overflow drops + BACKPRESSURE_DROP
	minAckDeadline    = 10 * time.Second // Pub/Sub's minimum
	maxAckDeadline    = 600 * time.Second
	schedulerInterval = 200 * time.Millisecond
)

// Config configures New.
type Config struct {
	ProjectID   string              // GCP project owning the topics/subscriptions
	Credentials *google.Credentials // nil = Application Default Credentials

	// EntityPrefix isolates deployments sharing a project; default "blkit".
	EntityPrefix string

	// Registry is required: worker registry, timers, and instance
	// last-event records. It must also implement InstanceStore
	// (FirestoreRegistryStore does). Its lifecycle belongs to the caller —
	// Close on the broker does not close it.
	Registry bl.RegistryStore

	// Endpoint is a development-only emulator endpoint override
	// (host:port). Production traffic always uses TLS through the SDK.
	Endpoint string

	// Cipher enables end-to-end payload encryption; default nil.
	Cipher bl.PayloadCipher

	// RegistrationTTL is how long registrations outlive their last
	// heartbeat; default 90s (3× the default 30s heartbeat interval).
	RegistrationTTL time.Duration

	// InFlightTimeout bounds how long a delivered, unsettled job stays
	// leased before Pub/Sub redelivers it; default 150s. Values below
	// Pub/Sub's 10s minimum ack deadline are raised to it.
	InFlightTimeout time.Duration

	// EventRetention is how long a finished instance's latest-event record
	// stays replayable to late subscribers; default 1h.
	EventRetention time.Duration
}

// Broker is the Google Pub/Sub bl.MessageBroker. Create one with New.
type Broker struct {
	cfg    Config
	client *pubsub.Client
	jobs   *pubsub.Topic
	events *pubsub.Topic
	store  InstanceStore

	mu       sync.Mutex
	closed   bool
	inFlight map[string][]*inFlightMsg // unsettled job messages per instance
	jobSubs  map[string]bool           // job subscriptions already ensured

	done chan struct{}
	wg   sync.WaitGroup
}

var _ bl.MessageBroker = (*Broker)(nil)

// New connects to Pub/Sub, ensures the jobs and instance-events topics
// exist, and starts the timer scheduler. Release its resources with Close.
func New(cfg Config) (*Broker, error) {
	if cfg.ProjectID == "" {
		return nil, errors.New("gpubsub: Config.ProjectID is required")
	}
	if cfg.Registry == nil {
		return nil, errors.New("gpubsub: Config.Registry is required")
	}
	store, ok := cfg.Registry.(InstanceStore)
	if !ok {
		return nil, errors.New("gpubsub: Config.Registry must implement gpubsub.InstanceStore (FirestoreRegistryStore does); Pub/Sub cannot replay events to late subscribers without last-event records")
	}
	if cfg.EntityPrefix == "" {
		cfg.EntityPrefix = "blkit"
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

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	client, err := pubsub.NewClient(ctx, cfg.ProjectID, gcloudClientOptions(cfg.Credentials, cfg.Endpoint)...)
	if err != nil {
		return nil, fmt.Errorf("gpubsub: pubsub client: %w", err)
	}
	b := &Broker{
		cfg:      cfg,
		client:   client,
		store:    store,
		inFlight: map[string][]*inFlightMsg{},
		jobSubs:  map[string]bool{},
		done:     make(chan struct{}),
	}
	if b.jobs, err = b.ensureTopic(ctx, cfg.EntityPrefix+"-jobs"); err != nil {
		_ = client.Close()
		return nil, err
	}
	if b.events, err = b.ensureTopic(ctx, cfg.EntityPrefix+"-inst-events"); err != nil {
		_ = client.Close()
		return nil, err
	}
	b.events.EnableMessageOrdering = true
	b.wg.Add(1)
	go b.schedulerLoop()
	return b, nil
}

// Close stops the scheduler and every fetch/subscribe goroutine, flushes
// pending publishes, and closes the Pub/Sub client. It does not close the
// RegistryStore, whose lifecycle belongs to the caller. Idempotent.
func (b *Broker) Close() error {
	b.mu.Lock()
	if b.closed {
		b.mu.Unlock()
		return nil
	}
	b.closed = true
	close(b.done)
	b.mu.Unlock()
	b.jobs.Stop()
	b.events.Stop()
	b.wg.Wait()
	return b.client.Close()
}

func (b *Broker) ensureTopic(ctx context.Context, id string) (*pubsub.Topic, error) {
	topic, err := b.client.CreateTopic(ctx, id)
	if status.Code(err) == codes.AlreadyExists {
		return b.client.Topic(id), nil
	}
	if err != nil {
		return nil, fmt.Errorf("gpubsub: create topic %s: %w", id, err)
	}
	return topic, nil
}

// guard rejects calls on a cancelled context or a closed broker.
func (b *Broker) guard(ctx context.Context) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.closed {
		return bl.ErrBrokerClosed
	}
	return nil
}

// ackDeadline clamps InFlightTimeout into Pub/Sub's allowed range.
func (b *Broker) ackDeadline() time.Duration {
	return min(max(b.cfg.InFlightTimeout, minAckDeadline), maxAckDeadline)
}

// keyAttr renders a process key as a filter-safe attribute value.
func keyAttr(k bl.ProcessKey) string {
	return url.QueryEscape(k.Namespace) + "|" + url.QueryEscape(k.ProcessID) + "|" + url.QueryEscape(k.Version)
}

func shortHash(s string) string {
	sum := sha256.Sum256([]byte(s))
	return hex.EncodeToString(sum[:6])
}

// liveRegistration returns one live registration for the key, from the
// RegistryStore. The store is authoritative and immediately consistent, so
// there is no cold-start snapshot wait: an unadvertised process is genuinely
// unknown.
func (b *Broker) liveRegistration(ctx context.Context, key bl.ProcessKey) (*bl.ProcessRegistration, error) {
	regs, err := b.cfg.Registry.Snapshot(ctx)
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

// liveRecord applies the event-retention window: a finished record past
// retention is treated as absent.
func (b *Broker) liveRecord(rec *InstanceRecord) *InstanceRecord {
	if rec != nil && rec.Finished && time.Since(rec.UpdatedAt) > b.cfg.EventRetention {
		return nil
	}
	return rec
}

// ===== Producer side =====

// Submit validates the request against the live registry, creates the
// instance's last-event record, publishes the JobStart, and publishes the
// Pending lifecycle event.
func (b *Broker) Submit(ctx context.Context, req bl.StartRequest) (string, error) {
	if err := b.guard(ctx); err != nil {
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
	pending := bl.InstanceEvent{
		InstanceID:     instanceID,
		CorrelationKey: req.CorrelationKey,
		Kind:           bl.InstanceEventLifecycle,
		OccurredAt:     time.Now(),
		Lifecycle:      &bl.LifecycleChange{Phase: bl.ProcessStatusPending},
	}
	stored, data, err := b.encodeEvent(pending)
	if err != nil {
		return "", err
	}
	if err := b.store.PutInstance(ctx, InstanceRecord{
		InstanceID:     instanceID,
		Key:            req.Key(),
		CorrelationKey: req.CorrelationKey,
		Latest:         stored,
	}); err != nil {
		return "", err
	}
	if err := b.publishJob(ctx, req.Key(), bl.Job{
		Kind: bl.JobStart,
		Key:  req.Key(),
		Start: &bl.StartJob{
			InstanceID:     instanceID,
			StartID:        req.StartID,
			Input:          req.Input,
			CorrelationKey: req.CorrelationKey,
		},
	}, req.CorrelationKey); err != nil {
		return "", err
	}
	if err := b.publishEncodedEvent(ctx, instanceID, stored.EventID, data); err != nil {
		return "", err
	}
	return instanceID, nil
}

// Cancel always takes the JobCancel route: Pub/Sub cannot remove a queued
// message, so there is no pre-execution queue-removal path and every cancel
// requires AllowExternalCancel.
func (b *Broker) Cancel(ctx context.Context, req bl.CancelRequest) error {
	if err := b.guard(ctx); err != nil {
		return err
	}
	reg, err := b.liveRegistration(ctx, req.Key())
	if err != nil {
		return err
	}
	if reg == nil {
		return bl.ErrUnknownProcess
	}
	if !reg.AllowExternalCancel {
		return bl.ErrCancelNotAllowed
	}
	return b.publishJob(ctx, req.Key(), bl.Job{
		Kind:   bl.JobCancel,
		Key:    req.Key(),
		Cancel: &bl.CancelJob{InstanceID: req.InstanceID, Reason: req.Reason},
	}, nil)
}

// Terminate publishes a JobTerminate. Requires AllowExternalTerminate.
func (b *Broker) Terminate(ctx context.Context, req bl.TerminateRequest) error {
	if err := b.guard(ctx); err != nil {
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
	return b.publishJob(ctx, req.Key(), bl.Job{
		Kind:      bl.JobTerminate,
		Key:       req.Key(),
		Terminate: &bl.TerminateJob{InstanceID: req.InstanceID, Reason: req.Reason},
	}, nil)
}

// RespondToInputRequest routes the response to the instance's process key
// via the last-event record. An unknown or finished instance surfaces
// asynchronously on the instance's topic, per the error model.
func (b *Broker) RespondToInputRequest(ctx context.Context, instanceID, requestID string, payload map[string]any) error {
	if err := b.guard(ctx); err != nil {
		return err
	}
	rec, err := b.store.GetInstance(ctx, instanceID)
	if err != nil {
		return err
	}
	rec = b.liveRecord(rec)
	if rec == nil || rec.Finished {
		code := bl.ErrCodeInstanceNotFound
		if rec != nil {
			code = bl.ErrCodeAlreadyFinished
		}
		return b.publishInstanceEvent(ctx, instanceID, bl.InstanceEvent{
			Kind:  bl.InstanceEventError,
			Error: &bl.InstanceError{Code: code, Message: "respond-to-input for an instance the broker does not hold"},
		}, false)
	}
	return b.publishJob(ctx, rec.Key, bl.Job{
		Kind:           bl.JobRespondToInput,
		Key:            rec.Key,
		RespondToInput: &bl.RespondToInputJob{InstanceID: instanceID, RequestID: requestID, Payload: payload},
	}, rec.CorrelationKey)
}

// SubscribeToProcessRegistry proxies the RegistryStore's watch feed, which
// delivers the snapshot, the SnapshotComplete sentinel, then incremental
// updates until ctx is cancelled.
func (b *Broker) SubscribeToProcessRegistry(ctx context.Context) (<-chan bl.RegistryUpdate, error) {
	if err := b.guard(ctx); err != nil {
		return nil, err
	}
	return b.cfg.Registry.Watch(ctx)
}

// ===== Worker side: registration =====

// RegisterProcesses replaces the worker's registration set. WorkerID,
// RegisteredAt, and LastHeartbeat are stamped store-side.
func (b *Broker) RegisterProcesses(ctx context.Context, workerID string, regs []bl.ProcessRegistration) error {
	if err := b.guard(ctx); err != nil {
		return err
	}
	stamped := make([]bl.ProcessRegistration, len(regs))
	for i, r := range regs {
		r.WorkerID = workerID
		stamped[i] = r
	}
	if err := b.cfg.Registry.PutRegistrations(ctx, workerID, stamped, b.cfg.RegistrationTTL); err != nil {
		return err
	}
	// Ensure the per-key job subscriptions exist before Submit can target
	// the keys: Pub/Sub drops messages published before any subscription
	// existed, and Submit requires a live registration — which this call
	// just created.
	for i := range stamped {
		if _, err := b.ensureJobSubscription(ctx, stamped[i].Key()); err != nil {
			return err
		}
	}
	return nil
}

// Heartbeat refreshes the worker's registration TTL.
func (b *Broker) Heartbeat(ctx context.Context, workerID string) error {
	if err := b.guard(ctx); err != nil {
		return err
	}
	return b.cfg.Registry.Touch(ctx, workerID, b.cfg.RegistrationTTL)
}

// Unregister removes the worker's registrations immediately.
func (b *Broker) Unregister(ctx context.Context, workerID string) error {
	if err := b.guard(ctx); err != nil {
		return err
	}
	return b.cfg.Registry.Delete(ctx, workerID)
}

// ===== Worker side: job queue =====

// ensureJobSubscription creates (idempotently) the per-key pull subscription
// on the jobs topic, filtered server-side on the key attribute. If the
// backend rejects the filter (older emulators), it falls back to an
// unfiltered subscription — receiveJobs always re-checks client-side.
func (b *Broker) ensureJobSubscription(ctx context.Context, key bl.ProcessKey) (*pubsub.Subscription, error) {
	name := b.cfg.EntityPrefix + "-jobs-" + shortHash(keyAttr(key))
	b.mu.Lock()
	known := b.jobSubs[name]
	b.mu.Unlock()
	if known {
		return b.client.Subscription(name), nil
	}
	base := pubsub.SubscriptionConfig{Topic: b.jobs, AckDeadline: b.ackDeadline()}
	withFilter := base
	withFilter.Filter = fmt.Sprintf("attributes.%s = %q", attrKey, keyAttr(key))
	if err := b.createSubscription(ctx, name, withFilter, base); err != nil {
		return nil, fmt.Errorf("gpubsub: create job subscription %s: %w", name, err)
	}
	b.mu.Lock()
	b.jobSubs[name] = true
	b.mu.Unlock()
	return b.client.Subscription(name), nil
}

// createSubscription tries each candidate config in order, treating
// AlreadyExists as success. Later candidates progressively drop features an
// emulator may reject (filters, ordering).
func (b *Broker) createSubscription(ctx context.Context, name string, candidates ...pubsub.SubscriptionConfig) error {
	var err error
	for _, cfg := range candidates {
		_, err = b.client.CreateSubscription(ctx, name, cfg)
		if err == nil || status.Code(err) == codes.AlreadyExists {
			return nil
		}
	}
	return err
}

// FetchJobs yields jobs for the given keys from their per-key subscriptions.
// A delivered job stays leased (in-flight) until a terminal Report* verb or
// ReportSuspended acks it; lease extension stops at InFlightTimeout, after
// which Pub/Sub redelivers.
func (b *Broker) FetchJobs(ctx context.Context, keys []bl.ProcessKey) (<-chan bl.Job, error) {
	if err := b.guard(ctx); err != nil {
		return nil, err
	}
	out := make(chan bl.Job)
	rctx, cancel := context.WithCancel(ctx)
	b.wg.Add(1)
	go func() {
		defer b.wg.Done()
		select {
		case <-b.done:
		case <-rctx.Done():
		}
		cancel()
	}()
	if len(keys) == 0 {
		go func() {
			<-rctx.Done()
			close(out)
		}()
		return out, nil
	}
	subs := make([]*pubsub.Subscription, len(keys))
	for i, k := range keys {
		sub, err := b.ensureJobSubscription(ctx, k)
		if err != nil {
			cancel()
			return nil, err
		}
		subs[i] = sub
	}
	var wg sync.WaitGroup
	for i := range subs {
		wg.Add(1)
		b.wg.Add(1)
		go func(sub *pubsub.Subscription, key bl.ProcessKey) {
			defer b.wg.Done()
			defer wg.Done()
			b.receiveJobs(rctx, sub, key, out)
		}(subs[i], keys[i])
	}
	go func() {
		wg.Wait()
		close(out)
	}()
	return out, nil
}

// inFlightMsg is one delivered, unsettled job message. Exactly one of ack or
// nack ever reaches Pub/Sub, whichever comes first.
type inFlightMsg struct {
	instanceID string
	msg        *pubsub.Message
	fetch      *jobFetcher
	once       sync.Once
}

func (m *inFlightMsg) ack()  { m.once.Do(m.msg.Ack) }
func (m *inFlightMsg) nack() { m.once.Do(m.msg.Nack) }

// jobFetcher tracks one receiveJobs loop's unsettled messages so they can be
// nacked when the loop shuts down. Guarded by Broker.mu.
type jobFetcher struct {
	closed bool
	msgs   map[*inFlightMsg]struct{}
}

// receiveJobs pulls one subscription and forwards decoded, key-matching jobs
// to out. Mismatched keys (unenforced emulator filters) are acked and
// skipped — each key owns its subscription, so the skipped message is this
// subscription's own copy. Delivered messages are tracked in-flight by
// instance until a report verb settles them; when the loop's context ends,
// still-unsettled messages are nacked so Pub/Sub redelivers them promptly to
// another worker (and so Receive, which waits on outstanding messages, can
// return).
func (b *Broker) receiveJobs(ctx context.Context, sub *pubsub.Subscription, key bl.ProcessKey, out chan<- bl.Job) {
	sub.ReceiveSettings.NumGoroutines = 1
	sub.ReceiveSettings.MaxOutstandingMessages = 16
	sub.ReceiveSettings.MaxExtension = b.cfg.InFlightTimeout
	f := &jobFetcher{msgs: map[*inFlightMsg]struct{}{}}
	stop := context.AfterFunc(ctx, func() { b.nackFetcher(f) })
	defer stop()
	want := keyAttr(key)
	_ = sub.Receive(ctx, func(_ context.Context, msg *pubsub.Message) {
		if msg.Attributes[attrKey] != want {
			msg.Ack()
			return
		}
		env, err := bl.DecodeEnvelope(msg.Data, b.cfg.Cipher)
		if err != nil {
			msg.Ack() // poison message: acked, never processed
			return
		}
		var job bl.Job
		if err := env.DecodePayload(&job); err != nil {
			msg.Ack()
			return
		}
		m := &inFlightMsg{instanceID: job.InstanceID(), msg: msg, fetch: f}
		if !b.trackInFlight(m) {
			m.nack() // fetcher already shutting down
			return
		}
		select {
		case out <- job:
		case <-ctx.Done():
			b.untrackInFlight(m)
			m.nack()
		}
	})
	b.nackFetcher(f)
}

// trackInFlight registers a delivered message; false means the fetcher is
// already shutting down and the message must be nacked instead.
func (b *Broker) trackInFlight(m *inFlightMsg) bool {
	b.mu.Lock()
	defer b.mu.Unlock()
	if m.fetch.closed {
		return false
	}
	b.inFlight[m.instanceID] = append(b.inFlight[m.instanceID], m)
	m.fetch.msgs[m] = struct{}{}
	return true
}

func (b *Broker) untrackInFlight(m *inFlightMsg) {
	b.mu.Lock()
	defer b.mu.Unlock()
	b.removeInFlightLocked(m)
}

func (b *Broker) removeInFlightLocked(m *inFlightMsg) {
	msgs := b.inFlight[m.instanceID]
	for i, x := range msgs {
		if x == m {
			b.inFlight[m.instanceID] = append(msgs[:i], msgs[i+1:]...)
			break
		}
	}
	if len(b.inFlight[m.instanceID]) == 0 {
		delete(b.inFlight, m.instanceID)
	}
	delete(m.fetch.msgs, m)
}

// nackFetcher nacks every message the fetcher still holds unsettled.
func (b *Broker) nackFetcher(f *jobFetcher) {
	b.mu.Lock()
	f.closed = true
	pending := make([]*inFlightMsg, 0, len(f.msgs))
	for m := range f.msgs {
		pending = append(pending, m)
	}
	for _, m := range pending {
		b.removeInFlightLocked(m)
	}
	b.mu.Unlock()
	for _, m := range pending {
		m.nack()
	}
}

// settle acks every in-flight job message held for the instance.
func (b *Broker) settle(instanceID string) {
	b.mu.Lock()
	msgs := b.inFlight[instanceID]
	delete(b.inFlight, instanceID)
	for _, m := range msgs {
		delete(m.fetch.msgs, m)
	}
	b.mu.Unlock()
	for _, m := range msgs {
		m.ack()
	}
}

// publishJob envelope-encodes and publishes one job to the jobs topic, with
// the process key duplicated into the filterable key attribute.
func (b *Broker) publishJob(ctx context.Context, key bl.ProcessKey, job bl.Job, corr *string) error {
	kind := jobKindNames[job.Kind]
	data, err := bl.EncodeEnvelope(kind, job.InstanceID(), corr, job, b.cfg.Cipher)
	if err != nil {
		return err
	}
	res := b.jobs.Publish(ctx, &pubsub.Message{
		Data: data,
		Attributes: map[string]string{
			attrKey:        keyAttr(key),
			attrKind:       kind,
			attrInstanceID: job.InstanceID(),
		},
	})
	if _, err := res.Get(ctx); err != nil {
		return fmt.Errorf("gpubsub: publish %s: %w", kind, err)
	}
	return nil
}

var jobKindNames = map[bl.JobKind]string{
	bl.JobStart:          bl.KindJobStart,
	bl.JobRespondToInput: bl.KindJobRespondToInput,
	bl.JobCancel:         bl.KindJobCancel,
	bl.JobTerminate:      bl.KindJobTerminate,
	bl.JobResume:         bl.KindJobResume,
}

// schedulerLoop claims due timer records and publishes their JobResume.
// Claims are transactional in the RegistryStore, so concurrent broker
// instances never double-fire.
func (b *Broker) schedulerLoop() {
	defer b.wg.Done()
	ticker := time.NewTicker(schedulerInterval)
	defer ticker.Stop()
	for {
		select {
		case <-b.done:
			return
		case <-ticker.C:
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			ids, err := b.cfg.Registry.DueTimers(ctx, time.Now())
			if err == nil {
				for _, id := range ids {
					rec, err := b.store.GetInstance(ctx, id)
					if err != nil || rec == nil || rec.Finished {
						continue
					}
					_ = b.publishJob(ctx, rec.Key, bl.Job{
						Kind:   bl.JobResume,
						Key:    rec.Key,
						Resume: &bl.ResumeJob{InstanceID: id},
					}, rec.CorrelationKey)
				}
			}
			cancel()
		}
	}
}

// ===== Worker side: lifecycle reports and instance topic =====

// ReportRunning publishes the Running lifecycle event. Does not settle the
// in-flight job.
func (b *Broker) ReportRunning(ctx context.Context, instanceID string) error {
	if err := b.guard(ctx); err != nil {
		return err
	}
	return b.publishInstanceEvent(ctx, instanceID, bl.InstanceEvent{
		Kind:      bl.InstanceEventLifecycle,
		Lifecycle: &bl.LifecycleChange{Phase: bl.ProcessStatusRunning},
	}, true)
}

// ReportSuspended publishes the Suspended lifecycle event, settles the
// in-flight job, and — given a resumeAt — writes the durable timer record
// the scheduler loop turns into a JobResume.
func (b *Broker) ReportSuspended(ctx context.Context, instanceID string, resumeAt *time.Time) error {
	if err := b.guard(ctx); err != nil {
		return err
	}
	if err := b.publishInstanceEvent(ctx, instanceID, bl.InstanceEvent{
		Kind:      bl.InstanceEventLifecycle,
		Lifecycle: &bl.LifecycleChange{Phase: bl.ProcessStatusSuspended},
	}, true); err != nil {
		return err
	}
	b.settle(instanceID)
	if resumeAt == nil {
		return nil
	}
	return b.cfg.Registry.PutTimer(ctx, instanceID, *resumeAt)
}

// ReportCompleted publishes the Completed lifecycle event and the Result
// event, settles the job, and lets subscriber channels close.
func (b *Broker) ReportCompleted(ctx context.Context, instanceID string, result bl.EvaluationResult) error {
	if err := b.guard(ctx); err != nil {
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
	if err := b.guard(ctx); err != nil {
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
	if err := b.guard(ctx); err != nil {
		return err
	}
	return b.finish(ctx, instanceID, bl.ProcessStatusCancelled, nil)
}

// PostError publishes a non-terminal error event.
func (b *Broker) PostError(ctx context.Context, instanceID string, instErr bl.InstanceError) error {
	if err := b.guard(ctx); err != nil {
		return err
	}
	return b.publishInstanceEvent(ctx, instanceID, bl.InstanceEvent{
		Kind:  bl.InstanceEventError,
		Error: &instErr,
	}, false)
}

// PostInputRequest publishes a RequestInputTask's request for input.
func (b *Broker) PostInputRequest(ctx context.Context, instanceID string, req bl.InputRequest) error {
	if err := b.guard(ctx); err != nil {
		return err
	}
	return b.publishInstanceEvent(ctx, instanceID, bl.InstanceEvent{
		Kind:         bl.InstanceEventInputRequest,
		InputRequest: &req,
	}, false)
}

// PostNodeCompleted publishes a node-completion event.
func (b *Broker) PostNodeCompleted(ctx context.Context, instanceID string, nc bl.NodeCompleted) error {
	if err := b.guard(ctx); err != nil {
		return err
	}
	return b.publishInstanceEvent(ctx, instanceID, bl.InstanceEvent{
		Kind:          bl.InstanceEventNodeCompleted,
		NodeCompleted: &nc,
	}, false)
}

// finish drives the instance terminal: settle the job, drop any pending
// timer, persist the terminal record, then publish the terminal lifecycle
// event and the extra Result/Error event in order.
func (b *Broker) finish(ctx context.Context, instanceID string, phase bl.ProcessStatus, extra *bl.InstanceEvent) error {
	b.settle(instanceID)
	_ = b.cfg.Registry.DeleteTimer(ctx, instanceID)
	rec, err := b.store.GetInstance(ctx, instanceID)
	if err != nil {
		return err
	}
	if rec != nil && rec.Finished {
		return nil // duplicate terminal report: no-op
	}
	var corr *string
	if rec != nil {
		corr = rec.CorrelationKey
	}
	lifecycle := bl.InstanceEvent{
		InstanceID:     instanceID,
		CorrelationKey: corr,
		Kind:           bl.InstanceEventLifecycle,
		OccurredAt:     time.Now(),
		Lifecycle:      &bl.LifecycleChange{Phase: phase},
	}
	latestStored, latestData, err := b.encodeEvent(lifecycle)
	if err != nil {
		return err
	}
	var terminalStored *StoredEvent
	var terminalData []byte
	if extra != nil {
		e := *extra
		e.InstanceID = instanceID
		e.CorrelationKey = corr
		e.OccurredAt = time.Now()
		if terminalStored, terminalData, err = b.encodeEvent(e); err != nil {
			return err
		}
	}
	if err := b.store.UpdateInstance(ctx, instanceID, latestStored, terminalStored, true); err != nil {
		return err
	}
	if err := b.publishEncodedEvent(ctx, instanceID, latestStored.EventID, latestData); err != nil {
		return err
	}
	if terminalStored != nil {
		return b.publishEncodedEvent(ctx, instanceID, terminalStored.EventID, terminalData)
	}
	return nil
}

// publishInstanceEvent stamps, encodes, optionally records (for lifecycle
// events), and publishes one instance event.
func (b *Broker) publishInstanceEvent(ctx context.Context, instanceID string, evt bl.InstanceEvent, isLifecycle bool) error {
	rec, err := b.store.GetInstance(ctx, instanceID)
	if err != nil {
		return err
	}
	rec = b.liveRecord(rec)
	evt.InstanceID = instanceID
	if evt.CorrelationKey == nil && rec != nil {
		evt.CorrelationKey = rec.CorrelationKey
	}
	evt.OccurredAt = time.Now()
	stored, data, err := b.encodeEvent(evt)
	if err != nil {
		return err
	}
	if isLifecycle {
		if err := b.store.UpdateInstance(ctx, instanceID, stored, nil, false); err != nil {
			return err
		}
	}
	return b.publishEncodedEvent(ctx, instanceID, stored.EventID, data)
}

// encodeEvent envelope-encodes an instance event under a fresh eventID.
func (b *Broker) encodeEvent(evt bl.InstanceEvent) (*StoredEvent, []byte, error) {
	data, err := bl.EncodeEnvelope(bl.KindInstanceEvent, evt.InstanceID, evt.CorrelationKey, evt, b.cfg.Cipher)
	if err != nil {
		return nil, nil, err
	}
	return &StoredEvent{EventID: randomID(), Data: data}, data, nil
}

// publishEncodedEvent publishes one pre-encoded instance event, ordered per
// instance via the ordering key.
func (b *Broker) publishEncodedEvent(ctx context.Context, instanceID, eventID string, data []byte) error {
	res := b.events.Publish(ctx, &pubsub.Message{
		Data:        data,
		OrderingKey: instanceID,
		Attributes: map[string]string{
			attrInstanceID: instanceID,
			attrEventID:    eventID,
			attrKind:       bl.KindInstanceEvent,
		},
	})
	if _, err := res.Get(ctx); err != nil {
		b.events.ResumePublish(instanceID) // ordering keys pause on error
		return fmt.Errorf("gpubsub: publish instance event: %w", err)
	}
	return nil
}

// ===== SubscribeToInstance =====

// SubscribeToInstance creates a per-subscriber filtered, ordered
// subscription on the events topic, replays the instance's latest lifecycle
// (and terminal) event from its last-event record, then follows the live
// subscription. The channel closes after the terminal event delivers or when
// ctx is cancelled; the subscription is deleted on close.
func (b *Broker) SubscribeToInstance(ctx context.Context, instanceID string) (<-chan bl.InstanceEvent, error) {
	if err := b.guard(ctx); err != nil {
		return nil, err
	}
	name := fmt.Sprintf("%s-e-%s-%s", b.cfg.EntityPrefix, instanceID, randomID())
	ordered := pubsub.SubscriptionConfig{
		Topic:                 b.events,
		AckDeadline:           minAckDeadline,
		Filter:                fmt.Sprintf("attributes.%s = %q", attrInstanceID, instanceID),
		EnableMessageOrdering: true,
	}
	unfiltered := ordered
	unfiltered.Filter = ""
	unordered := unfiltered
	unordered.EnableMessageOrdering = false
	if err := b.createSubscription(ctx, name, ordered, unfiltered, unordered); err != nil {
		return nil, fmt.Errorf("gpubsub: create event subscription: %w", err)
	}
	sub := b.client.Subscription(name)
	deleteSub := func() {
		dctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		_ = sub.Delete(dctx)
	}

	// The record is read only after the subscription exists, so every event
	// is covered: published before creation -> in the record; after -> live.
	rec, err := b.store.GetInstance(ctx, instanceID)
	if err != nil {
		deleteSub()
		return nil, err
	}
	rec = b.liveRecord(rec)

	s := &instanceSub{instanceID: instanceID, ch: make(chan bl.InstanceEvent, eventBufferSize)}
	replayed := map[string]bool{}
	if rec == nil {
		s.ch <- bl.InstanceEvent{
			InstanceID: instanceID,
			Kind:       bl.InstanceEventError,
			OccurredAt: time.Now(),
			Error:      &bl.InstanceError{Code: bl.ErrCodeInstanceNotFound, Message: "no events visible for this instance"},
		}
	} else {
		for _, se := range []*StoredEvent{rec.Latest, rec.Terminal} {
			if se == nil {
				continue
			}
			if evt, err := b.decodeEvent(se.Data); err == nil {
				s.ch <- evt
			}
			replayed[se.EventID] = true
		}
		if rec.Finished {
			close(s.ch)
			go deleteSub()
			return s.ch, nil
		}
	}

	rctx, cancel := context.WithCancel(context.Background())
	s.cancel = cancel
	b.wg.Add(1)
	go func() {
		defer b.wg.Done()
		select {
		case <-ctx.Done():
		case <-b.done:
		case <-rctx.Done():
		}
		cancel()
	}()
	b.wg.Add(1)
	go func() {
		defer b.wg.Done()
		sub.ReceiveSettings.NumGoroutines = 1
		sub.ReceiveSettings.MaxOutstandingMessages = 32
		_ = sub.Receive(rctx, func(_ context.Context, msg *pubsub.Message) {
			defer msg.Ack()                                   // event fan-out is fire-and-forget; never redeliver
			if msg.Attributes[attrInstanceID] != instanceID { // unenforced emulator filter
				return
			}
			if replayed[msg.Attributes[attrEventID]] { // duplicate of a replayed event
				return
			}
			evt, err := b.decodeEvent(msg.Data)
			if err != nil {
				return
			}
			s.deliver(evt)
		})
		s.close()
		deleteSub()
	}()
	return s.ch, nil
}

func (b *Broker) decodeEvent(data []byte) (bl.InstanceEvent, error) {
	env, err := bl.DecodeEnvelope(data, b.cfg.Cipher)
	if err != nil {
		return bl.InstanceEvent{}, err
	}
	var evt bl.InstanceEvent
	if err := env.DecodePayload(&evt); err != nil {
		return bl.InstanceEvent{}, err
	}
	return evt, nil
}

// instanceSub is one SubscribeToInstance subscriber: a bounded channel with
// drop-on-overflow backpressure and terminal-sequence close tracking.
type instanceSub struct {
	instanceID string
	ch         chan bl.InstanceEvent
	cancel     context.CancelFunc

	mu       sync.Mutex
	closed   bool
	dropped  bool
	awaiting bl.InstanceEventKind // extra event expected after a terminal lifecycle
	terminal bool                 // a terminal lifecycle event has been delivered
}

// deliver forwards one live event and closes the channel once the terminal
// sequence (terminal lifecycle, then Result/Error extra where applicable)
// has been delivered.
func (s *instanceSub) deliver(evt bl.InstanceEvent) {
	s.mu.Lock()
	if s.closed {
		s.mu.Unlock()
		return
	}
	s.send(evt)
	done := false
	if evt.Kind == bl.InstanceEventLifecycle && evt.Lifecycle != nil && evt.Lifecycle.Phase.Finished() {
		s.terminal = true
		switch evt.Lifecycle.Phase {
		case bl.ProcessStatusCompleted:
			s.awaiting = bl.InstanceEventResult
		case bl.ProcessStatusFailed:
			s.awaiting = bl.InstanceEventError
		default: // Cancelled: no extra event follows
			done = true
		}
	} else if s.terminal && evt.Kind == s.awaiting {
		done = true
	}
	if done {
		s.closed = true
		close(s.ch)
	}
	s.mu.Unlock()
	if done {
		s.cancel()
	}
}

// send enqueues with the shared backpressure policy: a full buffer drops the
// event, and recovery is signalled with a synthetic BACKPRESSURE_DROP.
// Callers hold s.mu.
func (s *instanceSub) send(evt bl.InstanceEvent) {
	if s.dropped {
		synthetic := bl.InstanceEvent{
			InstanceID: s.instanceID,
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

func (s *instanceSub) close() {
	s.mu.Lock()
	defer s.mu.Unlock()
	if !s.closed {
		s.closed = true
		close(s.ch)
	}
}
