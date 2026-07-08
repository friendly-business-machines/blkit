package gpubsub

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"sync"
	"time"

	"cloud.google.com/go/firestore"
	"golang.org/x/oauth2/google"
	"google.golang.org/api/iterator"
	"google.golang.org/api/option"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	insecurecreds "google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/status"

	bl "github.com/friendly-business-machines/blkit/core"
)

// ===== Instance records =====
//
// Pub/Sub cannot deliver messages published before a subscriber's
// subscription existed (Seek over retained messages is unreliable on the
// emulator and slow in production), so the broker keeps a last-event record
// per instance in the RegistryStore: the latest lifecycle event, the terminal
// Result/Error event, the process key (for RespondToInputRequest routing) and
// the correlation key. SubscribeToInstance replays from this record first,
// then follows the live subscription.

// StoredEvent is one envelope-encoded InstanceEvent kept in an instance
// record. EventID matches the eventID attribute on the corresponding Pub/Sub
// message so subscribers can suppress the duplicate live delivery.
type StoredEvent struct {
	EventID string
	Data    []byte // envelope-encoded (and possibly E2E-encrypted) InstanceEvent
}

// InstanceRecord is the broker-coordination record for one process instance:
// routing metadata and the latest-event replay data. It never holds process
// state — that lives in the state store.
type InstanceRecord struct {
	InstanceID     string
	Key            bl.ProcessKey
	CorrelationKey *string
	Latest         *StoredEvent // latest lifecycle event
	Terminal       *StoredEvent // terminal Result/Error event, when finished
	Finished       bool
	UpdatedAt      time.Time
}

// InstanceStore is the extension of bl.RegistryStore the Google Pub/Sub
// broker requires: last-event records per instance. FirestoreRegistryStore
// implements it; any bl.RegistryStore handed to Config.Registry must too.
type InstanceStore interface {
	// PutInstance creates (or replaces) the record. Called once at Submit.
	PutInstance(ctx context.Context, rec InstanceRecord) error
	// UpdateInstance merges the supplied fields into the record, creating
	// it if missing. finished is only ever raised, never cleared.
	UpdateInstance(ctx context.Context, instanceID string, latest, terminal *StoredEvent, finished bool) error
	// GetInstance returns the record, or (nil, nil) when none exists.
	GetInstance(ctx context.Context, instanceID string) (*InstanceRecord, error)
}

// ===== Firestore RegistryStore =====

// FirestoreConfig configures NewFirestoreRegistryStore.
type FirestoreConfig struct {
	ProjectID   string              // GCP project owning the Firestore database
	DatabaseID  string              // default "(default)"
	Credentials *google.Credentials // nil = Application Default Credentials

	// CollectionPrefix isolates deployments sharing a database; default
	// "blkit". Collections: <prefix>-workers, <prefix>-timers,
	// <prefix>-instances.
	CollectionPrefix string

	// Endpoint is a development-only emulator endpoint override
	// (host:port). Production traffic always uses TLS through the SDK.
	Endpoint string

	// Cipher must match the broker's PayloadCipher: worker registrations
	// are stored envelope-encoded, so the store needs the cipher to decode
	// them for Snapshot/Watch. Default nil.
	Cipher bl.PayloadCipher

	// PollInterval is the Watch/janitor polling interval; default 200ms.
	// Firestore snapshot listeners would push instead, but a polling diff
	// loop keeps the store emulator-proof and dependency-light.
	PollInterval time.Duration

	// EventRetention is how long finished instance records are kept before
	// the janitor deletes them; default 1h.
	EventRetention time.Duration
}

// FirestoreRegistryStore implements bl.RegistryStore plus InstanceStore on
// Cloud Firestore: one document per worker holding its envelope-encoded
// registrations and TTL deadline, one document per pending timer, and one
// last-event document per instance. TTL expiry is enforced client-side (reads
// filter expired documents; a janitor goroutine sweeps them).
type FirestoreRegistryStore struct {
	cfg    FirestoreConfig
	client *firestore.Client

	done      chan struct{}
	closeOnce sync.Once
	wg        sync.WaitGroup
}

type workerDoc struct {
	Deadline  time.Time `firestore:"deadline"`
	Heartbeat time.Time `firestore:"heartbeat"`
	Regs      [][]byte  `firestore:"regs"` // envelope-encoded ProcessRegistrations
	Rev       string    `firestore:"rev"`  // changes on PutRegistrations, not on Touch
}

type timerDoc struct {
	FireAt time.Time `firestore:"fireAt"`
}

type instanceDoc struct {
	Namespace  string    `firestore:"namespace"`
	ProcessID  string    `firestore:"processId"`
	Version    string    `firestore:"version"`
	Corr       string    `firestore:"corr"`
	CorrSet    bool      `firestore:"corrSet"`
	LatestID   string    `firestore:"latestId"`
	Latest     []byte    `firestore:"latest"`
	TerminalID string    `firestore:"terminalId"`
	Terminal   []byte    `firestore:"terminal"`
	Finished   bool      `firestore:"finished"`
	UpdatedAt  time.Time `firestore:"updatedAt"`
}

// NewFirestoreRegistryStore connects to Firestore and starts the janitor.
// Release its resources with Close.
func NewFirestoreRegistryStore(cfg FirestoreConfig) (*FirestoreRegistryStore, error) {
	if cfg.ProjectID == "" {
		return nil, errors.New("gpubsub: FirestoreConfig.ProjectID is required")
	}
	if cfg.DatabaseID == "" {
		cfg.DatabaseID = firestore.DefaultDatabaseID
	}
	if cfg.CollectionPrefix == "" {
		cfg.CollectionPrefix = "blkit"
	}
	if cfg.PollInterval <= 0 {
		cfg.PollInterval = 200 * time.Millisecond
	}
	if cfg.EventRetention <= 0 {
		cfg.EventRetention = time.Hour
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	client, err := firestore.NewClientWithDatabase(ctx, cfg.ProjectID, cfg.DatabaseID, gcloudClientOptions(cfg.Credentials, cfg.Endpoint)...)
	if err != nil {
		return nil, fmt.Errorf("gpubsub: firestore client: %w", err)
	}
	s := &FirestoreRegistryStore{cfg: cfg, client: client, done: make(chan struct{})}
	s.wg.Add(1)
	go s.janitor()
	return s, nil
}

// Close stops the janitor and Watch feeds and closes the Firestore client.
// Idempotent.
func (s *FirestoreRegistryStore) Close() error {
	var err error
	s.closeOnce.Do(func() {
		close(s.done)
		s.wg.Wait()
		err = s.client.Close()
	})
	return err
}

func (s *FirestoreRegistryStore) workers() *firestore.CollectionRef {
	return s.client.Collection(s.cfg.CollectionPrefix + "-workers")
}

func (s *FirestoreRegistryStore) timers() *firestore.CollectionRef {
	return s.client.Collection(s.cfg.CollectionPrefix + "-timers")
}

func (s *FirestoreRegistryStore) instances() *firestore.CollectionRef {
	return s.client.Collection(s.cfg.CollectionPrefix + "-instances")
}

// ===== Worker registry =====

// PutRegistrations replaces the worker's registration set, stamping WorkerID,
// RegisteredAt (preserved per key across re-registration) and LastHeartbeat.
func (s *FirestoreRegistryStore) PutRegistrations(ctx context.Context, workerID string, regs []bl.ProcessRegistration, ttl time.Duration) error {
	now := time.Now()
	firstReg := map[bl.ProcessKey]time.Time{}
	if snap, err := s.workers().Doc(workerID).Get(ctx); err == nil {
		var doc workerDoc
		if snap.DataTo(&doc) == nil {
			for _, prior := range s.decodeRegs(doc) {
				firstReg[prior.Key()] = prior.RegisteredAt
			}
		}
	}
	encoded := make([][]byte, len(regs))
	for i, r := range regs {
		r.WorkerID = workerID
		if first, ok := firstReg[r.Key()]; ok {
			r.RegisteredAt = first
		} else {
			r.RegisteredAt = now
		}
		r.LastHeartbeat = now
		data, err := bl.EncodeEnvelope(bl.KindRegistration, "", nil, r, s.cfg.Cipher)
		if err != nil {
			return err
		}
		encoded[i] = data
	}
	_, err := s.workers().Doc(workerID).Set(ctx, workerDoc{
		Deadline:  now.Add(ttl),
		Heartbeat: now,
		Regs:      encoded,
		Rev:       randomID(),
	})
	if err != nil {
		return fmt.Errorf("gpubsub: put registrations: %w", err)
	}
	return nil
}

// Touch refreshes the worker's TTL deadline. An absent or already-expired
// worker is unknown.
func (s *FirestoreRegistryStore) Touch(ctx context.Context, workerID string, ttl time.Duration) error {
	ref := s.workers().Doc(workerID)
	snap, err := ref.Get(ctx)
	if status.Code(err) == codes.NotFound {
		return bl.ErrUnknownWorker
	}
	if err != nil {
		return fmt.Errorf("gpubsub: touch: %w", err)
	}
	var doc workerDoc
	if err := snap.DataTo(&doc); err != nil {
		return fmt.Errorf("gpubsub: touch: %w", err)
	}
	now := time.Now()
	if now.After(doc.Deadline) {
		return bl.ErrUnknownWorker
	}
	_, err = ref.Update(ctx, []firestore.Update{
		{Path: "deadline", Value: now.Add(ttl)},
		{Path: "heartbeat", Value: now},
	})
	if err != nil {
		return fmt.Errorf("gpubsub: touch: %w", err)
	}
	return nil
}

// Delete removes the worker's registrations. An absent or already-expired
// worker is unknown.
func (s *FirestoreRegistryStore) Delete(ctx context.Context, workerID string) error {
	ref := s.workers().Doc(workerID)
	snap, err := ref.Get(ctx)
	if status.Code(err) == codes.NotFound {
		return bl.ErrUnknownWorker
	}
	if err != nil {
		return fmt.Errorf("gpubsub: delete worker: %w", err)
	}
	var doc workerDoc
	if err := snap.DataTo(&doc); err != nil {
		return fmt.Errorf("gpubsub: delete worker: %w", err)
	}
	if time.Now().After(doc.Deadline) {
		// Expired but not yet swept: gone as far as callers are concerned.
		_, _ = ref.Delete(ctx)
		return bl.ErrUnknownWorker
	}
	if _, err := ref.Delete(ctx); err != nil {
		return fmt.Errorf("gpubsub: delete worker: %w", err)
	}
	return nil
}

// Snapshot returns every live (unexpired) registration.
func (s *FirestoreRegistryStore) Snapshot(ctx context.Context) ([]bl.ProcessRegistration, error) {
	workers, err := s.readWorkers(ctx)
	if err != nil {
		return nil, err
	}
	var regs []bl.ProcessRegistration
	for _, w := range workers {
		regs = append(regs, w.regs...)
	}
	return regs, nil
}

// Watch polls the workers collection and delivers the full
// SubscribeToProcessRegistry sequence: one Snapshot update per live
// registration, a SnapshotComplete sentinel, then incremental
// Added/Removed/HeartbeatLost diffs. The channel closes when ctx is cancelled
// or the store is closed.
func (s *FirestoreRegistryStore) Watch(ctx context.Context) (<-chan bl.RegistryUpdate, error) {
	select {
	case <-s.done:
		return nil, errors.New("gpubsub: registry store closed")
	default:
	}
	out := make(chan bl.RegistryUpdate)
	s.wg.Add(1)
	go func() {
		defer s.wg.Done()
		defer close(out)
		send := func(u bl.RegistryUpdate) bool {
			select {
			case out <- u:
				return true
			case <-ctx.Done():
				return false
			case <-s.done:
				return false
			}
		}
		emit := func(kind bl.RegistryUpdateKind, regs []bl.ProcessRegistration) bool {
			for i := range regs {
				if !send(bl.RegistryUpdate{Kind: kind, Registration: &regs[i]}) {
					return false
				}
			}
			return true
		}
		var prev map[string]watchedWorker
		ticker := time.NewTicker(s.cfg.PollInterval)
		defer ticker.Stop()
		for {
			now := time.Now()
			cur, err := s.readWorkers(ctx)
			if err == nil && prev == nil {
				for _, w := range cur {
					if !emit(bl.RegistryUpdateSnapshot, w.regs) {
						return
					}
				}
				if !send(bl.RegistryUpdate{Kind: bl.RegistryUpdateSnapshotComplete}) {
					return
				}
				prev = cur
			} else if err == nil {
				for id, pw := range prev {
					cw, ok := cur[id]
					switch {
					case !ok:
						kind := bl.RegistryUpdateRemoved
						if now.After(pw.deadline) {
							kind = bl.RegistryUpdateHeartbeatLost
						}
						if !emit(kind, pw.regs) {
							return
						}
					case cw.rev != pw.rev:
						if !emit(bl.RegistryUpdateRemoved, pw.regs) {
							return
						}
						if !emit(bl.RegistryUpdateAdded, cw.regs) {
							return
						}
					}
				}
				for id, cw := range cur {
					if _, ok := prev[id]; !ok {
						if !emit(bl.RegistryUpdateAdded, cw.regs) {
							return
						}
					}
				}
				prev = cur
			}
			select {
			case <-ctx.Done():
				return
			case <-s.done:
				return
			case <-ticker.C:
			}
		}
	}()
	return out, nil
}

type watchedWorker struct {
	deadline time.Time
	rev      string
	regs     []bl.ProcessRegistration
}

// readWorkers loads every worker document, dropping expired ones.
func (s *FirestoreRegistryStore) readWorkers(ctx context.Context) (map[string]watchedWorker, error) {
	now := time.Now()
	iter := s.workers().Documents(ctx)
	defer iter.Stop()
	out := map[string]watchedWorker{}
	for {
		snap, err := iter.Next()
		if errors.Is(err, iterator.Done) {
			return out, nil
		}
		if err != nil {
			return nil, fmt.Errorf("gpubsub: read workers: %w", err)
		}
		var doc workerDoc
		if err := snap.DataTo(&doc); err != nil {
			continue // malformed document; skip
		}
		if now.After(doc.Deadline) {
			continue
		}
		out[snap.Ref.ID] = watchedWorker{deadline: doc.Deadline, rev: doc.Rev, regs: s.decodeRegs(doc)}
	}
}

// decodeRegs decodes a worker document's envelope-encoded registrations,
// decorating each with the document-level heartbeat.
func (s *FirestoreRegistryStore) decodeRegs(doc workerDoc) []bl.ProcessRegistration {
	regs := make([]bl.ProcessRegistration, 0, len(doc.Regs))
	for _, data := range doc.Regs {
		env, err := bl.DecodeEnvelope(data, s.cfg.Cipher)
		if err != nil {
			continue
		}
		var reg bl.ProcessRegistration
		if err := env.DecodePayload(&reg); err != nil {
			continue
		}
		reg.LastHeartbeat = doc.Heartbeat
		regs = append(regs, reg)
	}
	return regs
}

// ===== Timers =====

// PutTimer records a durable "deliver a JobResume at fireAt" entry,
// replacing any pending timer for the instance.
func (s *FirestoreRegistryStore) PutTimer(ctx context.Context, instanceID string, fireAt time.Time) error {
	if _, err := s.timers().Doc(instanceID).Set(ctx, timerDoc{FireAt: fireAt}); err != nil {
		return fmt.Errorf("gpubsub: put timer: %w", err)
	}
	return nil
}

var errTimerGone = errors.New("timer already claimed")

// DueTimers claims and returns every timer due at now. Each claim runs in a
// Firestore transaction that deletes the document, so concurrent schedulers
// never double-fire a timer.
func (s *FirestoreRegistryStore) DueTimers(ctx context.Context, now time.Time) ([]string, error) {
	iter := s.timers().Where("fireAt", "<=", now).Documents(ctx)
	defer iter.Stop()
	var refs []*firestore.DocumentRef
	for {
		snap, err := iter.Next()
		if errors.Is(err, iterator.Done) {
			break
		}
		if err != nil {
			return nil, fmt.Errorf("gpubsub: due timers: %w", err)
		}
		refs = append(refs, snap.Ref)
	}
	var claimed []string
	for _, ref := range refs {
		err := s.client.RunTransaction(ctx, func(_ context.Context, tx *firestore.Transaction) error {
			snap, err := tx.Get(ref)
			if status.Code(err) == codes.NotFound {
				return errTimerGone
			}
			if err != nil {
				return err
			}
			var doc timerDoc
			if err := snap.DataTo(&doc); err != nil {
				return err
			}
			if doc.FireAt.After(now) {
				return errTimerGone // rescheduled since the query ran
			}
			return tx.Delete(ref)
		})
		if errors.Is(err, errTimerGone) {
			continue
		}
		if err != nil {
			return claimed, fmt.Errorf("gpubsub: claim timer: %w", err)
		}
		claimed = append(claimed, ref.ID)
	}
	return claimed, nil
}

// DeleteTimer removes the instance's pending timer, if any.
func (s *FirestoreRegistryStore) DeleteTimer(ctx context.Context, instanceID string) error {
	if _, err := s.timers().Doc(instanceID).Delete(ctx); err != nil {
		return fmt.Errorf("gpubsub: delete timer: %w", err)
	}
	return nil
}

// ===== Instance records =====

// PutInstance creates (or replaces) the instance's last-event record.
func (s *FirestoreRegistryStore) PutInstance(ctx context.Context, rec InstanceRecord) error {
	doc := instanceDoc{
		Namespace: rec.Key.Namespace,
		ProcessID: rec.Key.ProcessID,
		Version:   rec.Key.Version,
		Finished:  rec.Finished,
		UpdatedAt: time.Now(),
	}
	if rec.CorrelationKey != nil {
		doc.Corr, doc.CorrSet = *rec.CorrelationKey, true
	}
	if rec.Latest != nil {
		doc.LatestID, doc.Latest = rec.Latest.EventID, rec.Latest.Data
	}
	if rec.Terminal != nil {
		doc.TerminalID, doc.Terminal = rec.Terminal.EventID, rec.Terminal.Data
	}
	if _, err := s.instances().Doc(rec.InstanceID).Set(ctx, doc); err != nil {
		return fmt.Errorf("gpubsub: put instance: %w", err)
	}
	return nil
}

// UpdateInstance merges the supplied fields into the instance's record.
func (s *FirestoreRegistryStore) UpdateInstance(ctx context.Context, instanceID string, latest, terminal *StoredEvent, finished bool) error {
	m := map[string]any{"updatedAt": time.Now()}
	if latest != nil {
		m["latestId"], m["latest"] = latest.EventID, latest.Data
	}
	if terminal != nil {
		m["terminalId"], m["terminal"] = terminal.EventID, terminal.Data
	}
	if finished {
		m["finished"] = true
	}
	if _, err := s.instances().Doc(instanceID).Set(ctx, m, firestore.MergeAll); err != nil {
		return fmt.Errorf("gpubsub: update instance: %w", err)
	}
	return nil
}

// GetInstance returns the instance's record, or (nil, nil) when none exists.
func (s *FirestoreRegistryStore) GetInstance(ctx context.Context, instanceID string) (*InstanceRecord, error) {
	snap, err := s.instances().Doc(instanceID).Get(ctx)
	if status.Code(err) == codes.NotFound {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("gpubsub: get instance: %w", err)
	}
	var doc instanceDoc
	if err := snap.DataTo(&doc); err != nil {
		return nil, fmt.Errorf("gpubsub: get instance: %w", err)
	}
	rec := &InstanceRecord{
		InstanceID: instanceID,
		Key:        bl.ProcessKey{Namespace: doc.Namespace, ProcessID: doc.ProcessID, Version: doc.Version},
		Finished:   doc.Finished,
		UpdatedAt:  doc.UpdatedAt,
	}
	if doc.CorrSet {
		corr := doc.Corr
		rec.CorrelationKey = &corr
	}
	if len(doc.Latest) > 0 {
		rec.Latest = &StoredEvent{EventID: doc.LatestID, Data: doc.Latest}
	}
	if len(doc.Terminal) > 0 {
		rec.Terminal = &StoredEvent{EventID: doc.TerminalID, Data: doc.Terminal}
	}
	return rec, nil
}

// ===== Janitor =====

// janitor sweeps expired worker documents and finished instance records past
// their retention window. Reads already filter expired workers, so the sweep
// is pure garbage collection.
func (s *FirestoreRegistryStore) janitor() {
	defer s.wg.Done()
	interval := max(10*s.cfg.PollInterval, 2*time.Second)
	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	for {
		select {
		case <-s.done:
			return
		case <-ticker.C:
			ctx, cancel := context.WithTimeout(context.Background(), interval)
			s.sweepWorkers(ctx)
			s.sweepInstances(ctx)
			cancel()
		}
	}
}

func (s *FirestoreRegistryStore) sweepWorkers(ctx context.Context) {
	cutoff := time.Now().Add(-2 * s.cfg.PollInterval) // grace so watchers see the expiry first
	iter := s.workers().Where("deadline", "<", cutoff).Documents(ctx)
	defer iter.Stop()
	for {
		snap, err := iter.Next()
		if err != nil {
			return
		}
		_, _ = snap.Ref.Delete(ctx)
	}
}

func (s *FirestoreRegistryStore) sweepInstances(ctx context.Context) {
	cutoff := time.Now().Add(-s.cfg.EventRetention)
	iter := s.instances().Where("updatedAt", "<", cutoff).Documents(ctx)
	defer iter.Stop()
	for {
		snap, err := iter.Next()
		if err != nil {
			return
		}
		var doc instanceDoc
		if snap.DataTo(&doc) == nil && doc.Finished {
			_, _ = snap.Ref.Delete(ctx)
		}
	}
}

// ===== Shared helpers =====

// gcloudClientOptions builds the client options shared by the Pub/Sub and
// Firestore clients: an emulator endpoint (development only, plaintext) or
// explicit/ambient credentials over the SDK's always-on TLS.
func gcloudClientOptions(creds *google.Credentials, endpoint string) []option.ClientOption {
	if endpoint != "" {
		return []option.ClientOption{
			option.WithEndpoint(endpoint),
			option.WithoutAuthentication(),
			option.WithGRPCDialOption(grpc.WithTransportCredentials(insecurecreds.NewCredentials())),
		}
	}
	if creds != nil {
		return []option.ClientOption{option.WithCredentials(creds)}
	}
	return nil
}

// randomID returns a short random hex identifier.
func randomID() string {
	var b [8]byte
	if _, err := rand.Read(b[:]); err != nil {
		panic(err) // crypto/rand never fails on supported platforms
	}
	return hex.EncodeToString(b[:])
}
