package core

import (
	"context"
	"sync"
	"time"
)

// InMemoryMessageBroker is the built-in single-process MessageBroker backend
// — Go channels and mutex-guarded maps, no external broker
// (specs/message-brokers/in-memory-message-broker.spec.md). Each call to
// NewInMemoryMessageBroker creates its own independent broker; producers and
// workers that must talk to each other share the same broker value. Not
// suitable for multi-process deployments: nothing crosses the process
// boundary and nothing survives a restart.
type InMemoryMessageBroker struct {
	cfg inMemoryBrokerConfig

	mu       sync.Mutex
	closed   bool
	done     chan struct{} // closed by Close; unblocks every waiter
	notify   chan struct{} // closed+replaced whenever pending work changes
	workers  map[string]*inMemWorker
	queues   map[ProcessKey][]*inMemJob
	inFlight map[*inMemJob]struct{}
	byInst   map[string][]*inMemJob // in-flight jobs per instance
	insts    map[string]*inMemInstance
	regSubs  map[int]*registryPump
	timers   map[string]*time.Timer // pending JobResume timers per instance
	nextSub  int
}

type inMemoryBrokerConfig struct {
	eventBufferSize int
	cipher          PayloadCipher
	registrationTTL time.Duration
	inFlightTimeout time.Duration
	eventRetention  time.Duration
}

// InMemoryBrokerOption configures NewInMemoryMessageBroker.
type InMemoryBrokerOption func(*inMemoryBrokerConfig)

// WithEventBufferSize sets the per-subscriber event buffer (default 64). A
// full buffer drops events and synthesizes BACKPRESSURE_DROP on recovery.
func WithEventBufferSize(n int) InMemoryBrokerOption {
	return func(c *inMemoryBrokerConfig) { c.eventBufferSize = n }
}

// WithPayloadCipher sets an end-to-end payload cipher (default nil). Payloads
// still round-trip through the CBOR envelope in-process, so the encryption
// path is exercised by the same code as the external backends.
func WithPayloadCipher(cipher PayloadCipher) InMemoryBrokerOption {
	return func(c *inMemoryBrokerConfig) { c.cipher = cipher }
}

// WithRegistrationTTL sets how long registrations outlive their last
// heartbeat (default 90s — 3× the default 30s heartbeat interval).
func WithRegistrationTTL(d time.Duration) InMemoryBrokerOption {
	return func(c *inMemoryBrokerConfig) { c.registrationTTL = d }
}

// WithInFlightTimeout sets how long a delivered job may stay unsettled before
// it is redelivered (default 150s — 5× the default heartbeat interval).
func WithInFlightTimeout(d time.Duration) InMemoryBrokerOption {
	return func(c *inMemoryBrokerConfig) { c.inFlightTimeout = d }
}

// WithEventRetention sets how long after an instance finishes its latest
// lifecycle and terminal events stay replayable to late subscribers (default
// 1h).
func WithEventRetention(d time.Duration) InMemoryBrokerOption {
	return func(c *inMemoryBrokerConfig) { c.eventRetention = d }
}

// NewInMemoryMessageBroker returns a new, empty in-memory broker. Release its
// goroutines with Close.
func NewInMemoryMessageBroker(opts ...InMemoryBrokerOption) *InMemoryMessageBroker {
	cfg := inMemoryBrokerConfig{
		eventBufferSize: 64,
		registrationTTL: 90 * time.Second,
		inFlightTimeout: 150 * time.Second,
		eventRetention:  time.Hour,
	}
	for _, o := range opts {
		o(&cfg)
	}
	b := &InMemoryMessageBroker{
		cfg:      cfg,
		done:     make(chan struct{}),
		notify:   make(chan struct{}),
		workers:  map[string]*inMemWorker{},
		queues:   map[ProcessKey][]*inMemJob{},
		inFlight: map[*inMemJob]struct{}{},
		byInst:   map[string][]*inMemJob{},
		insts:    map[string]*inMemInstance{},
		regSubs:  map[int]*registryPump{},
		timers:   map[string]*time.Timer{},
	}
	go b.sweep()
	return b
}

type inMemWorker struct {
	regs     []ProcessRegistration
	deadline time.Time
	firstReg map[ProcessKey]time.Time // preserves RegisteredAt across re-registration
}

type inMemJob struct {
	job     Job
	timer   *time.Timer // redelivery timer while in-flight
	settled bool
}

type inMemInstance struct {
	key             ProcessKey
	correlationKey  *string
	latestLifecycle *InstanceEvent
	terminal        *InstanceEvent // Result / Error event replayed after the terminal lifecycle
	finished        bool
	expiresAt       time.Time // zero until finished
	subs            map[int]*inMemSub
}

type inMemSub struct {
	ch      chan InstanceEvent
	dropped bool
	closed  bool
}

// Close stops the sweeper and all timers, closes every subscriber and fetch
// channel, and fails subsequent calls with ErrBrokerClosed. Idempotent.
func (b *InMemoryMessageBroker) Close() error {
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.closed {
		return nil
	}
	b.closed = true
	close(b.done)
	b.signalLocked()
	for _, t := range b.timers {
		t.Stop()
	}
	b.timers = map[string]*time.Timer{}
	for j := range b.inFlight {
		if j.timer != nil {
			j.timer.Stop()
		}
	}
	for _, rec := range b.insts {
		for _, s := range rec.subs {
			s.close()
		}
		rec.subs = map[int]*inMemSub{}
	}
	for _, p := range b.regSubs {
		p.stop()
	}
	b.regSubs = map[int]*registryPump{}
	return nil
}

// signalLocked wakes every fetcher waiting for pending work. Callers hold mu.
func (b *InMemoryMessageBroker) signalLocked() {
	close(b.notify)
	b.notify = make(chan struct{})
}

// sweep expires worker registrations whose TTL lapsed and purges finished
// instance records past their retention window.
func (b *InMemoryMessageBroker) sweep() {
	tick := b.cfg.registrationTTL / 4
	if r := b.cfg.eventRetention / 4; r < tick {
		tick = r
	}
	tick = max(tick, 5*time.Millisecond)
	tick = min(tick, 5*time.Second)
	ticker := time.NewTicker(tick)
	defer ticker.Stop()
	for {
		select {
		case <-b.done:
			return
		case now := <-ticker.C:
			b.mu.Lock()
			for id, w := range b.workers {
				if now.After(w.deadline) {
					delete(b.workers, id)
					for i := range w.regs {
						b.emitRegistryUpdateLocked(RegistryUpdate{Kind: RegistryUpdateHeartbeatLost, Registration: &w.regs[i]})
					}
				}
			}
			for id, rec := range b.insts {
				if rec.finished && now.After(rec.expiresAt) && len(rec.subs) == 0 {
					delete(b.insts, id)
				}
			}
			b.mu.Unlock()
		}
	}
}

// ===== Producer side =====

// Submit validates the request against the live registry, queues a JobStart,
// and publishes the Pending lifecycle event. The in-memory registry is always
// current, so there is no cold-start snapshot wait: an unadvertised process
// is genuinely unknown.
func (b *InMemoryMessageBroker) Submit(ctx context.Context, req StartRequest) (string, error) {
	if err := ctx.Err(); err != nil {
		return "", err
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.closed {
		return "", ErrBrokerClosed
	}
	reg := b.liveRegistrationLocked(req.Key())
	if reg == nil {
		return "", ErrUnknownProcess
	}
	var start *StartEventInfo
	for i := range reg.StartEvents {
		if reg.StartEvents[i].Id == req.StartID {
			start = &reg.StartEvents[i]
			break
		}
	}
	if start == nil {
		return "", ErrUnknownStartID
	}
	if start.InputContract != nil {
		if err := start.InputContract.Validate(req.Input); err != nil {
			return "", err
		}
	}

	instanceID := NewProcessInstanceID()
	b.insts[instanceID] = &inMemInstance{
		key:            req.Key(),
		correlationKey: req.CorrelationKey,
		subs:           map[int]*inMemSub{},
	}
	if err := b.enqueueLocked(Job{
		Kind: JobStart,
		Key:  req.Key(),
		Start: &StartJob{
			InstanceID:     instanceID,
			StartID:        req.StartID,
			Input:          req.Input,
			CorrelationKey: req.CorrelationKey,
		},
	}); err != nil {
		delete(b.insts, instanceID)
		return "", err
	}
	b.publishLocked(instanceID, InstanceEvent{
		Kind:      InstanceEventLifecycle,
		Lifecycle: &LifecycleChange{Phase: ProcessStatusPending},
	})
	return instanceID, nil
}

// Cancel removes a still-queued JobStart exactly (publishing the terminal
// Cancelled event itself), or publishes a JobCancel when a worker already
// holds the instance — which requires AllowExternalCancel.
func (b *InMemoryMessageBroker) Cancel(ctx context.Context, req CancelRequest) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.closed {
		return ErrBrokerClosed
	}
	reg := b.liveRegistrationLocked(req.Key())
	if reg == nil {
		return ErrUnknownProcess
	}
	if b.removeQueuedStartLocked(req.Key(), req.InstanceID) {
		b.finishLocked(req.InstanceID, ProcessStatusCancelled, nil)
		return nil
	}
	if !reg.AllowExternalCancel {
		return ErrCancelNotAllowed
	}
	return b.enqueueLocked(Job{
		Kind:   JobCancel,
		Key:    req.Key(),
		Cancel: &CancelJob{InstanceID: req.InstanceID, Reason: req.Reason},
	})
}

// Terminate publishes a JobTerminate. Requires AllowExternalTerminate.
func (b *InMemoryMessageBroker) Terminate(ctx context.Context, req TerminateRequest) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.closed {
		return ErrBrokerClosed
	}
	reg := b.liveRegistrationLocked(req.Key())
	if reg == nil {
		return ErrUnknownProcess
	}
	if !reg.AllowExternalTerminate {
		return ErrTerminateNotAllowed
	}
	return b.enqueueLocked(Job{
		Kind:      JobTerminate,
		Key:       req.Key(),
		Terminate: &TerminateJob{InstanceID: req.InstanceID, Reason: req.Reason},
	})
}

// RespondToInputRequest queues a JobRespondToInput for the instance's process
// key. An instance the broker no longer knows surfaces asynchronously as
// INSTANCE_NOT_FOUND on the instance's topic, per the error model.
func (b *InMemoryMessageBroker) RespondToInputRequest(ctx context.Context, instanceID, requestID string, payload map[string]any) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.closed {
		return ErrBrokerClosed
	}
	rec := b.insts[instanceID]
	if rec == nil || rec.finished {
		code := ErrCodeInstanceNotFound
		if rec != nil {
			code = ErrCodeAlreadyFinished
		}
		b.publishLocked(instanceID, InstanceEvent{
			Kind:  InstanceEventError,
			Error: &InstanceError{Code: code, Message: "respond-to-input for an instance the broker does not hold"},
		})
		return nil
	}
	return b.enqueueLocked(Job{
		Kind:           JobRespondToInput,
		Key:            rec.key,
		RespondToInput: &RespondToInputJob{InstanceID: instanceID, RequestID: requestID, Payload: payload},
	})
}

// SubscribeToInstance delivers the instance's events. Late subscribers first
// get the latest lifecycle event (and the terminal event if finished within
// retention); an instance with no visible events gets INSTANCE_NOT_FOUND as
// its first event.
func (b *InMemoryMessageBroker) SubscribeToInstance(ctx context.Context, instanceID string) (<-chan InstanceEvent, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.closed {
		return nil, ErrBrokerClosed
	}
	sub := &inMemSub{ch: make(chan InstanceEvent, max(b.cfg.eventBufferSize, 2))}
	rec := b.insts[instanceID]
	if rec == nil {
		sub.ch <- InstanceEvent{
			InstanceID: instanceID,
			Kind:       InstanceEventError,
			OccurredAt: time.Now(),
			Error:      &InstanceError{Code: ErrCodeInstanceNotFound, Message: "no events visible for this instance"},
		}
		rec = &inMemInstance{subs: map[int]*inMemSub{}}
		b.insts[instanceID] = rec
	} else {
		if rec.latestLifecycle != nil {
			sub.ch <- *rec.latestLifecycle
		}
		if rec.terminal != nil {
			sub.ch <- *rec.terminal
		}
		if rec.finished {
			close(sub.ch)
			sub.closed = true
			return sub.ch, nil
		}
	}
	id := b.nextSub
	b.nextSub++
	rec.subs[id] = sub
	go func() {
		select {
		case <-ctx.Done():
		case <-b.done:
			return // Close already closed the channel
		}
		b.mu.Lock()
		defer b.mu.Unlock()
		if rec.subs[id] == sub {
			delete(rec.subs, id)
			sub.close()
		}
	}()
	return sub.ch, nil
}

// SubscribeToProcessRegistry delivers the registry snapshot, the snapshot
// sentinel, then incremental updates until ctx is cancelled.
func (b *InMemoryMessageBroker) SubscribeToProcessRegistry(ctx context.Context) (<-chan RegistryUpdate, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.closed {
		return nil, ErrBrokerClosed
	}
	pump := newRegistryPump()
	for _, w := range b.workers {
		for i := range w.regs {
			pump.push(RegistryUpdate{Kind: RegistryUpdateSnapshot, Registration: &w.regs[i]})
		}
	}
	pump.push(RegistryUpdate{Kind: RegistryUpdateSnapshotComplete})
	id := b.nextSub
	b.nextSub++
	b.regSubs[id] = pump
	go func() {
		select {
		case <-ctx.Done():
		case <-b.done:
			return
		}
		b.mu.Lock()
		defer b.mu.Unlock()
		if b.regSubs[id] == pump {
			delete(b.regSubs, id)
			pump.stop()
		}
	}()
	return pump.out, nil
}

// ===== Worker side: registration =====

// RegisterProcesses replaces the worker's registration set and stamps
// registry metadata (RegisteredAt is preserved per key across
// re-registration).
func (b *InMemoryMessageBroker) RegisterProcesses(ctx context.Context, workerID string, regs []ProcessRegistration) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.closed {
		return ErrBrokerClosed
	}
	now := time.Now()
	w := b.workers[workerID]
	if w == nil {
		w = &inMemWorker{firstReg: map[ProcessKey]time.Time{}}
		b.workers[workerID] = w
	} else {
		for i := range w.regs {
			b.emitRegistryUpdateLocked(RegistryUpdate{Kind: RegistryUpdateRemoved, Registration: &w.regs[i]})
		}
	}
	stamped := make([]ProcessRegistration, len(regs))
	for i, r := range regs {
		r.WorkerID = workerID
		first, ok := w.firstReg[r.Key()]
		if !ok {
			first = now
			w.firstReg[r.Key()] = first
		}
		r.RegisteredAt = first
		r.LastHeartbeat = now
		stamped[i] = r
	}
	w.regs = stamped
	w.deadline = now.Add(b.cfg.registrationTTL)
	for i := range w.regs {
		b.emitRegistryUpdateLocked(RegistryUpdate{Kind: RegistryUpdateAdded, Registration: &w.regs[i]})
	}
	return nil
}

// Heartbeat refreshes the worker's registration TTL.
func (b *InMemoryMessageBroker) Heartbeat(ctx context.Context, workerID string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.closed {
		return ErrBrokerClosed
	}
	w := b.workers[workerID]
	if w == nil {
		return ErrUnknownWorker
	}
	now := time.Now()
	w.deadline = now.Add(b.cfg.registrationTTL)
	for i := range w.regs {
		w.regs[i].LastHeartbeat = now
	}
	return nil
}

// Unregister removes the worker's registrations immediately.
func (b *InMemoryMessageBroker) Unregister(ctx context.Context, workerID string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.closed {
		return ErrBrokerClosed
	}
	w := b.workers[workerID]
	if w == nil {
		return ErrUnknownWorker
	}
	delete(b.workers, workerID)
	for i := range w.regs {
		b.emitRegistryUpdateLocked(RegistryUpdate{Kind: RegistryUpdateRemoved, Registration: &w.regs[i]})
	}
	return nil
}

// ===== Worker side: job queue =====

// FetchJobs yields jobs for the given keys. Delivery moves a job in-flight
// with a redelivery timer; the Report* verbs settle it.
func (b *InMemoryMessageBroker) FetchJobs(ctx context.Context, keys []ProcessKey) (<-chan Job, error) {
	b.mu.Lock()
	if b.closed {
		b.mu.Unlock()
		return nil, ErrBrokerClosed
	}
	b.mu.Unlock()
	out := make(chan Job)
	go func() {
		defer close(out)
		for {
			b.mu.Lock()
			if b.closed {
				b.mu.Unlock()
				return
			}
			j := b.takePendingLocked(keys)
			wait := b.notify
			b.mu.Unlock()
			if j == nil {
				select {
				case <-ctx.Done():
					return
				case <-b.done:
					return
				case <-wait:
					continue
				}
			}
			select {
			case out <- j.job:
			case <-ctx.Done():
				b.requeue(j)
				return
			case <-b.done:
				return
			}
		}
	}()
	return out, nil
}

// takePendingLocked pops the first pending job for any of keys and moves it
// in-flight with a redelivery timer. Callers hold mu.
func (b *InMemoryMessageBroker) takePendingLocked(keys []ProcessKey) *inMemJob {
	for _, k := range keys {
		q := b.queues[k]
		if len(q) == 0 {
			continue
		}
		j := q[0]
		b.queues[k] = q[1:]
		j.settled = false
		b.inFlight[j] = struct{}{}
		inst := j.job.InstanceID()
		b.byInst[inst] = append(b.byInst[inst], j)
		j.timer = time.AfterFunc(b.cfg.inFlightTimeout, func() { b.requeue(j) })
		return j
	}
	return nil
}

// requeue returns an unsettled in-flight job to the front of its queue.
func (b *InMemoryMessageBroker) requeue(j *inMemJob) {
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.closed || j.settled {
		return
	}
	if _, ok := b.inFlight[j]; !ok {
		return
	}
	b.releaseLocked(j)
	b.queues[j.job.Key] = append([]*inMemJob{j}, b.queues[j.job.Key]...)
	b.signalLocked()
}

// releaseLocked drops the job from the in-flight indexes. Callers hold mu.
func (b *InMemoryMessageBroker) releaseLocked(j *inMemJob) {
	if j.timer != nil {
		j.timer.Stop()
		j.timer = nil
	}
	delete(b.inFlight, j)
	inst := j.job.InstanceID()
	jobs := b.byInst[inst]
	for i, x := range jobs {
		if x == j {
			b.byInst[inst] = append(jobs[:i], jobs[i+1:]...)
			break
		}
	}
	if len(b.byInst[inst]) == 0 {
		delete(b.byInst, inst)
	}
}

// settleInstanceLocked settles every in-flight job for the instance. Callers
// hold mu.
func (b *InMemoryMessageBroker) settleInstanceLocked(instanceID string) {
	for _, j := range b.byInst[instanceID] {
		j.settled = true
		if j.timer != nil {
			j.timer.Stop()
			j.timer = nil
		}
		delete(b.inFlight, j)
	}
	delete(b.byInst, instanceID)
}

// enqueueLocked round-trips the job through the CBOR envelope (exercising the
// wire format and any configured cipher, exactly as an external backend
// would) and appends it to its key's pending queue. Callers hold mu.
func (b *InMemoryMessageBroker) enqueueLocked(job Job) error {
	rt, err := b.roundTripJob(job)
	if err != nil {
		return err
	}
	b.queues[rt.Key] = append(b.queues[rt.Key], &inMemJob{job: rt})
	b.signalLocked()
	return nil
}

func (b *InMemoryMessageBroker) roundTripJob(job Job) (Job, error) {
	kind := map[JobKind]string{
		JobStart:          KindJobStart,
		JobRespondToInput: KindJobRespondToInput,
		JobCancel:         KindJobCancel,
		JobTerminate:      KindJobTerminate,
		JobResume:         KindJobResume,
	}[job.Kind]
	data, err := EncodeEnvelope(kind, job.InstanceID(), nil, job, b.cfg.cipher)
	if err != nil {
		return Job{}, err
	}
	env, err := DecodeEnvelope(data, b.cfg.cipher)
	if err != nil {
		return Job{}, err
	}
	var out Job
	if err := env.DecodePayload(&out); err != nil {
		return Job{}, err
	}
	return out, nil
}

// ===== Worker side: lifecycle reports and instance topic =====

// ReportRunning publishes the Running lifecycle event. Does not settle the
// in-flight job.
func (b *InMemoryMessageBroker) ReportRunning(ctx context.Context, instanceID string) error {
	return b.report(ctx, instanceID, func() {
		b.publishLocked(instanceID, InstanceEvent{
			Kind:      InstanceEventLifecycle,
			Lifecycle: &LifecycleChange{Phase: ProcessStatusRunning},
		})
	})
}

// ReportSuspended publishes the Suspended lifecycle event, settles the
// in-flight job, and — given a resumeAt — schedules the JobResume.
func (b *InMemoryMessageBroker) ReportSuspended(ctx context.Context, instanceID string, resumeAt *time.Time) error {
	return b.report(ctx, instanceID, func() {
		b.publishLocked(instanceID, InstanceEvent{
			Kind:      InstanceEventLifecycle,
			Lifecycle: &LifecycleChange{Phase: ProcessStatusSuspended},
		})
		b.settleInstanceLocked(instanceID)
		if resumeAt == nil {
			return
		}
		if t := b.timers[instanceID]; t != nil {
			t.Stop()
		}
		b.timers[instanceID] = time.AfterFunc(time.Until(*resumeAt), func() {
			b.mu.Lock()
			defer b.mu.Unlock()
			delete(b.timers, instanceID)
			rec := b.insts[instanceID]
			if b.closed || rec == nil || rec.finished {
				return
			}
			_ = b.enqueueLocked(Job{
				Kind:   JobResume,
				Key:    rec.key,
				Resume: &ResumeJob{InstanceID: instanceID},
			})
		})
	})
}

// ReportCompleted publishes the Completed lifecycle event and the Result
// event, settles the job, and closes subscriber channels.
func (b *InMemoryMessageBroker) ReportCompleted(ctx context.Context, instanceID string, result EvaluationResult) error {
	return b.report(ctx, instanceID, func() {
		b.finishLocked(instanceID, ProcessStatusCompleted, &InstanceEvent{
			Kind:   InstanceEventResult,
			Result: &result,
		})
	})
}

// ReportFailed publishes the Failed lifecycle event and the Error event,
// settles the job, and closes subscriber channels.
func (b *InMemoryMessageBroker) ReportFailed(ctx context.Context, instanceID string, instErr InstanceError) error {
	return b.report(ctx, instanceID, func() {
		b.finishLocked(instanceID, ProcessStatusFailed, &InstanceEvent{
			Kind:  InstanceEventError,
			Error: &instErr,
		})
	})
}

// ReportCancelled publishes the Cancelled lifecycle event, settles the job,
// and closes subscriber channels.
func (b *InMemoryMessageBroker) ReportCancelled(ctx context.Context, instanceID string) error {
	return b.report(ctx, instanceID, func() {
		b.finishLocked(instanceID, ProcessStatusCancelled, nil)
	})
}

// PostError publishes a non-terminal error event.
func (b *InMemoryMessageBroker) PostError(ctx context.Context, instanceID string, instErr InstanceError) error {
	return b.report(ctx, instanceID, func() {
		b.publishLocked(instanceID, InstanceEvent{Kind: InstanceEventError, Error: &instErr})
	})
}

// PostInputRequest publishes a RequestInputTask's request for input.
func (b *InMemoryMessageBroker) PostInputRequest(ctx context.Context, instanceID string, req InputRequest) error {
	return b.report(ctx, instanceID, func() {
		b.publishLocked(instanceID, InstanceEvent{Kind: InstanceEventInputRequest, InputRequest: &req})
	})
}

// PostNodeCompleted publishes a node-completion event.
func (b *InMemoryMessageBroker) PostNodeCompleted(ctx context.Context, instanceID string, nc NodeCompleted) error {
	return b.report(ctx, instanceID, func() {
		b.publishLocked(instanceID, InstanceEvent{Kind: InstanceEventNodeCompleted, NodeCompleted: &nc})
	})
}

func (b *InMemoryMessageBroker) report(ctx context.Context, instanceID string, fn func()) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if instanceID == "" {
		return ErrUnknownProcess
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.closed {
		return ErrBrokerClosed
	}
	fn()
	return nil
}

// finishLocked drives the instance to a terminal phase: terminal lifecycle
// event, then the extra Result/Error event, then channel close. Retention
// starts now. Callers hold mu.
func (b *InMemoryMessageBroker) finishLocked(instanceID string, phase ProcessStatus, extra *InstanceEvent) {
	b.settleInstanceLocked(instanceID)
	if t := b.timers[instanceID]; t != nil {
		t.Stop()
		delete(b.timers, instanceID)
	}
	rec := b.recordLocked(instanceID)
	if rec.finished {
		return
	}
	b.publishLocked(instanceID, InstanceEvent{
		Kind:      InstanceEventLifecycle,
		Lifecycle: &LifecycleChange{Phase: phase},
	})
	if extra != nil {
		b.publishLocked(instanceID, *extra)
	}
	rec.finished = true
	rec.expiresAt = time.Now().Add(b.cfg.eventRetention)
	for id, s := range rec.subs {
		s.close()
		delete(rec.subs, id)
	}
}

func (b *InMemoryMessageBroker) recordLocked(instanceID string) *inMemInstance {
	rec := b.insts[instanceID]
	if rec == nil {
		rec = &inMemInstance{subs: map[int]*inMemSub{}}
		b.insts[instanceID] = rec
	}
	return rec
}

// publishLocked stamps, round-trips, retains, and fans out one instance
// event. A full subscriber buffer drops the event; recovery is signalled with
// a synthetic BACKPRESSURE_DROP. Callers hold mu.
func (b *InMemoryMessageBroker) publishLocked(instanceID string, evt InstanceEvent) {
	rec := b.recordLocked(instanceID)
	evt.InstanceID = instanceID
	evt.CorrelationKey = rec.correlationKey
	evt.OccurredAt = time.Now()

	// Round-trip through the envelope so the in-memory broker exercises the
	// same wire path (CBOR + optional cipher) as the external backends.
	if data, err := EncodeEnvelope(KindInstanceEvent, instanceID, rec.correlationKey, evt, b.cfg.cipher); err == nil {
		if env, err := DecodeEnvelope(data, b.cfg.cipher); err == nil {
			var rt InstanceEvent
			if env.DecodePayload(&rt) == nil {
				evt = rt
			}
		}
	}

	switch evt.Kind {
	case InstanceEventLifecycle:
		e := evt
		rec.latestLifecycle = &e
	case InstanceEventResult:
		e := evt
		rec.terminal = &e
	case InstanceEventError:
		if rec.latestLifecycle != nil && rec.latestLifecycle.Lifecycle.Phase == ProcessStatusFailed {
			e := evt
			rec.terminal = &e
		}
	case InstanceEventInputRequest, InstanceEventNodeCompleted:
	}

	for _, s := range rec.subs {
		s.send(evt, instanceID)
	}
}

func (s *inMemSub) send(evt InstanceEvent, instanceID string) {
	if s.closed {
		return
	}
	if s.dropped {
		synthetic := InstanceEvent{
			InstanceID: instanceID,
			Kind:       InstanceEventError,
			OccurredAt: time.Now(),
			Error:      &InstanceError{Code: ErrCodeBackpressureDrop, Message: "subscriber buffer overflowed; events were dropped"},
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

func (s *inMemSub) close() {
	if !s.closed {
		s.closed = true
		close(s.ch)
	}
}

// ===== Registry helpers =====

// liveRegistrationLocked returns one live registration for the key (any
// worker). Callers hold mu.
func (b *InMemoryMessageBroker) liveRegistrationLocked(key ProcessKey) *ProcessRegistration {
	now := time.Now()
	for _, w := range b.workers {
		if now.After(w.deadline) {
			continue
		}
		for i := range w.regs {
			if w.regs[i].Key() == key {
				return &w.regs[i]
			}
		}
	}
	return nil
}

func (b *InMemoryMessageBroker) emitRegistryUpdateLocked(u RegistryUpdate) {
	for _, p := range b.regSubs {
		p.push(u)
	}
}

// removeQueuedStartLocked removes the instance's still-pending JobStart from
// its queue. Callers hold mu.
func (b *InMemoryMessageBroker) removeQueuedStartLocked(key ProcessKey, instanceID string) bool {
	q := b.queues[key]
	for i, j := range q {
		if j.job.Kind == JobStart && j.job.Start.InstanceID == instanceID {
			b.queues[key] = append(q[:i], q[i+1:]...)
			return true
		}
	}
	return false
}

// registryPump is a lossless per-watcher delivery queue: registry updates
// must not drop, so each watcher gets an unbounded buffer drained by its own
// goroutine.
type registryPump struct {
	mu     sync.Mutex
	queue  []RegistryUpdate
	signal chan struct{}
	done   chan struct{}
	once   sync.Once
	out    chan RegistryUpdate
}

func newRegistryPump() *registryPump {
	p := &registryPump{
		signal: make(chan struct{}, 1),
		done:   make(chan struct{}),
		out:    make(chan RegistryUpdate),
	}
	go p.run()
	return p
}

func (p *registryPump) push(u RegistryUpdate) {
	p.mu.Lock()
	p.queue = append(p.queue, u)
	p.mu.Unlock()
	select {
	case p.signal <- struct{}{}:
	default:
	}
}

func (p *registryPump) stop() {
	p.once.Do(func() { close(p.done) })
}

func (p *registryPump) run() {
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
