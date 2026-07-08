// Package azuresb implements blkit's MessageBroker against Azure Service Bus
// (specs/message-brokers/azure-service-bus-message-broker.spec.md): peek-lock
// queues for jobs, one topic for instance events, native scheduled messages
// for suspend-resume timers, and a RegistryStore side-store — this file's
// TableRegistryStore on Azure Table Storage — for the worker registry, timer
// records, and last-event records.
package azuresb

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"sync"
	"time"

	"github.com/Azure/azure-sdk-for-go/sdk/azcore"
	"github.com/Azure/azure-sdk-for-go/sdk/azcore/to"
	"github.com/Azure/azure-sdk-for-go/sdk/data/aztables"

	bl "github.com/friendly-business-machines/blkit/core"
)

// InstanceRecord is the last-event record kept per process instance. Service
// Bus subscriptions cannot deliver messages published before they existed, so
// latest-event replay for late subscribers — and the instance→process-key
// routing RespondToInputRequest needs — comes from these records instead.
type InstanceRecord struct {
	Key            bl.ProcessKey `json:"key"`
	CorrelationKey *string       `json:"correlationKey,omitempty"`
	Latest         []byte        `json:"latest,omitempty"`   // envelope of the latest lifecycle InstanceEvent
	Terminal       []byte        `json:"terminal,omitempty"` // envelope of the terminal Result/Error InstanceEvent
	Finished       bool          `json:"finished,omitempty"`
	ExpiresAt      time.Time     `json:"expiresAt,omitzero"` // retention deadline once finished
}

// InstanceRecordStore is the extension of bl.RegistryStore this backend uses
// for last-event records. TableRegistryStore implements it; a custom
// RegistryStore that does not is handled with a per-handle in-memory fallback
// (replay then only works within one broker handle).
type InstanceRecordStore interface {
	PutInstanceRecord(ctx context.Context, instanceID string, rec InstanceRecord) error
	GetInstanceRecord(ctx context.Context, instanceID string) (*InstanceRecord, error)
}

// TableRegistryStoreConfig configures NewTableRegistryStore.
type TableRegistryStoreConfig struct {
	// ConnectionString is an Azure Storage connection string (Azurite in
	// tests). Alternatively set ServiceURL + Credential.
	ConnectionString string
	ServiceURL       string
	Credential       azcore.TokenCredential

	// TableName is the table holding all records. Required. Created if it
	// does not exist.
	TableName string

	// Cipher optionally end-to-end encrypts the envelope payloads stored in
	// the table (registrations, last events). Default nil.
	Cipher bl.PayloadCipher

	// PollInterval is the Watch feed's polling cadence (Table Storage has no
	// native change feed). Default 200ms.
	PollInterval time.Duration

	// SweepInterval is the TTL sweeper's cadence. Default 500ms.
	SweepInterval time.Duration

	// SweepGrace is how long past its deadline a worker entity survives
	// before the sweeper deletes it. The grace keeps expired entities
	// visible long enough for every Watch poller to observe the expiry and
	// emit RegistryUpdateHeartbeatLost (deleted entities are
	// indistinguishable from an Unregister). Default 2s.
	SweepGrace time.Duration
}

// TableRegistryStore implements bl.RegistryStore (plus InstanceRecordStore)
// on Azure Table Storage. One entity per worker holds the envelope-encoded
// registration set and its TTL deadline; timer records are claimed with
// ETag-conditioned deletes; last-event records hold the replay data Service
// Bus subscriptions cannot provide. A client-side sweeper enforces TTLs and
// retention; Watch is a polling diff (default 200ms).
type TableRegistryStore struct {
	cfg   TableRegistryStoreConfig
	table *aztables.Client

	done      chan struct{}
	closeOnce sync.Once
	wg        sync.WaitGroup
}

const (
	partitionWorkers   = "worker"
	partitionTimers    = "timer"
	partitionInstances = "inst"
)

type workerEntityData struct {
	Deadline time.Time `json:"deadline"`
	Regs     []byte    `json:"regs"` // envelope of []bl.ProcessRegistration
}

type timerEntityData struct {
	FireAt time.Time `json:"fireAt"`
}

// NewTableRegistryStore opens (and creates, if missing) the table and starts
// the TTL/retention sweeper. Release its goroutines with Close.
func NewTableRegistryStore(cfg TableRegistryStoreConfig) (*TableRegistryStore, error) {
	if cfg.TableName == "" {
		return nil, errors.New("azuresb: TableRegistryStoreConfig.TableName is required")
	}
	if cfg.PollInterval <= 0 {
		cfg.PollInterval = 200 * time.Millisecond
	}
	if cfg.SweepInterval <= 0 {
		cfg.SweepInterval = 500 * time.Millisecond
	}
	if cfg.SweepGrace <= 0 {
		cfg.SweepGrace = 2 * time.Second
	}
	var svc *aztables.ServiceClient
	var err error
	switch {
	case cfg.ConnectionString != "":
		svc, err = aztables.NewServiceClientFromConnectionString(cfg.ConnectionString, nil)
	case cfg.ServiceURL != "" && cfg.Credential != nil:
		svc, err = aztables.NewServiceClient(cfg.ServiceURL, cfg.Credential, nil)
	default:
		return nil, errors.New("azuresb: TableRegistryStore needs ConnectionString or ServiceURL+Credential")
	}
	if err != nil {
		return nil, fmt.Errorf("azuresb: table service client: %w", err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	if _, err := svc.CreateTable(ctx, cfg.TableName, nil); err != nil && !isConflict(err) {
		return nil, fmt.Errorf("azuresb: create table %s: %w", cfg.TableName, err)
	}
	s := &TableRegistryStore{
		cfg:   cfg,
		table: svc.NewClient(cfg.TableName),
		done:  make(chan struct{}),
	}
	s.wg.Add(1)
	go s.sweep()
	return s, nil
}

// Close stops the sweeper and every Watch poller. Idempotent.
func (s *TableRegistryStore) Close() error {
	s.closeOnce.Do(func() { close(s.done) })
	s.wg.Wait()
	return nil
}

// ===== entity plumbing =====

func isConflict(err error) bool {
	var respErr *azcore.ResponseError
	return errors.As(err, &respErr) && respErr.StatusCode == 409
}

func isNotFound(err error) bool {
	var respErr *azcore.ResponseError
	return errors.As(err, &respErr) && respErr.StatusCode == 404
}

func isPreconditionFailed(err error) bool {
	var respErr *azcore.ResponseError
	return errors.As(err, &respErr) && respErr.StatusCode == 412
}

// putEntity upserts one entity whose whole payload is a single JSON "Data"
// property (sidestepping EDM property-type mapping).
func (s *TableRegistryStore) putEntity(ctx context.Context, pk, rk string, data any) error {
	payload, err := json.Marshal(data)
	if err != nil {
		return fmt.Errorf("azuresb: encode %s/%s: %w", pk, rk, err)
	}
	ent := aztables.EDMEntity{
		Entity:     aztables.Entity{PartitionKey: pk, RowKey: rk},
		Properties: map[string]any{"Data": string(payload)},
	}
	blob, err := json.Marshal(ent)
	if err != nil {
		return fmt.Errorf("azuresb: marshal entity %s/%s: %w", pk, rk, err)
	}
	if _, err := s.table.UpsertEntity(ctx, blob, &aztables.UpsertEntityOptions{UpdateMode: aztables.UpdateModeReplace}); err != nil {
		return fmt.Errorf("azuresb: upsert %s/%s: %w", pk, rk, err)
	}
	return nil
}

// getEntity fetches one entity's Data payload. Missing entity → (false, nil).
func (s *TableRegistryStore) getEntity(ctx context.Context, pk, rk string, into any) (bool, error) {
	resp, err := s.table.GetEntity(ctx, pk, rk, nil)
	if err != nil {
		if isNotFound(err) {
			return false, nil
		}
		return false, fmt.Errorf("azuresb: get %s/%s: %w", pk, rk, err)
	}
	if err := decodeEntityData(resp.Value, into); err != nil {
		return false, err
	}
	return true, nil
}

func decodeEntityData(raw []byte, into any) error {
	var ent aztables.EDMEntity
	if err := json.Unmarshal(raw, &ent); err != nil {
		return fmt.Errorf("azuresb: unmarshal entity: %w", err)
	}
	data, _ := ent.Properties["Data"].(string)
	if err := json.Unmarshal([]byte(data), into); err != nil {
		return fmt.Errorf("azuresb: decode entity data: %w", err)
	}
	return nil
}

type entityRow struct {
	rowKey string
	etag   azcore.ETag
	data   []byte // the JSON "Data" property
}

func (s *TableRegistryStore) listEntities(ctx context.Context, pk string) ([]entityRow, error) {
	filter := fmt.Sprintf("PartitionKey eq '%s'", pk)
	pager := s.table.NewListEntitiesPager(&aztables.ListEntitiesOptions{Filter: to.Ptr(filter)})
	var rows []entityRow
	for pager.More() {
		page, err := pager.NextPage(ctx)
		if err != nil {
			return nil, fmt.Errorf("azuresb: list %s entities: %w", pk, err)
		}
		for _, raw := range page.Entities {
			var ent aztables.EDMEntity
			if err := json.Unmarshal(raw, &ent); err != nil {
				continue
			}
			data, _ := ent.Properties["Data"].(string)
			rows = append(rows, entityRow{rowKey: ent.RowKey, etag: azcore.ETag(ent.ETag), data: []byte(data)})
		}
	}
	return rows, nil
}

// ===== worker registry =====

// PutRegistrations replaces the worker's registration set with a fresh TTL
// deadline. RegisteredAt is preserved per process key across re-registration.
func (s *TableRegistryStore) PutRegistrations(ctx context.Context, workerID string, regs []bl.ProcessRegistration, ttl time.Duration) error {
	firstReg := map[bl.ProcessKey]time.Time{}
	var existing workerEntityData
	if found, err := s.getEntity(ctx, partitionWorkers, workerID, &existing); err == nil && found {
		if old, err := s.decodeRegs(existing.Regs); err == nil {
			for _, r := range old {
				firstReg[r.Key()] = r.RegisteredAt
			}
		}
	}
	stamped := make([]bl.ProcessRegistration, len(regs))
	for i, r := range regs {
		if first, ok := firstReg[r.Key()]; ok && !first.IsZero() {
			r.RegisteredAt = first
		}
		stamped[i] = r
	}
	env, err := bl.EncodeEnvelope(bl.KindRegistration, "", nil, stamped, s.cfg.Cipher)
	if err != nil {
		return err
	}
	return s.putEntity(ctx, partitionWorkers, workerID, workerEntityData{
		Deadline: time.Now().Add(ttl),
		Regs:     env,
	})
}

// Touch refreshes the worker's TTL deadline and LastHeartbeat stamps.
func (s *TableRegistryStore) Touch(ctx context.Context, workerID string, ttl time.Duration) error {
	var data workerEntityData
	found, err := s.getEntity(ctx, partitionWorkers, workerID, &data)
	if err != nil {
		return err
	}
	if !found {
		return bl.ErrUnknownWorker
	}
	regs, err := s.decodeRegs(data.Regs)
	if err != nil {
		return err
	}
	now := time.Now()
	for i := range regs {
		regs[i].LastHeartbeat = now
	}
	env, err := bl.EncodeEnvelope(bl.KindRegistration, "", nil, regs, s.cfg.Cipher)
	if err != nil {
		return err
	}
	return s.putEntity(ctx, partitionWorkers, workerID, workerEntityData{Deadline: now.Add(ttl), Regs: env})
}

// Delete removes the worker's registrations immediately (unregister).
func (s *TableRegistryStore) Delete(ctx context.Context, workerID string) error {
	if _, err := s.table.DeleteEntity(ctx, partitionWorkers, workerID, nil); err != nil {
		if isNotFound(err) {
			return bl.ErrUnknownWorker
		}
		return fmt.Errorf("azuresb: delete worker %s: %w", workerID, err)
	}
	return nil
}

// Snapshot returns every registration whose worker entity still exists. TTL
// expiry is enforced by the sweeper (deadline + SweepGrace), so a lapsed
// registration stays visible for at most the grace period.
func (s *TableRegistryStore) Snapshot(ctx context.Context) ([]bl.ProcessRegistration, error) {
	rows, err := s.listEntities(ctx, partitionWorkers)
	if err != nil {
		return nil, err
	}
	var out []bl.ProcessRegistration
	for _, row := range rows {
		var data workerEntityData
		if err := json.Unmarshal(row.data, &data); err != nil {
			continue
		}
		regs, err := s.decodeRegs(data.Regs)
		if err != nil {
			return nil, err
		}
		out = append(out, regs...)
	}
	return out, nil
}

func (s *TableRegistryStore) decodeRegs(envData []byte) ([]bl.ProcessRegistration, error) {
	env, err := bl.DecodeEnvelope(envData, s.cfg.Cipher)
	if err != nil {
		return nil, err
	}
	var regs []bl.ProcessRegistration
	if err := env.DecodePayload(&regs); err != nil {
		return nil, err
	}
	return regs, nil
}

// ===== Watch: polling diff =====

type workerView struct {
	live bool
	regs map[bl.ProcessKey]bl.ProcessRegistration
}

// Watch emits incremental registry changes. Table Storage has no change
// feed, so the feed is a polling diff on PollInterval (default 200ms): a
// worker entity appearing → Added per registration; disappearing → Removed;
// present past its deadline → HeartbeatLost. The baseline is captured
// synchronously, so only changes after Watch returns are emitted. The channel
// closes on ctx cancellation or store Close; updates are never dropped (each
// watcher drains through an unbounded pump).
func (s *TableRegistryStore) Watch(ctx context.Context) (<-chan bl.RegistryUpdate, error) {
	prev, err := s.pollWorkers(ctx)
	if err != nil {
		return nil, err
	}
	pump := newUpdatePump()
	s.wg.Add(1)
	go func() {
		defer s.wg.Done()
		defer pump.stop()
		ticker := time.NewTicker(s.cfg.PollInterval)
		defer ticker.Stop()
		for {
			select {
			case <-ctx.Done():
				return
			case <-s.done:
				return
			case <-ticker.C:
				cur, err := s.pollWorkers(ctx)
				if err != nil {
					continue // transient; keep the previous view
				}
				diffWorkerViews(prev, cur, pump.push)
				prev = cur
			}
		}
	}()
	return pump.out, nil
}

func (s *TableRegistryStore) pollWorkers(ctx context.Context) (map[string]workerView, error) {
	rows, err := s.listEntities(ctx, partitionWorkers)
	if err != nil {
		return nil, err
	}
	now := time.Now()
	views := make(map[string]workerView, len(rows))
	for _, row := range rows {
		var data workerEntityData
		if err := json.Unmarshal(row.data, &data); err != nil {
			continue
		}
		regs, err := s.decodeRegs(data.Regs)
		if err != nil {
			continue
		}
		v := workerView{live: now.Before(data.Deadline), regs: map[bl.ProcessKey]bl.ProcessRegistration{}}
		for _, r := range regs {
			v.regs[r.Key()] = r
		}
		views[row.rowKey] = v
	}
	return views, nil
}

// diffWorkerViews emits the RegistryUpdates that turn prev into cur.
func diffWorkerViews(prev, cur map[string]workerView, emit func(bl.RegistryUpdate)) {
	emitAll := func(kind bl.RegistryUpdateKind, regs map[bl.ProcessKey]bl.ProcessRegistration) {
		for _, r := range regs {
			reg := r
			emit(bl.RegistryUpdate{Kind: kind, Registration: &reg})
		}
	}
	for id, was := range prev {
		now, ok := cur[id]
		switch {
		case !ok: // entity gone: Unregister (or swept long after HeartbeatLost)
			if was.live {
				emitAll(bl.RegistryUpdateRemoved, was.regs)
			}
		case !now.live: // present but past deadline
			if was.live {
				emitAll(bl.RegistryUpdateHeartbeatLost, was.regs)
			}
		case !was.live: // revived (re-register after expiry)
			emitAll(bl.RegistryUpdateAdded, now.regs)
		default: // live → live: per-key diff
			for key, r := range now.regs {
				if _, had := was.regs[key]; !had {
					reg := r
					emit(bl.RegistryUpdate{Kind: bl.RegistryUpdateAdded, Registration: &reg})
				}
			}
			for key, r := range was.regs {
				if _, has := now.regs[key]; !has {
					reg := r
					emit(bl.RegistryUpdate{Kind: bl.RegistryUpdateRemoved, Registration: &reg})
				}
			}
		}
	}
	for id, now := range cur {
		if _, ok := prev[id]; !ok && now.live {
			emitAll(bl.RegistryUpdateAdded, now.regs)
		}
	}
}

// ===== timers =====

// PutTimer records a durable "deliver a JobResume at fireAt" entry. The
// Service Bus broker itself uses native scheduled messages instead; these
// records exist to satisfy the RegistryStore contract for deployments that
// want store-driven timers.
func (s *TableRegistryStore) PutTimer(ctx context.Context, instanceID string, fireAt time.Time) error {
	return s.putEntity(ctx, partitionTimers, instanceID, timerEntityData{FireAt: fireAt})
}

// DueTimers returns the instanceIDs of timers due at now, each claimed
// atomically with an ETag-conditioned delete so concurrent claimers never
// double-fire.
func (s *TableRegistryStore) DueTimers(ctx context.Context, now time.Time) ([]string, error) {
	rows, err := s.listEntities(ctx, partitionTimers)
	if err != nil {
		return nil, err
	}
	var claimed []string
	for _, row := range rows {
		var data timerEntityData
		if err := json.Unmarshal(row.data, &data); err != nil {
			continue
		}
		if data.FireAt.After(now) {
			continue
		}
		etag := row.etag
		if _, err := s.table.DeleteEntity(ctx, partitionTimers, row.rowKey, &aztables.DeleteEntityOptions{IfMatch: &etag}); err != nil {
			if isNotFound(err) || isPreconditionFailed(err) {
				continue // another claimer won
			}
			return claimed, fmt.Errorf("azuresb: claim timer %s: %w", row.rowKey, err)
		}
		claimed = append(claimed, row.rowKey)
	}
	return claimed, nil
}

// DeleteTimer removes a timer record. Deleting an absent timer is a no-op.
func (s *TableRegistryStore) DeleteTimer(ctx context.Context, instanceID string) error {
	if _, err := s.table.DeleteEntity(ctx, partitionTimers, instanceID, nil); err != nil && !isNotFound(err) {
		return fmt.Errorf("azuresb: delete timer %s: %w", instanceID, err)
	}
	return nil
}

// ===== last-event records =====

// PutInstanceRecord upserts the instance's last-event record.
func (s *TableRegistryStore) PutInstanceRecord(ctx context.Context, instanceID string, rec InstanceRecord) error {
	return s.putEntity(ctx, partitionInstances, instanceID, rec)
}

// GetInstanceRecord fetches the instance's last-event record. Absent — or
// finished and past its retention deadline — returns (nil, nil).
func (s *TableRegistryStore) GetInstanceRecord(ctx context.Context, instanceID string) (*InstanceRecord, error) {
	var rec InstanceRecord
	found, err := s.getEntity(ctx, partitionInstances, instanceID, &rec)
	if err != nil {
		return nil, err
	}
	if !found || (rec.Finished && !rec.ExpiresAt.IsZero() && time.Now().After(rec.ExpiresAt)) {
		return nil, nil
	}
	return &rec, nil
}

// ===== sweeper =====

// sweep deletes worker entities expired past SweepGrace (watchers have had
// time to emit HeartbeatLost) and finished instance records past retention.
func (s *TableRegistryStore) sweep() {
	defer s.wg.Done()
	ticker := time.NewTicker(s.cfg.SweepInterval)
	defer ticker.Stop()
	for {
		select {
		case <-s.done:
			return
		case <-ticker.C:
			ctx, cancel := context.WithTimeout(context.Background(), s.cfg.SweepInterval*4)
			s.sweepOnce(ctx, time.Now())
			cancel()
		}
	}
}

func (s *TableRegistryStore) sweepOnce(ctx context.Context, now time.Time) {
	if rows, err := s.listEntities(ctx, partitionWorkers); err == nil {
		for _, row := range rows {
			var data workerEntityData
			if err := json.Unmarshal(row.data, &data); err != nil {
				continue
			}
			if now.After(data.Deadline.Add(s.cfg.SweepGrace)) {
				etag := row.etag
				_, _ = s.table.DeleteEntity(ctx, partitionWorkers, row.rowKey, &aztables.DeleteEntityOptions{IfMatch: &etag})
			}
		}
	}
	if rows, err := s.listEntities(ctx, partitionInstances); err == nil {
		for _, row := range rows {
			var rec InstanceRecord
			if err := json.Unmarshal(row.data, &rec); err != nil {
				continue
			}
			if rec.Finished && !rec.ExpiresAt.IsZero() && now.After(rec.ExpiresAt) {
				_, _ = s.table.DeleteEntity(ctx, partitionInstances, row.rowKey, nil)
			}
		}
	}
}

// ===== lossless per-watcher delivery =====

// updatePump is a lossless delivery queue: registry updates must not drop,
// so each watcher gets an unbounded buffer drained by its own goroutine.
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

// memRecordStore is the per-handle fallback when the configured RegistryStore
// does not implement InstanceRecordStore.
type memRecordStore struct {
	mu   sync.RWMutex
	recs map[string]InstanceRecord
}

func newMemRecordStore() *memRecordStore {
	return &memRecordStore{recs: map[string]InstanceRecord{}}
}

func (m *memRecordStore) PutInstanceRecord(_ context.Context, instanceID string, rec InstanceRecord) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.recs[instanceID] = rec
	return nil
}

func (m *memRecordStore) GetInstanceRecord(_ context.Context, instanceID string) (*InstanceRecord, error) {
	m.mu.RLock()
	defer m.mu.RUnlock()
	rec, ok := m.recs[instanceID]
	if !ok || (rec.Finished && !rec.ExpiresAt.IsZero() && time.Now().After(rec.ExpiresAt)) {
		return nil, nil
	}
	return &rec, nil
}
