package azuresb

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"math"
	"strings"
	"sync"
	"time"

	"github.com/Azure/azure-sdk-for-go/sdk/azcore"
	"github.com/Azure/azure-sdk-for-go/sdk/azcore/to"
	"github.com/Azure/azure-sdk-for-go/sdk/messaging/azservicebus"
	"github.com/Azure/azure-sdk-for-go/sdk/messaging/azservicebus/admin"

	bl "github.com/friendly-business-machines/blkit/core"
)

var (
	_ bl.MessageBroker    = (*Broker)(nil)
	_ bl.RegistryStore    = (*TableRegistryStore)(nil)
	_ InstanceRecordStore = (*TableRegistryStore)(nil)
)

// Config configures New.
type Config struct {
	Namespace        string                 // "myns.servicebus.windows.net"; with Credential
	ConnectionString string                 // SAS auth; or set Namespace+Credential
	Credential       azcore.TokenCredential // Azure AD / Managed Identity (recommended)

	// Client optionally supplies an existing Service Bus client so several
	// broker handles share one AMQP connection (links are still per
	// handle). The handle never closes a supplied client — the caller owns
	// it. ConnectionString / Namespace+Credential are then only needed for
	// the admin client (entity creation), and not at all with UseEmulator.
	Client *azservicebus.Client

	// EntityPrefix isolates deployments sharing a namespace. Default "blkit".
	EntityPrefix string

	// Registry is the required RegistryStore side-store (worker registry,
	// timer records, last-event records). TableRegistryStore is the
	// implementation shipped with this module.
	Registry bl.RegistryStore

	// UseEmulator marks a Service Bus emulator endpoint: entity management
	// is skipped (the emulator cannot create entities at runtime — declare
	// them in its config JSON) and PumpSubscription must name a predeclared
	// subscription on the instance-events topic.
	UseEmulator bool

	// Cipher optionally end-to-end encrypts envelope payloads. Default nil.
	Cipher bl.PayloadCipher

	// RegistrationTTL is how long registrations outlive their last
	// heartbeat. Default 90s.
	RegistrationTTL time.Duration

	// InFlightTimeout is how long a delivered job may stay unsettled before
	// Service Bus redelivers it. It maps onto the queue's peek-lock
	// duration, whose Service Bus minimum is 5s: max(InFlightTimeout, 5s)
	// is what actually applies. Default 150s.
	InFlightTimeout time.Duration

	// EventRetention is how long a finished instance's last-event record
	// stays replayable to late subscribers. Default 1h.
	EventRetention time.Duration

	// PumpSubscription optionally names an existing subscription on the
	// instance-events topic for this handle's event pump. When empty a
	// uniquely named subscription is created at construction
	// (AutoDeleteOnIdle cleans it up). Required with UseEmulator.
	PumpSubscription string
}

// Broker implements bl.MessageBroker on Azure Service Bus: one peek-lock
// queue per ProcessKey for jobs, one instance-events topic with one
// subscription per broker handle (client-side fan-out to subscribers), native
// scheduled messages for delayed JobResume delivery, and the RegistryStore
// for everything Service Bus has no primitive for.
type Broker struct {
	cfg      Config
	client   *azservicebus.Client
	admin    *admin.Client // nil in emulator mode
	recStore InstanceRecordStore

	prefix     string
	topicName  string
	subName    string
	ownsSub    bool
	ownsClient bool
	lockDur    time.Duration

	pumpRecv   *azservicebus.Receiver
	pumpCancel context.CancelFunc

	mu        sync.Mutex
	closed    bool
	done      chan struct{}
	senders   map[string]*azservicebus.Sender
	queuesOK  map[string]bool
	receivers []*azservicebus.Receiver
	inFlight  map[string][]*inflightMsg // by instanceID
	insts     map[string]*instInfo      // handle-local cache of instance records
	subs      map[string]map[int]*instSub
	scheduled map[string][]scheduledResume // pending scheduled JobResumes by instanceID
	nextSub   int

	wg sync.WaitGroup
}

const (
	defaultRegistrationTTL = 90 * time.Second
	defaultInFlightTimeout = 150 * time.Second
	defaultEventRetention  = time.Hour
	minLockDuration        = 5 * time.Second // Service Bus minimum peek-lock duration
	eventBufferSize        = 64

	propInstance = "blkitInstance"
	propKind     = "blkitKind"
	propFinal    = "blkitFinal" // marks the last event of a terminal sequence
)

type instInfo struct {
	key      bl.ProcessKey
	corr     *string
	finished bool
}

type inflightMsg struct {
	receiver  *azservicebus.Receiver
	msg       *azservicebus.ReceivedMessage
	stopRenew context.CancelFunc
}

type scheduledResume struct {
	queue string
	seq   int64
}

type instSub struct {
	mu      sync.Mutex
	ch      chan bl.InstanceEvent
	dropped bool
	closed  bool
}

// New connects to the namespace, ensures the instance-events topic and this
// handle's pump subscription exist (skipped with UseEmulator — predeclare
// them), and starts the event pump. Release resources with Close.
func New(cfg Config) (*Broker, error) {
	if cfg.Registry == nil {
		return nil, errors.New("azuresb: Config.Registry is required")
	}
	if cfg.EntityPrefix == "" {
		cfg.EntityPrefix = "blkit"
	}
	if cfg.RegistrationTTL <= 0 {
		cfg.RegistrationTTL = defaultRegistrationTTL
	}
	if cfg.InFlightTimeout <= 0 {
		cfg.InFlightTimeout = defaultInFlightTimeout
	}
	if cfg.EventRetention <= 0 {
		cfg.EventRetention = defaultEventRetention
	}
	if cfg.UseEmulator && cfg.PumpSubscription == "" {
		return nil, errors.New("azuresb: UseEmulator requires PumpSubscription (the emulator cannot create entities at runtime)")
	}

	client := cfg.Client
	ownsClient := client == nil
	var adminClient *admin.Client
	var err error
	switch {
	case cfg.ConnectionString != "":
		if client == nil {
			if client, err = azservicebus.NewClientFromConnectionString(cfg.ConnectionString, nil); err != nil {
				return nil, fmt.Errorf("azuresb: client: %w", err)
			}
		}
		if !cfg.UseEmulator {
			if adminClient, err = admin.NewClientFromConnectionString(cfg.ConnectionString, nil); err != nil {
				return nil, fmt.Errorf("azuresb: admin client: %w", err)
			}
		}
	case cfg.Namespace != "" && cfg.Credential != nil:
		if client == nil {
			if client, err = azservicebus.NewClient(cfg.Namespace, cfg.Credential, nil); err != nil {
				return nil, fmt.Errorf("azuresb: client: %w", err)
			}
		}
		if !cfg.UseEmulator {
			if adminClient, err = admin.NewClient(cfg.Namespace, cfg.Credential, nil); err != nil {
				return nil, fmt.Errorf("azuresb: admin client: %w", err)
			}
		}
	case client != nil && cfg.UseEmulator:
		// A supplied client is all the emulator route needs (no admin ops).
	default:
		return nil, errors.New("azuresb: Config needs ConnectionString, Namespace+Credential, or Client with UseEmulator")
	}

	recStore, ok := cfg.Registry.(InstanceRecordStore)
	if !ok {
		recStore = newMemRecordStore()
	}
	b := &Broker{
		cfg:        cfg,
		client:     client,
		ownsClient: ownsClient,
		admin:      adminClient,
		recStore:   recStore,
		prefix:     cfg.EntityPrefix,
		topicName:  InstanceEventsTopicName(cfg.EntityPrefix),
		lockDur:    max(cfg.InFlightTimeout, minLockDuration),
		done:       make(chan struct{}),
		senders:    map[string]*azservicebus.Sender{},
		queuesOK:   map[string]bool{},
		inFlight:   map[string][]*inflightMsg{},
		insts:      map[string]*instInfo{},
		subs:       map[string]map[int]*instSub{},
		scheduled:  map[string][]scheduledResume{},
	}

	setupCtx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()
	closeOwned := func() {
		if ownsClient {
			_ = client.Close(setupCtx)
		}
	}
	if err := b.setupPumpSubscription(setupCtx); err != nil {
		closeOwned()
		return nil, err
	}
	b.pumpRecv, err = client.NewReceiverForSubscription(b.topicName, b.subName, &azservicebus.ReceiverOptions{
		ReceiveMode: azservicebus.ReceiveModeReceiveAndDelete,
	})
	if err != nil {
		closeOwned()
		return nil, fmt.Errorf("azuresb: pump receiver: %w", err)
	}
	var pumpCtx context.Context
	pumpCtx, b.pumpCancel = context.WithCancel(context.Background())
	b.wg.Add(1)
	go b.pump(pumpCtx)
	return b, nil
}

func (b *Broker) setupPumpSubscription(ctx context.Context) error {
	if b.cfg.PumpSubscription != "" {
		b.subName = b.cfg.PumpSubscription
		return nil
	}
	if _, err := b.admin.CreateTopic(ctx, b.topicName, &admin.CreateTopicOptions{
		Properties: &admin.TopicProperties{
			DefaultMessageTimeToLive: to.Ptr(iso8601Duration(b.cfg.EventRetention)),
		},
	}); err != nil && !isConflict(err) {
		return fmt.Errorf("azuresb: create topic %s: %w", b.topicName, err)
	}
	b.subName = "h-" + randomHex(8)
	b.ownsSub = true
	if _, err := b.admin.CreateSubscription(ctx, b.topicName, b.subName, &admin.CreateSubscriptionOptions{
		Properties: &admin.SubscriptionProperties{
			AutoDeleteOnIdle:         to.Ptr("PT5M"),
			DefaultMessageTimeToLive: to.Ptr(iso8601Duration(b.cfg.EventRetention)),
		},
	}); err != nil && !isConflict(err) {
		return fmt.Errorf("azuresb: create subscription %s: %w", b.subName, err)
	}
	return nil
}

// Close abandons unsettled in-flight messages (making them promptly
// redeliverable), closes every receiver, sender, and subscriber channel, and
// fails subsequent calls with ErrBrokerClosed. It does not close the
// configured RegistryStore — the caller owns it. Idempotent.
func (b *Broker) Close() error {
	b.mu.Lock()
	if b.closed {
		b.mu.Unlock()
		return nil
	}
	b.closed = true
	close(b.done)
	b.pumpCancel()
	var entries []*inflightMsg
	for _, list := range b.inFlight {
		entries = append(entries, list...)
	}
	b.inFlight = map[string][]*inflightMsg{}
	receivers := b.receivers
	b.receivers = nil
	senders := make([]*azservicebus.Sender, 0, len(b.senders))
	for _, s := range b.senders {
		senders = append(senders, s)
	}
	b.senders = map[string]*azservicebus.Sender{}
	var allSubs []*instSub
	for _, m := range b.subs {
		for _, s := range m {
			allSubs = append(allSubs, s)
		}
	}
	b.subs = map[string]map[int]*instSub{}
	b.mu.Unlock()

	// Each teardown step gets its own bounded context so one wedged call
	// cannot starve the rest — client.Close in particular must run, or the
	// AMQP connection leaks.
	step := func(fn func(ctx context.Context)) {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		fn(ctx)
	}
	for _, e := range entries {
		e.stopRenew()
		step(func(ctx context.Context) { _ = e.receiver.AbandonMessage(ctx, e.msg, nil) })
	}
	for _, r := range receivers {
		step(func(ctx context.Context) { _ = r.Close(ctx) })
	}
	step(func(ctx context.Context) { _ = b.pumpRecv.Close(ctx) })
	for _, s := range senders {
		step(func(ctx context.Context) { _ = s.Close(ctx) })
	}
	if b.ownsSub && b.admin != nil {
		step(func(ctx context.Context) { _, _ = b.admin.DeleteSubscription(ctx, b.topicName, b.subName, nil) })
	}
	if b.ownsClient {
		step(func(ctx context.Context) { _ = b.client.Close(ctx) })
	}
	for _, s := range allSubs {
		s.close()
	}
	b.wg.Wait()
	return nil
}

// ===== entity naming =====

// InstanceEventsTopicName returns the instance-events topic name for a
// prefix. Exported so deployments (and the emulator's config JSON) can
// predeclare the entity.
func InstanceEventsTopicName(prefix string) string {
	return prefix + "-inst-events"
}

// JobsQueueName returns the jobs queue name for a process key. Process keys
// contain characters Service Bus entity names forbid, so each component is
// escaped injectively ([A-Za-z0-9-] kept, anything else "_"+2-digit hex).
// Exported so deployments (and the emulator's config JSON) can predeclare
// the entities.
func JobsQueueName(prefix string, key bl.ProcessKey) string {
	return prefix + "-jobs." +
		sanitizeEntityComponent(key.Namespace) + "." +
		sanitizeEntityComponent(key.ProcessID) + "." +
		sanitizeEntityComponent(key.Version)
}

func sanitizeEntityComponent(s string) string {
	var sb strings.Builder
	for i := 0; i < len(s); i++ {
		c := s[i]
		switch {
		case c >= 'a' && c <= 'z', c >= 'A' && c <= 'Z', c >= '0' && c <= '9', c == '-':
			sb.WriteByte(c)
		default:
			fmt.Fprintf(&sb, "_%02x", c)
		}
	}
	return sb.String()
}

func iso8601Duration(d time.Duration) string {
	return fmt.Sprintf("PT%dS", int64(math.Ceil(d.Seconds())))
}

func randomHex(n int) string {
	buf := make([]byte, n)
	if _, err := rand.Read(buf); err != nil {
		panic(err) // crypto/rand never fails on supported platforms
	}
	return hex.EncodeToString(buf)
}

// ===== producer side =====

// Submit resolves the process from the RegistryStore, validates the input
// against the registration's InputContract, creates the instance's
// last-event record (latest lifecycle: Pending), enqueues the JobStart, and
// publishes the Pending event. The RegistryStore snapshot is authoritative
// and synchronous, so there is no cold-start wait: an unadvertised process
// is ErrUnknownProcess immediately.
func (b *Broker) Submit(ctx context.Context, req bl.StartRequest) (string, error) {
	if err := b.usable(ctx); err != nil {
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
	evt := b.stampEvent(instanceID, req.CorrelationKey, bl.InstanceEvent{
		Kind:      bl.InstanceEventLifecycle,
		Lifecycle: &bl.LifecycleChange{Phase: bl.ProcessStatusPending},
	})
	env, err := bl.EncodeEnvelope(bl.KindInstanceEvent, instanceID, req.CorrelationKey, evt, b.cfg.Cipher)
	if err != nil {
		return "", err
	}
	if err := b.recStore.PutInstanceRecord(ctx, instanceID, InstanceRecord{
		Key:            req.Key(),
		CorrelationKey: req.CorrelationKey,
		Latest:         env,
	}); err != nil {
		return "", err
	}
	b.mu.Lock()
	b.insts[instanceID] = &instInfo{key: req.Key(), corr: req.CorrelationKey}
	b.mu.Unlock()
	if _, err := b.enqueueJob(ctx, bl.Job{
		Kind: bl.JobStart,
		Key:  req.Key(),
		Start: &bl.StartJob{
			InstanceID:     instanceID,
			StartID:        req.StartID,
			Input:          req.Input,
			CorrelationKey: req.CorrelationKey,
		},
	}, nil); err != nil {
		return "", err
	}
	if err := b.sendEvent(ctx, instanceID, env, false); err != nil {
		return "", err
	}
	return instanceID, nil
}

// Cancel always takes the JobCancel route: Service Bus cannot selectively
// remove a queued message, so queue-side removal is not supported and
// AllowExternalCancel is required even for still-queued instances.
func (b *Broker) Cancel(ctx context.Context, req bl.CancelRequest) error {
	if err := b.usable(ctx); err != nil {
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
	_, err = b.enqueueJob(ctx, bl.Job{
		Kind:   bl.JobCancel,
		Key:    req.Key(),
		Cancel: &bl.CancelJob{InstanceID: req.InstanceID, Reason: req.Reason},
	}, nil)
	return err
}

// Terminate publishes a JobTerminate. Requires AllowExternalTerminate.
func (b *Broker) Terminate(ctx context.Context, req bl.TerminateRequest) error {
	if err := b.usable(ctx); err != nil {
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
	_, err = b.enqueueJob(ctx, bl.Job{
		Kind:      bl.JobTerminate,
		Key:       req.Key(),
		Terminate: &bl.TerminateJob{InstanceID: req.InstanceID, Reason: req.Reason},
	}, nil)
	return err
}

// RespondToInputRequest routes the response to the instance's process-key
// queue, resolving the key from the last-event record. Unknown or finished
// instances surface asynchronously on the instance topic, per the error
// model.
func (b *Broker) RespondToInputRequest(ctx context.Context, instanceID, requestID string, payload map[string]any) error {
	if err := b.usable(ctx); err != nil {
		return err
	}
	info, err := b.instanceInfo(ctx, instanceID)
	if err != nil {
		return err
	}
	if info == nil || info.finished {
		code := bl.ErrCodeInstanceNotFound
		if info != nil {
			code = bl.ErrCodeAlreadyFinished
		}
		return b.postErrorEvent(ctx, instanceID, bl.InstanceError{
			Code:    code,
			Message: "respond-to-input for an instance the broker does not hold",
		})
	}
	_, err = b.enqueueJob(ctx, bl.Job{
		Kind:           bl.JobRespondToInput,
		Key:            info.key,
		RespondToInput: &bl.RespondToInputJob{InstanceID: instanceID, RequestID: requestID, Payload: payload},
	}, nil)
	return err
}

// SubscribeToInstance delivers the instance's events. Late subscribers first
// get latest-event replay from the last-event record (Service Bus
// subscriptions cannot deliver messages published before this handle's pump
// subscription existed); live events then follow from the pump, with
// duplicates possible across the replay/live boundary. An instance with no
// record gets INSTANCE_NOT_FOUND as its first event.
func (b *Broker) SubscribeToInstance(ctx context.Context, instanceID string) (<-chan bl.InstanceEvent, error) {
	if err := b.usable(ctx); err != nil {
		return nil, err
	}
	rec, err := b.recStore.GetInstanceRecord(ctx, instanceID)
	if err != nil {
		return nil, err
	}
	sub := &instSub{ch: make(chan bl.InstanceEvent, eventBufferSize)}
	if rec == nil {
		sub.send(bl.InstanceEvent{
			InstanceID: instanceID,
			Kind:       bl.InstanceEventError,
			OccurredAt: time.Now(),
			Error:      &bl.InstanceError{Code: bl.ErrCodeInstanceNotFound, Message: "no events visible for this instance"},
		}, instanceID)
		b.registerSub(ctx, instanceID, sub)
		return sub.ch, nil
	}
	if rec.Finished {
		b.replayRecord(sub, instanceID, rec)
		sub.close()
		return sub.ch, nil
	}
	b.cacheInfo(instanceID, rec)
	b.registerSub(ctx, instanceID, sub)
	b.replayRecord(sub, instanceID, rec)
	// The instance may have finished between the record fetch and the sub
	// registration; the pump would have closed subscribers before we
	// registered. Re-check the handle cache and finish the stream from a
	// fresh record if so.
	b.mu.Lock()
	finished := b.insts[instanceID] != nil && b.insts[instanceID].finished
	b.mu.Unlock()
	if finished {
		if rec, err := b.recStore.GetInstanceRecord(ctx, instanceID); err == nil && rec != nil {
			b.unregisterSub(instanceID, sub)
			b.replayRecord(sub, instanceID, rec)
			sub.close()
		}
	}
	return sub.ch, nil
}

// replayRecord delivers the record's latest lifecycle and terminal events.
func (b *Broker) replayRecord(sub *instSub, instanceID string, rec *InstanceRecord) {
	for _, env := range [][]byte{rec.Latest, rec.Terminal} {
		if len(env) == 0 {
			continue
		}
		if evt, err := b.decodeEvent(env); err == nil {
			sub.send(evt, instanceID)
		}
	}
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

func (b *Broker) registerSub(ctx context.Context, instanceID string, sub *instSub) {
	b.mu.Lock()
	if b.closed {
		b.mu.Unlock()
		sub.close()
		return
	}
	id := b.nextSub
	b.nextSub++
	if b.subs[instanceID] == nil {
		b.subs[instanceID] = map[int]*instSub{}
	}
	b.subs[instanceID][id] = sub
	b.mu.Unlock()
	b.wg.Add(1)
	go func() {
		defer b.wg.Done()
		select {
		case <-ctx.Done():
			b.unregisterSub(instanceID, sub)
			sub.close()
		case <-b.done: // Close closes every subscriber
		}
	}()
}

func (b *Broker) unregisterSub(instanceID string, sub *instSub) {
	b.mu.Lock()
	defer b.mu.Unlock()
	for id, s := range b.subs[instanceID] {
		if s == sub {
			delete(b.subs[instanceID], id)
			break
		}
	}
	if len(b.subs[instanceID]) == 0 {
		delete(b.subs, instanceID)
	}
}

// SubscribeToProcessRegistry delivers the RegistryStore snapshot, the
// sentinel, then the store's Watch feed until ctx is cancelled.
func (b *Broker) SubscribeToProcessRegistry(ctx context.Context) (<-chan bl.RegistryUpdate, error) {
	if err := b.usable(ctx); err != nil {
		return nil, err
	}
	// Watch first, then snapshot: a registration landing in between shows
	// up in both (duplicate Added), never in neither.
	watch, err := b.cfg.Registry.Watch(ctx)
	if err != nil {
		return nil, err
	}
	snap, err := b.cfg.Registry.Snapshot(ctx)
	if err != nil {
		return nil, err
	}
	out := make(chan bl.RegistryUpdate)
	b.wg.Add(1)
	go func() {
		defer b.wg.Done()
		defer close(out)
		send := func(u bl.RegistryUpdate) bool {
			select {
			case out <- u:
				return true
			case <-ctx.Done():
				return false
			case <-b.done:
				return false
			}
		}
		for i := range snap {
			if !send(bl.RegistryUpdate{Kind: bl.RegistryUpdateSnapshot, Registration: &snap[i]}) {
				return
			}
		}
		if !send(bl.RegistryUpdate{Kind: bl.RegistryUpdateSnapshotComplete}) {
			return
		}
		for {
			select {
			case u, ok := <-watch:
				if !ok {
					return
				}
				if !send(u) {
					return
				}
			case <-ctx.Done():
				return
			case <-b.done:
				return
			}
		}
	}()
	return out, nil
}

// ===== worker side: registration =====

// RegisterProcesses stamps WorkerID / RegisteredAt / LastHeartbeat and
// replaces the worker's registration set in the RegistryStore (which
// preserves RegisteredAt per key across re-registration).
func (b *Broker) RegisterProcesses(ctx context.Context, workerID string, regs []bl.ProcessRegistration) error {
	if err := b.usable(ctx); err != nil {
		return err
	}
	now := time.Now()
	stamped := make([]bl.ProcessRegistration, len(regs))
	for i, r := range regs {
		r.WorkerID = workerID
		if r.RegisteredAt.IsZero() {
			r.RegisteredAt = now
		}
		r.LastHeartbeat = now
		stamped[i] = r
	}
	return b.cfg.Registry.PutRegistrations(ctx, workerID, stamped, b.cfg.RegistrationTTL)
}

// Heartbeat refreshes the worker's registration TTL.
func (b *Broker) Heartbeat(ctx context.Context, workerID string) error {
	if err := b.usable(ctx); err != nil {
		return err
	}
	return b.cfg.Registry.Touch(ctx, workerID, b.cfg.RegistrationTTL)
}

// Unregister removes the worker's registrations immediately.
func (b *Broker) Unregister(ctx context.Context, workerID string) error {
	if err := b.usable(ctx); err != nil {
		return err
	}
	return b.cfg.Registry.Delete(ctx, workerID)
}

// liveRegistration returns one registration for the key from the store's
// snapshot (any worker), or nil.
func (b *Broker) liveRegistration(ctx context.Context, key bl.ProcessKey) (*bl.ProcessRegistration, error) {
	snap, err := b.cfg.Registry.Snapshot(ctx)
	if err != nil {
		return nil, err
	}
	for i := range snap {
		if snap[i].Key() == key {
			return &snap[i], nil
		}
	}
	return nil, nil
}

// ===== worker side: job queue =====

// FetchJobs receives from each key's queue in peek-lock mode. A delivered
// job stays locked — with a renewal goroutine keeping the lock alive — until
// a terminal Report* verb or ReportSuspended completes the message; a worker
// whose fetch context dies stops renewing, and Service Bus redelivers after
// the lock (max(InFlightTimeout, 5s)) expires.
func (b *Broker) FetchJobs(ctx context.Context, keys []bl.ProcessKey) (<-chan bl.Job, error) {
	if err := b.usable(ctx); err != nil {
		return nil, err
	}
	out := make(chan bl.Job)
	var wg sync.WaitGroup
	for _, key := range keys {
		queue := JobsQueueName(b.prefix, key)
		if err := b.ensureQueue(ctx, queue); err != nil {
			return nil, err
		}
		receiver, err := b.client.NewReceiverForQueue(queue, nil) // peek-lock
		if err != nil {
			return nil, fmt.Errorf("azuresb: receiver for %s: %w", queue, err)
		}
		b.mu.Lock()
		if b.closed {
			b.mu.Unlock()
			return nil, bl.ErrBrokerClosed
		}
		b.receivers = append(b.receivers, receiver)
		b.mu.Unlock()
		wg.Add(1)
		b.wg.Add(1)
		go b.receiveJobs(ctx, receiver, out, &wg)
	}
	b.wg.Add(1)
	go func() {
		defer b.wg.Done()
		wg.Wait()
		if len(keys) == 0 { // idle wait: yield nothing until ctx cancels
			select {
			case <-ctx.Done():
			case <-b.done:
			}
		}
		close(out)
	}()
	return out, nil
}

func (b *Broker) receiveJobs(ctx context.Context, receiver *azservicebus.Receiver, out chan<- bl.Job, wg *sync.WaitGroup) {
	defer b.wg.Done()
	defer wg.Done()
	for {
		if ctx.Err() != nil || b.isClosed() {
			return
		}
		msgs, err := receiver.ReceiveMessages(ctx, 1, nil)
		if err != nil {
			if ctx.Err() != nil || b.isClosed() {
				return
			}
			select {
			case <-time.After(200 * time.Millisecond):
				continue
			case <-ctx.Done():
				return
			case <-b.done:
				return
			}
		}
		for _, m := range msgs {
			var job bl.Job
			env, err := bl.DecodeEnvelope(m.Body, b.cfg.Cipher)
			if err == nil {
				err = env.DecodePayload(&job)
			}
			if err != nil {
				// Poison message: settle it out of the queue.
				_ = receiver.CompleteMessage(ctx, m, nil)
				continue
			}
			entry := &inflightMsg{receiver: receiver, msg: m}
			b.trackInFlight(ctx, job.InstanceID(), entry)
			select {
			case out <- job:
			case <-ctx.Done():
				b.untrack(job.InstanceID(), entry)
				entry.stopRenew()
				abandonCtx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
				_ = receiver.AbandonMessage(abandonCtx, m, nil)
				cancel()
				return
			case <-b.done:
				return
			}
		}
	}
}

// trackInFlight indexes the message by instance and starts its lock-renewal
// goroutine. Renewal stops when the message settles, the fetch context dies
// (letting the lock expire so Service Bus redelivers), or the broker closes.
func (b *Broker) trackInFlight(fetchCtx context.Context, instanceID string, entry *inflightMsg) {
	renewCtx, cancel := context.WithCancel(fetchCtx)
	entry.stopRenew = cancel
	b.mu.Lock()
	b.inFlight[instanceID] = append(b.inFlight[instanceID], entry)
	b.mu.Unlock()
	interval := max(b.lockDur/2, 500*time.Millisecond)
	b.wg.Add(1)
	go func() {
		defer b.wg.Done()
		for {
			select {
			case <-renewCtx.Done():
				return
			case <-b.done:
				return
			case <-time.After(interval):
				if err := entry.receiver.RenewMessageLock(renewCtx, entry.msg, nil); err != nil {
					return
				}
			}
		}
	}()
}

func (b *Broker) untrack(instanceID string, entry *inflightMsg) {
	b.mu.Lock()
	defer b.mu.Unlock()
	list := b.inFlight[instanceID]
	for i, e := range list {
		if e == entry {
			b.inFlight[instanceID] = append(list[:i], list[i+1:]...)
			break
		}
	}
	if len(b.inFlight[instanceID]) == 0 {
		delete(b.inFlight, instanceID)
	}
}

// settleInstance completes every in-flight message for the instance.
// Settlement failures (an already-expired lock) are ignored: the message
// redelivers and at-least-once semantics absorb the duplicate.
func (b *Broker) settleInstance(instanceID string) {
	b.mu.Lock()
	entries := b.inFlight[instanceID]
	delete(b.inFlight, instanceID)
	b.mu.Unlock()
	for _, e := range entries {
		e.stopRenew()
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		_ = e.receiver.CompleteMessage(ctx, e.msg, nil)
		cancel()
	}
}

// cancelScheduledResumes best-effort cancels the instance's scheduled
// JobResume messages (already-enqueued ones cannot be cancelled; errors are
// ignored).
func (b *Broker) cancelScheduledResumes(ctx context.Context, instanceID string) {
	b.mu.Lock()
	entries := b.scheduled[instanceID]
	delete(b.scheduled, instanceID)
	b.mu.Unlock()
	for _, e := range entries {
		if sender, err := b.getSender(e.queue); err == nil {
			_ = sender.CancelScheduledMessages(ctx, []int64{e.seq}, nil)
		}
	}
}

// enqueueJob envelope-encodes the job and sends it to its key's queue,
// scheduled for scheduleAt when non-nil (native delayed delivery).
func (b *Broker) enqueueJob(ctx context.Context, job bl.Job, scheduleAt *time.Time) (int64, error) {
	kind := map[bl.JobKind]string{
		bl.JobStart:          bl.KindJobStart,
		bl.JobRespondToInput: bl.KindJobRespondToInput,
		bl.JobCancel:         bl.KindJobCancel,
		bl.JobTerminate:      bl.KindJobTerminate,
		bl.JobResume:         bl.KindJobResume,
	}[job.Kind]
	data, err := bl.EncodeEnvelope(kind, job.InstanceID(), nil, job, b.cfg.Cipher)
	if err != nil {
		return 0, err
	}
	queue := JobsQueueName(b.prefix, job.Key)
	if err := b.ensureQueue(ctx, queue); err != nil {
		return 0, err
	}
	sender, err := b.getSender(queue)
	if err != nil {
		return 0, err
	}
	msg := &azservicebus.Message{
		Body: data,
		ApplicationProperties: map[string]any{
			propInstance: job.InstanceID(),
			propKind:     kind,
		},
	}
	if scheduleAt != nil {
		seqs, err := sender.ScheduleMessages(ctx, []*azservicebus.Message{msg}, *scheduleAt, nil)
		if err != nil {
			return 0, fmt.Errorf("azuresb: schedule %s on %s: %w", kind, queue, err)
		}
		if len(seqs) > 0 {
			return seqs[0], nil
		}
		return 0, nil
	}
	if err := sender.SendMessage(ctx, msg, nil); err != nil {
		return 0, fmt.Errorf("azuresb: send %s to %s: %w", kind, queue, err)
	}
	return 0, nil
}

// ensureQueue creates the key's queue on demand via the admin client (a
// no-op in emulator mode, where entities are predeclared).
func (b *Broker) ensureQueue(ctx context.Context, queue string) error {
	b.mu.Lock()
	if b.queuesOK[queue] {
		b.mu.Unlock()
		return nil
	}
	b.mu.Unlock()
	if b.admin != nil {
		if _, err := b.admin.CreateQueue(ctx, queue, &admin.CreateQueueOptions{
			Properties: &admin.QueueProperties{
				LockDuration: to.Ptr(iso8601Duration(b.lockDur)),
			},
		}); err != nil && !isConflict(err) {
			return fmt.Errorf("azuresb: create queue %s: %w", queue, err)
		}
	}
	b.mu.Lock()
	b.queuesOK[queue] = true
	b.mu.Unlock()
	return nil
}

func (b *Broker) getSender(entity string) (*azservicebus.Sender, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.closed {
		return nil, bl.ErrBrokerClosed
	}
	if s, ok := b.senders[entity]; ok {
		return s, nil
	}
	s, err := b.client.NewSender(entity, nil)
	if err != nil {
		return nil, fmt.Errorf("azuresb: sender for %s: %w", entity, err)
	}
	b.senders[entity] = s
	return s, nil
}

// ===== worker side: lifecycle reports and instance topic =====

// ReportRunning publishes the Running lifecycle event. Does not settle the
// in-flight job.
func (b *Broker) ReportRunning(ctx context.Context, instanceID string) error {
	if err := b.usable(ctx); err != nil {
		return err
	}
	return b.publishLifecycle(ctx, instanceID, bl.ProcessStatusRunning)
}

// ReportSuspended publishes the Suspended lifecycle event, settles the
// in-flight job, and — given a resumeAt — schedules the JobResume natively
// (ScheduledEnqueueTime on the message).
func (b *Broker) ReportSuspended(ctx context.Context, instanceID string, resumeAt *time.Time) error {
	if err := b.usable(ctx); err != nil {
		return err
	}
	if err := b.publishLifecycle(ctx, instanceID, bl.ProcessStatusSuspended); err != nil {
		return err
	}
	b.settleInstance(instanceID)
	if resumeAt == nil {
		return nil
	}
	info, err := b.instanceInfo(ctx, instanceID)
	if err != nil {
		return err
	}
	if info == nil {
		return nil // no routing record: nothing to schedule against
	}
	seq, err := b.enqueueJob(ctx, bl.Job{
		Kind:   bl.JobResume,
		Key:    info.key,
		Resume: &bl.ResumeJob{InstanceID: instanceID},
	}, resumeAt)
	if err != nil {
		return err
	}
	if seq != 0 {
		b.mu.Lock()
		b.scheduled[instanceID] = append(b.scheduled[instanceID], scheduledResume{queue: JobsQueueName(b.prefix, info.key), seq: seq})
		b.mu.Unlock()
	}
	return nil
}

// ReportCompleted publishes the Completed lifecycle event and the Result
// event, settles the job, and closes subscriber channels after delivery.
func (b *Broker) ReportCompleted(ctx context.Context, instanceID string, result bl.EvaluationResult) error {
	if err := b.usable(ctx); err != nil {
		return err
	}
	return b.finish(ctx, instanceID, bl.ProcessStatusCompleted, &bl.InstanceEvent{
		Kind:   bl.InstanceEventResult,
		Result: &result,
	})
}

// ReportFailed publishes the Failed lifecycle event and the Error event,
// settles the job, and closes subscriber channels after delivery.
func (b *Broker) ReportFailed(ctx context.Context, instanceID string, instErr bl.InstanceError) error {
	if err := b.usable(ctx); err != nil {
		return err
	}
	return b.finish(ctx, instanceID, bl.ProcessStatusFailed, &bl.InstanceEvent{
		Kind:  bl.InstanceEventError,
		Error: &instErr,
	})
}

// ReportCancelled publishes the Cancelled lifecycle event, settles the job,
// and closes subscriber channels after delivery.
func (b *Broker) ReportCancelled(ctx context.Context, instanceID string) error {
	if err := b.usable(ctx); err != nil {
		return err
	}
	return b.finish(ctx, instanceID, bl.ProcessStatusCancelled, nil)
}

// PostError publishes a non-terminal error event.
func (b *Broker) PostError(ctx context.Context, instanceID string, instErr bl.InstanceError) error {
	if err := b.usable(ctx); err != nil {
		return err
	}
	return b.postErrorEvent(ctx, instanceID, instErr)
}

// PostInputRequest publishes a RequestInputTask's request for input.
func (b *Broker) PostInputRequest(ctx context.Context, instanceID string, req bl.InputRequest) error {
	if err := b.usable(ctx); err != nil {
		return err
	}
	return b.publishEvent(ctx, instanceID, bl.InstanceEvent{
		Kind:         bl.InstanceEventInputRequest,
		InputRequest: &req,
	}, false, false)
}

// PostNodeCompleted publishes a node-completion event.
func (b *Broker) PostNodeCompleted(ctx context.Context, instanceID string, nc bl.NodeCompleted) error {
	if err := b.usable(ctx); err != nil {
		return err
	}
	return b.publishEvent(ctx, instanceID, bl.InstanceEvent{
		Kind:          bl.InstanceEventNodeCompleted,
		NodeCompleted: &nc,
	}, false, false)
}

func (b *Broker) postErrorEvent(ctx context.Context, instanceID string, instErr bl.InstanceError) error {
	return b.publishEvent(ctx, instanceID, bl.InstanceEvent{
		Kind:  bl.InstanceEventError,
		Error: &instErr,
	}, false, false)
}

// publishLifecycle publishes a non-terminal lifecycle event and records it
// as the instance's latest.
func (b *Broker) publishLifecycle(ctx context.Context, instanceID string, phase bl.ProcessStatus) error {
	return b.publishEvent(ctx, instanceID, bl.InstanceEvent{
		Kind:      bl.InstanceEventLifecycle,
		Lifecycle: &bl.LifecycleChange{Phase: phase},
	}, false, true)
}

// publishEvent stamps, envelope-encodes, optionally records (updateLatest),
// and sends one instance event to the topic.
func (b *Broker) publishEvent(ctx context.Context, instanceID string, evt bl.InstanceEvent, final, updateLatest bool) error {
	info, err := b.instanceInfo(ctx, instanceID)
	if err != nil {
		return err
	}
	var corr *string
	if info != nil {
		corr = info.corr
	}
	evt = b.stampEvent(instanceID, corr, evt)
	env, err := bl.EncodeEnvelope(bl.KindInstanceEvent, instanceID, corr, evt, b.cfg.Cipher)
	if err != nil {
		return err
	}
	if updateLatest && info != nil {
		rec, err := b.recStore.GetInstanceRecord(ctx, instanceID)
		if err != nil {
			return err
		}
		if rec == nil {
			rec = &InstanceRecord{Key: info.key, CorrelationKey: info.corr}
		}
		rec.Latest = env
		if err := b.recStore.PutInstanceRecord(ctx, instanceID, *rec); err != nil {
			return err
		}
	}
	return b.sendEvent(ctx, instanceID, env, final)
}

// finish drives the instance terminal: settle in-flight jobs, cancel
// scheduled resumes, persist the terminal record (latest lifecycle +
// terminal extra + retention deadline), then publish the terminal lifecycle
// event and the extra Result/Error event — the last one marked final so
// every handle's pump closes the instance's subscriber channels after it
// delivers.
func (b *Broker) finish(ctx context.Context, instanceID string, phase bl.ProcessStatus, extra *bl.InstanceEvent) error {
	b.settleInstance(instanceID)
	b.cancelScheduledResumes(ctx, instanceID)

	info, err := b.instanceInfo(ctx, instanceID)
	if err != nil {
		return err
	}
	var corr *string
	var key bl.ProcessKey
	if info != nil {
		corr = info.corr
		key = info.key
	}
	lifeEvt := b.stampEvent(instanceID, corr, bl.InstanceEvent{
		Kind:      bl.InstanceEventLifecycle,
		Lifecycle: &bl.LifecycleChange{Phase: phase},
	})
	lifeEnv, err := bl.EncodeEnvelope(bl.KindInstanceEvent, instanceID, corr, lifeEvt, b.cfg.Cipher)
	if err != nil {
		return err
	}
	var extraEnv []byte
	if extra != nil {
		extraEvt := b.stampEvent(instanceID, corr, *extra)
		if extraEnv, err = bl.EncodeEnvelope(bl.KindInstanceEvent, instanceID, corr, extraEvt, b.cfg.Cipher); err != nil {
			return err
		}
	}
	if err := b.recStore.PutInstanceRecord(ctx, instanceID, InstanceRecord{
		Key:            key,
		CorrelationKey: corr,
		Latest:         lifeEnv,
		Terminal:       extraEnv,
		Finished:       true,
		ExpiresAt:      time.Now().Add(b.cfg.EventRetention),
	}); err != nil {
		return err
	}
	b.mu.Lock()
	if cached := b.insts[instanceID]; cached != nil {
		cached.finished = true
	} else {
		b.insts[instanceID] = &instInfo{key: key, corr: corr, finished: true}
	}
	b.mu.Unlock()
	if err := b.sendEvent(ctx, instanceID, lifeEnv, extra == nil); err != nil {
		return err
	}
	if extraEnv != nil {
		return b.sendEvent(ctx, instanceID, extraEnv, true)
	}
	return nil
}

func (b *Broker) stampEvent(instanceID string, corr *string, evt bl.InstanceEvent) bl.InstanceEvent {
	evt.InstanceID = instanceID
	evt.CorrelationKey = corr
	evt.OccurredAt = time.Now()
	return evt
}

func (b *Broker) sendEvent(ctx context.Context, instanceID string, envelope []byte, final bool) error {
	sender, err := b.getSender(b.topicName)
	if err != nil {
		return err
	}
	if err := sender.SendMessage(ctx, &azservicebus.Message{
		Body: envelope,
		ApplicationProperties: map[string]any{
			propInstance: instanceID,
			propKind:     bl.KindInstanceEvent,
			propFinal:    final,
		},
	}, nil); err != nil {
		return fmt.Errorf("azuresb: publish instance event: %w", err)
	}
	return nil
}

// instanceInfo resolves the instance's routing info (process key,
// correlation key, finished) from the handle cache, falling back to the
// last-event record. Unknown instances return (nil, nil).
func (b *Broker) instanceInfo(ctx context.Context, instanceID string) (*instInfo, error) {
	b.mu.Lock()
	if info, ok := b.insts[instanceID]; ok {
		b.mu.Unlock()
		return info, nil
	}
	b.mu.Unlock()
	rec, err := b.recStore.GetInstanceRecord(ctx, instanceID)
	if err != nil {
		return nil, err
	}
	if rec == nil {
		return nil, nil
	}
	return b.cacheInfo(instanceID, rec), nil
}

func (b *Broker) cacheInfo(instanceID string, rec *InstanceRecord) *instInfo {
	b.mu.Lock()
	defer b.mu.Unlock()
	if info, ok := b.insts[instanceID]; ok {
		return info
	}
	info := &instInfo{key: rec.Key, corr: rec.CorrelationKey, finished: rec.Finished}
	b.insts[instanceID] = info
	return info
}

// ===== event pump =====

// pump receives every instance event from this handle's topic subscription
// (receive-and-delete) and dispatches in-process to the handle's subscribers
// by instanceID — client-side filtering, so subscriber counts never create
// Service Bus entities.
func (b *Broker) pump(ctx context.Context) {
	defer b.wg.Done()
	for {
		if ctx.Err() != nil || b.isClosed() {
			return
		}
		msgs, err := b.pumpRecv.ReceiveMessages(ctx, 16, nil)
		if err != nil {
			if ctx.Err() != nil || b.isClosed() {
				return
			}
			select {
			case <-time.After(200 * time.Millisecond):
				continue
			case <-ctx.Done():
				return
			}
		}
		for _, m := range msgs {
			b.dispatchEvent(m)
		}
	}
}

func (b *Broker) dispatchEvent(m *azservicebus.ReceivedMessage) {
	instanceID, _ := m.ApplicationProperties[propInstance].(string)
	evt, err := b.decodeEvent(m.Body)
	if err != nil {
		if instanceID == "" {
			return
		}
		// Undecodable payload (e.g. a KeyID this handle cannot resolve):
		// surface a decryption error instead of processing the payload.
		evt = bl.InstanceEvent{
			InstanceID: instanceID,
			Kind:       bl.InstanceEventError,
			OccurredAt: time.Now(),
			Error:      &bl.InstanceError{Code: "DECRYPT_FAILED", Message: err.Error()},
		}
	}
	if instanceID == "" {
		instanceID = evt.InstanceID
	}
	final, _ := m.ApplicationProperties[propFinal].(bool)

	b.mu.Lock()
	if final {
		if info := b.insts[instanceID]; info != nil {
			info.finished = true
		} else {
			b.insts[instanceID] = &instInfo{finished: true}
		}
	}
	var targets []*instSub
	for _, s := range b.subs[instanceID] {
		targets = append(targets, s)
	}
	if final {
		delete(b.subs, instanceID)
	}
	b.mu.Unlock()

	for _, s := range targets {
		s.send(evt, instanceID)
	}
	if final {
		for _, s := range targets {
			s.close()
		}
	}
}

// ===== subscriber channel =====

// send delivers without ever blocking the pump: a full buffer drops the
// event, and a synthetic BACKPRESSURE_DROP is delivered once a slot frees.
func (s *instSub) send(evt bl.InstanceEvent, instanceID string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		return
	}
	if s.dropped {
		synthetic := bl.InstanceEvent{
			InstanceID: instanceID,
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

// ===== misc =====

func (b *Broker) isClosed() bool {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.closed
}

func (b *Broker) usable(ctx context.Context) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if b.isClosed() {
		return bl.ErrBrokerClosed
	}
	return nil
}
