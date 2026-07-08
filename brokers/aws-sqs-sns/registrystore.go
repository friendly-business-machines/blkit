package awssqssns

import (
	"context"
	"errors"
	"fmt"
	"strconv"
	"sync"
	"time"

	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb"
	ddbtypes "github.com/aws/aws-sdk-go-v2/service/dynamodb/types"

	bl "github.com/friendly-business-machines/blkit/core"
)

// Item key prefixes: the store keeps the worker registry, timer records, and
// last-event records in one DynamoDB table, partitioned by a prefixed key.
const (
	workerKeyPrefix = "worker#"
	timerKeyPrefix  = "timer#"
	instKeyPrefix   = "inst#"
)

// DynamoDB attribute names.
const (
	dynAttrPK        = "pk"
	dynAttrWorkerID  = "workerId"
	dynAttrDeadline  = "deadline"  // registration TTL deadline, unix milliseconds
	dynAttrBeat      = "beat"      // last heartbeat, unix milliseconds
	dynAttrRegs      = "regs"      // list of envelope-encoded ProcessRegistrations
	dynAttrExpiresAt = "expiresAt" // native DynamoDB TTL, unix seconds (belt-and-braces cleanup only)
	dynAttrFireAt    = "fireAt"    // timer fire time, unix milliseconds
	dynAttrNamespace = "ns"
	dynAttrProcessID = "proc"
	dynAttrVersion   = "ver"
	dynAttrCorr      = "corr"
	dynAttrSeq       = "seq"
	dynAttrLatest    = "latest"   // envelope-encoded latest lifecycle InstanceEvent
	dynAttrTerminal  = "terminal" // envelope-encoded terminal Result/Error InstanceEvent
	dynAttrFinished  = "finished"
)

// nativeTTLSlack is added to the registration deadline when computing the
// native DynamoDB TTL attribute. Expiry is enforced by the client-side
// sweeper and deadline comparisons — DynamoDB native TTL lags by minutes to
// hours and is only a fallback cleanup for abandoned items.
const nativeTTLSlack = 24 * time.Hour

// DynamoRegistryStoreConfig configures NewDynamoRegistryStore.
type DynamoRegistryStoreConfig struct {
	Region      string                  // e.g. "eu-west-2"
	Credentials aws.CredentialsProvider // nil = default AWS credential chain
	Endpoint    string                  // development only; LocalStack endpoint override
	TableName   string                  // default "blkit-registry"; created if absent
	Cipher      bl.PayloadCipher        // optional end-to-end payload encryption; must match the broker's

	PollInterval  time.Duration // Watch/DueTimers polling cadence; default 200ms
	SweepInterval time.Duration // expired-registration sweeper cadence; default 1s
}

// DynamoRegistryStore implements bl.RegistryStore (worker registry with TTL
// expiry and a change feed, plus timer records) and InstanceEventStore
// (last-event records) against a single DynamoDB table.
//
// TTL expiry is client-side: registrations carry a deadline attribute, reads
// treat past-deadline items as gone, and a background sweeper deletes them.
// A native DynamoDB TTL attribute is also set, purely as delayed fallback
// cleanup. Watch is a polling change feed (~200ms) that diffs snapshots — it
// does not use DynamoDB Streams.
type DynamoRegistryStore struct {
	cfg DynamoRegistryStoreConfig
	db  *dynamodb.Client

	ctx    context.Context
	cancel context.CancelFunc
	wg     sync.WaitGroup

	mu     sync.Mutex
	closed bool
}

// NewDynamoRegistryStore connects to DynamoDB, creates the table if it does
// not exist, and starts the expiry sweeper. Release its goroutines with
// Close.
func NewDynamoRegistryStore(cfg DynamoRegistryStoreConfig) (*DynamoRegistryStore, error) {
	if cfg.Region == "" {
		return nil, errors.New("awssqssns: DynamoRegistryStoreConfig.Region is required")
	}
	if cfg.TableName == "" {
		cfg.TableName = "blkit-registry"
	}
	if cfg.PollInterval <= 0 {
		cfg.PollInterval = 200 * time.Millisecond
	}
	if cfg.SweepInterval <= 0 {
		cfg.SweepInterval = time.Second
	}

	setupCtx, setupCancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer setupCancel()

	awsCfg, err := loadAWSConfig(setupCtx, cfg.Region, cfg.Credentials)
	if err != nil {
		return nil, fmt.Errorf("awssqssns: load AWS config: %w", err)
	}
	db := dynamodb.NewFromConfig(awsCfg, func(o *dynamodb.Options) {
		if cfg.Endpoint != "" {
			o.BaseEndpoint = aws.String(cfg.Endpoint)
		}
	})
	if err := ensureTable(setupCtx, db, cfg.TableName); err != nil {
		return nil, err
	}

	ctx, cancel := context.WithCancel(context.Background())
	s := &DynamoRegistryStore{cfg: cfg, db: db, ctx: ctx, cancel: cancel}
	s.wg.Add(1)
	go s.sweep()
	return s, nil
}

func ensureTable(ctx context.Context, db *dynamodb.Client, table string) error {
	_, err := db.CreateTable(ctx, &dynamodb.CreateTableInput{
		TableName: aws.String(table),
		AttributeDefinitions: []ddbtypes.AttributeDefinition{
			{AttributeName: aws.String(dynAttrPK), AttributeType: ddbtypes.ScalarAttributeTypeS},
		},
		KeySchema: []ddbtypes.KeySchemaElement{
			{AttributeName: aws.String(dynAttrPK), KeyType: ddbtypes.KeyTypeHash},
		},
		BillingMode: ddbtypes.BillingModePayPerRequest,
	})
	var inUse *ddbtypes.ResourceInUseException
	if err != nil && !errors.As(err, &inUse) {
		return fmt.Errorf("awssqssns: create table %s: %w", table, err)
	}
	waiter := dynamodb.NewTableExistsWaiter(db)
	if err := waiter.Wait(ctx, &dynamodb.DescribeTableInput{TableName: aws.String(table)}, 30*time.Second); err != nil {
		return fmt.Errorf("awssqssns: wait for table %s: %w", table, err)
	}
	// Best-effort: native TTL is only fallback cleanup, so a store pointed at
	// an emulator without TTL support still works.
	_, _ = db.UpdateTimeToLive(ctx, &dynamodb.UpdateTimeToLiveInput{
		TableName: aws.String(table),
		TimeToLiveSpecification: &ddbtypes.TimeToLiveSpecification{
			AttributeName: aws.String(dynAttrExpiresAt),
			Enabled:       aws.Bool(true),
		},
	})
	return nil
}

// Close stops the sweeper and any Watch feeds bound to the store's lifetime.
// Idempotent.
func (s *DynamoRegistryStore) Close() error {
	s.mu.Lock()
	if s.closed {
		s.mu.Unlock()
		return nil
	}
	s.closed = true
	s.mu.Unlock()
	s.cancel()
	s.wg.Wait()
	return nil
}

// sweep deletes worker items whose registration deadline lapsed. Expiry is
// already enforced on every read; the sweeper just removes the dead items.
func (s *DynamoRegistryStore) sweep() {
	defer s.wg.Done()
	ticker := time.NewTicker(s.cfg.SweepInterval)
	defer ticker.Stop()
	for {
		select {
		case <-s.ctx.Done():
			return
		case <-ticker.C:
			items, err := s.scanWorkers(s.ctx)
			if err != nil {
				continue
			}
			now := time.Now()
			for _, it := range items {
				if it.deadline.After(now) {
					continue
				}
				// Conditioned on the observed deadline so a concurrent
				// re-register or heartbeat is never clobbered.
				_, _ = s.db.DeleteItem(s.ctx, &dynamodb.DeleteItemInput{
					TableName:           aws.String(s.cfg.TableName),
					Key:                 pkKey(workerKeyPrefix + it.workerID),
					ConditionExpression: aws.String("#d = :d"),
					ExpressionAttributeNames: map[string]string{
						"#d": dynAttrDeadline,
					},
					ExpressionAttributeValues: map[string]ddbtypes.AttributeValue{
						":d": avN(it.deadline.UnixMilli()),
					},
				})
			}
		}
	}
}

// ===== Worker registry =====

// PutRegistrations stores the worker's registration set as one item: a list
// of envelope-encoded registrations plus the TTL deadline. Replaces any prior
// set for the same workerID.
func (s *DynamoRegistryStore) PutRegistrations(ctx context.Context, workerID string, regs []bl.ProcessRegistration, ttl time.Duration) error {
	now := time.Now()
	list := make([]ddbtypes.AttributeValue, 0, len(regs))
	for i := range regs {
		data, err := bl.EncodeEnvelope(bl.KindRegistration, "", nil, regs[i], s.cfg.Cipher)
		if err != nil {
			return err
		}
		list = append(list, &ddbtypes.AttributeValueMemberB{Value: data})
	}
	deadline := now.Add(ttl)
	_, err := s.db.PutItem(ctx, &dynamodb.PutItemInput{
		TableName: aws.String(s.cfg.TableName),
		Item: map[string]ddbtypes.AttributeValue{
			dynAttrPK:        avS(workerKeyPrefix + workerID),
			dynAttrWorkerID:  avS(workerID),
			dynAttrDeadline:  avN(deadline.UnixMilli()),
			dynAttrBeat:      avN(now.UnixMilli()),
			dynAttrRegs:      &ddbtypes.AttributeValueMemberL{Value: list},
			dynAttrExpiresAt: avN(deadline.Add(nativeTTLSlack).Unix()),
		},
	})
	if err != nil {
		return fmt.Errorf("awssqssns: put registrations for %s: %w", workerID, err)
	}
	return nil
}

// Touch refreshes the worker's TTL deadline. A worker that is absent — or
// whose deadline already lapsed — is unknown.
func (s *DynamoRegistryStore) Touch(ctx context.Context, workerID string, ttl time.Duration) error {
	now := time.Now()
	deadline := now.Add(ttl)
	_, err := s.db.UpdateItem(ctx, &dynamodb.UpdateItemInput{
		TableName:           aws.String(s.cfg.TableName),
		Key:                 pkKey(workerKeyPrefix + workerID),
		UpdateExpression:    aws.String("SET #d = :d, #b = :b, #e = :e"),
		ConditionExpression: aws.String("attribute_exists(#pk) AND #d > :now"),
		ExpressionAttributeNames: map[string]string{
			"#pk": dynAttrPK,
			"#d":  dynAttrDeadline,
			"#b":  dynAttrBeat,
			"#e":  dynAttrExpiresAt,
		},
		ExpressionAttributeValues: map[string]ddbtypes.AttributeValue{
			":d":   avN(deadline.UnixMilli()),
			":b":   avN(now.UnixMilli()),
			":e":   avN(deadline.Add(nativeTTLSlack).Unix()),
			":now": avN(now.UnixMilli()),
		},
	})
	if isConditionalCheckFailed(err) {
		return bl.ErrUnknownWorker
	}
	if err != nil {
		return fmt.Errorf("awssqssns: touch %s: %w", workerID, err)
	}
	return nil
}

// Delete removes the worker's registration set. A worker that is absent — or
// whose deadline already lapsed — is unknown.
func (s *DynamoRegistryStore) Delete(ctx context.Context, workerID string) error {
	_, err := s.db.DeleteItem(ctx, &dynamodb.DeleteItemInput{
		TableName:           aws.String(s.cfg.TableName),
		Key:                 pkKey(workerKeyPrefix + workerID),
		ConditionExpression: aws.String("attribute_exists(#pk) AND #d > :now"),
		ExpressionAttributeNames: map[string]string{
			"#pk": dynAttrPK,
			"#d":  dynAttrDeadline,
		},
		ExpressionAttributeValues: map[string]ddbtypes.AttributeValue{
			":now": avN(time.Now().UnixMilli()),
		},
	})
	if isConditionalCheckFailed(err) {
		return bl.ErrUnknownWorker
	}
	if err != nil {
		return fmt.Errorf("awssqssns: delete %s: %w", workerID, err)
	}
	return nil
}

// Snapshot returns every live registration (deadline in the future) across
// all workers.
func (s *DynamoRegistryStore) Snapshot(ctx context.Context) ([]bl.ProcessRegistration, error) {
	items, err := s.scanWorkers(ctx)
	if err != nil {
		return nil, err
	}
	now := time.Now()
	var regs []bl.ProcessRegistration
	for _, it := range items {
		if it.deadline.After(now) {
			regs = append(regs, it.regs...)
		}
	}
	return regs, nil
}

// Watch is a polling change feed: it emits a snapshot of live registrations,
// the SnapshotComplete sentinel, then diffs consecutive polls into
// Added/Removed/HeartbeatLost updates. A registration that vanishes with its
// deadline lapsed is HeartbeatLost; one deleted before its deadline is
// Removed. The channel closes when ctx is cancelled or the store closes.
func (s *DynamoRegistryStore) Watch(ctx context.Context) (<-chan bl.RegistryUpdate, error) {
	s.mu.Lock()
	if s.closed {
		s.mu.Unlock()
		return nil, bl.ErrBrokerClosed
	}
	s.mu.Unlock()

	ch := make(chan bl.RegistryUpdate, 16)
	s.wg.Add(1)
	go s.watch(ctx, ch)
	return ch, nil
}

// watchKey identifies one worker's registration of one process version.
type watchKey struct {
	workerID string
	key      bl.ProcessKey
}

func (s *DynamoRegistryStore) watch(ctx context.Context, ch chan<- bl.RegistryUpdate) {
	defer s.wg.Done()
	defer close(ch)

	send := func(u bl.RegistryUpdate) bool {
		select {
		case ch <- u:
			return true
		case <-ctx.Done():
			return false
		case <-s.ctx.Done():
			return false
		}
	}

	// Initial snapshot: retry the scan until it succeeds so a watcher opened
	// against a table still settling does not silently start empty.
	var items []workerItem
	for {
		its, err := s.scanWorkers(ctx)
		if err == nil {
			items = its
			break
		}
		select {
		case <-ctx.Done():
			return
		case <-s.ctx.Done():
			return
		case <-time.After(s.cfg.PollInterval):
		}
	}

	prev, prevDeadlines := liveView(items, time.Now())
	for k := range prev {
		reg := prev[k]
		if !send(bl.RegistryUpdate{Kind: bl.RegistryUpdateSnapshot, Registration: &reg}) {
			return
		}
	}
	if !send(bl.RegistryUpdate{Kind: bl.RegistryUpdateSnapshotComplete}) {
		return
	}

	ticker := time.NewTicker(s.cfg.PollInterval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-s.ctx.Done():
			return
		case <-ticker.C:
		}
		items, err := s.scanWorkers(ctx)
		if err != nil {
			continue
		}
		now := time.Now()
		cur, curDeadlines := liveView(items, now)

		for k := range cur {
			if _, ok := prev[k]; !ok {
				reg := cur[k]
				if !send(bl.RegistryUpdate{Kind: bl.RegistryUpdateAdded, Registration: &reg}) {
					return
				}
			}
		}
		// Expired-but-present items still show their (lapsed) deadline; for
		// items the sweeper already deleted, fall back to the deadline seen
		// on the previous poll.
		for k := range prev {
			if _, ok := cur[k]; ok {
				continue
			}
			kind := bl.RegistryUpdateRemoved
			deadline, seen := curDeadlines[k.workerID]
			if !seen {
				deadline = prevDeadlines[k.workerID]
			}
			if !deadline.After(now) {
				kind = bl.RegistryUpdateHeartbeatLost
			}
			reg := prev[k]
			if !send(bl.RegistryUpdate{Kind: kind, Registration: &reg}) {
				return
			}
		}
		prev, prevDeadlines = cur, curDeadlines
	}
}

// liveView flattens scanned worker items into a per-(worker, process key) map
// of live registrations plus every worker's deadline (live or not).
func liveView(items []workerItem, now time.Time) (map[watchKey]bl.ProcessRegistration, map[string]time.Time) {
	live := map[watchKey]bl.ProcessRegistration{}
	deadlines := map[string]time.Time{}
	for _, it := range items {
		deadlines[it.workerID] = it.deadline
		if !it.deadline.After(now) {
			continue
		}
		for _, reg := range it.regs {
			live[watchKey{workerID: it.workerID, key: reg.Key()}] = reg
		}
	}
	return live, deadlines
}

type workerItem struct {
	workerID string
	deadline time.Time
	beat     time.Time
	regs     []bl.ProcessRegistration
}

func (s *DynamoRegistryStore) scanWorkers(ctx context.Context) ([]workerItem, error) {
	var items []workerItem
	var start map[string]ddbtypes.AttributeValue
	for {
		out, err := s.db.Scan(ctx, &dynamodb.ScanInput{
			TableName:                aws.String(s.cfg.TableName),
			ConsistentRead:           aws.Bool(true),
			FilterExpression:         aws.String("begins_with(#pk, :p)"),
			ExpressionAttributeNames: map[string]string{"#pk": dynAttrPK},
			ExpressionAttributeValues: map[string]ddbtypes.AttributeValue{
				":p": avS(workerKeyPrefix),
			},
			ExclusiveStartKey: start,
		})
		if err != nil {
			return nil, fmt.Errorf("awssqssns: scan workers: %w", err)
		}
		for _, item := range out.Items {
			it, err := s.parseWorkerItem(item)
			if err != nil {
				return nil, err
			}
			items = append(items, it)
		}
		if len(out.LastEvaluatedKey) == 0 {
			return items, nil
		}
		start = out.LastEvaluatedKey
	}
}

func (s *DynamoRegistryStore) parseWorkerItem(item map[string]ddbtypes.AttributeValue) (workerItem, error) {
	it := workerItem{
		workerID: itemS(item, dynAttrWorkerID),
		deadline: time.UnixMilli(itemN(item, dynAttrDeadline)),
		beat:     time.UnixMilli(itemN(item, dynAttrBeat)),
	}
	list, _ := item[dynAttrRegs].(*ddbtypes.AttributeValueMemberL)
	if list == nil {
		return it, nil
	}
	for _, av := range list.Value {
		b, ok := av.(*ddbtypes.AttributeValueMemberB)
		if !ok {
			continue
		}
		env, err := bl.DecodeEnvelope(b.Value, s.cfg.Cipher)
		if err != nil {
			return it, fmt.Errorf("awssqssns: decode registration for %s: %w", it.workerID, err)
		}
		var reg bl.ProcessRegistration
		if err := env.DecodePayload(&reg); err != nil {
			return it, err
		}
		reg.LastHeartbeat = it.beat
		it.regs = append(it.regs, reg)
	}
	return it, nil
}

// ===== Timer records =====

// PutTimer stores a durable "deliver a JobResume at fireAt" record.
func (s *DynamoRegistryStore) PutTimer(ctx context.Context, instanceID string, fireAt time.Time) error {
	_, err := s.db.PutItem(ctx, &dynamodb.PutItemInput{
		TableName: aws.String(s.cfg.TableName),
		Item: map[string]ddbtypes.AttributeValue{
			dynAttrPK:        avS(timerKeyPrefix + instanceID),
			dynAttrFireAt:    avN(fireAt.UnixMilli()),
			dynAttrExpiresAt: avN(fireAt.Add(nativeTTLSlack).Unix()),
		},
	})
	if err != nil {
		return fmt.Errorf("awssqssns: put timer for %s: %w", instanceID, err)
	}
	return nil
}

// DueTimers returns — and atomically claims — the instanceIDs of timers due
// at now. The claim is a conditional delete on the observed fire time, so
// concurrent scheduler loops never claim the same timer twice.
func (s *DynamoRegistryStore) DueTimers(ctx context.Context, now time.Time) ([]string, error) {
	type candidate struct {
		instanceID string
		fireAt     int64
	}
	var candidates []candidate
	var start map[string]ddbtypes.AttributeValue
	for {
		out, err := s.db.Scan(ctx, &dynamodb.ScanInput{
			TableName:        aws.String(s.cfg.TableName),
			ConsistentRead:   aws.Bool(true),
			FilterExpression: aws.String("begins_with(#pk, :p) AND #f <= :now"),
			ExpressionAttributeNames: map[string]string{
				"#pk": dynAttrPK,
				"#f":  dynAttrFireAt,
			},
			ExpressionAttributeValues: map[string]ddbtypes.AttributeValue{
				":p":   avS(timerKeyPrefix),
				":now": avN(now.UnixMilli()),
			},
			ExclusiveStartKey: start,
		})
		if err != nil {
			return nil, fmt.Errorf("awssqssns: scan timers: %w", err)
		}
		for _, item := range out.Items {
			candidates = append(candidates, candidate{
				instanceID: itemS(item, dynAttrPK)[len(timerKeyPrefix):],
				fireAt:     itemN(item, dynAttrFireAt),
			})
		}
		if len(out.LastEvaluatedKey) == 0 {
			break
		}
		start = out.LastEvaluatedKey
	}

	var claimed []string
	for _, c := range candidates {
		_, err := s.db.DeleteItem(ctx, &dynamodb.DeleteItemInput{
			TableName:           aws.String(s.cfg.TableName),
			Key:                 pkKey(timerKeyPrefix + c.instanceID),
			ConditionExpression: aws.String("attribute_exists(#pk) AND #f = :f"),
			ExpressionAttributeNames: map[string]string{
				"#pk": dynAttrPK,
				"#f":  dynAttrFireAt,
			},
			ExpressionAttributeValues: map[string]ddbtypes.AttributeValue{
				":f": avN(c.fireAt),
			},
		})
		if isConditionalCheckFailed(err) {
			continue // another scheduler claimed it, or it was rescheduled
		}
		if err != nil {
			return claimed, fmt.Errorf("awssqssns: claim timer for %s: %w", c.instanceID, err)
		}
		claimed = append(claimed, c.instanceID)
	}
	return claimed, nil
}

// DeleteTimer removes the instance's timer record, if any.
func (s *DynamoRegistryStore) DeleteTimer(ctx context.Context, instanceID string) error {
	_, err := s.db.DeleteItem(ctx, &dynamodb.DeleteItemInput{
		TableName: aws.String(s.cfg.TableName),
		Key:       pkKey(timerKeyPrefix + instanceID),
	})
	if err != nil {
		return fmt.Errorf("awssqssns: delete timer for %s: %w", instanceID, err)
	}
	return nil
}

// ===== Last-event records (InstanceEventStore) =====

// InitInstance upserts the instance's routing metadata — process key and
// correlation key — when a Submit creates the instance.
func (s *DynamoRegistryStore) InitInstance(ctx context.Context, instanceID string, key bl.ProcessKey, correlationKey *string, retention time.Duration) error {
	update := "SET #ns = :ns, #proc = :proc, #ver = :ver, #e = :e"
	names := map[string]string{
		"#ns":   dynAttrNamespace,
		"#proc": dynAttrProcessID,
		"#ver":  dynAttrVersion,
		"#e":    dynAttrExpiresAt,
	}
	values := map[string]ddbtypes.AttributeValue{
		":ns":   avS(key.Namespace),
		":proc": avS(key.ProcessID),
		":ver":  avS(key.Version),
		":e":    avN(time.Now().Add(retention).Unix()),
	}
	if correlationKey != nil {
		update += ", #c = :c"
		names["#c"] = dynAttrCorr
		values[":c"] = avS(*correlationKey)
	}
	_, err := s.db.UpdateItem(ctx, &dynamodb.UpdateItemInput{
		TableName:                 aws.String(s.cfg.TableName),
		Key:                       pkKey(instKeyPrefix + instanceID),
		UpdateExpression:          aws.String(update),
		ExpressionAttributeNames:  names,
		ExpressionAttributeValues: values,
	})
	if err != nil {
		return fmt.Errorf("awssqssns: init instance %s: %w", instanceID, err)
	}
	return nil
}

// UpdateLastEvent atomically increments the instance's event sequence and
// upserts the latest lifecycle envelope, the terminal envelope, and the
// finished flag as supplied. Returns the new sequence number, which the
// broker stamps on the published message so subscribers can dedupe replayed
// events.
func (s *DynamoRegistryStore) UpdateLastEvent(ctx context.Context, instanceID string, latest, terminal []byte, finish bool, retention time.Duration) (int64, error) {
	update := "SET #e = :e"
	names := map[string]string{
		"#e": dynAttrExpiresAt,
		"#s": dynAttrSeq,
	}
	values := map[string]ddbtypes.AttributeValue{
		":e":   avN(time.Now().Add(retention).Unix()),
		":one": avN(1),
	}
	if latest != nil {
		update += ", #l = :l"
		names["#l"] = dynAttrLatest
		values[":l"] = avB(latest)
	}
	if terminal != nil {
		update += ", #t = :t"
		names["#t"] = dynAttrTerminal
		values[":t"] = avB(terminal)
	}
	if finish {
		update += ", #f = :f"
		names["#f"] = dynAttrFinished
		values[":f"] = &ddbtypes.AttributeValueMemberBOOL{Value: true}
	}
	update += " ADD #s :one"
	out, err := s.db.UpdateItem(ctx, &dynamodb.UpdateItemInput{
		TableName:                 aws.String(s.cfg.TableName),
		Key:                       pkKey(instKeyPrefix + instanceID),
		UpdateExpression:          aws.String(update),
		ExpressionAttributeNames:  names,
		ExpressionAttributeValues: values,
		ReturnValues:              ddbtypes.ReturnValueUpdatedNew,
	})
	if err != nil {
		return 0, fmt.Errorf("awssqssns: update last event for %s: %w", instanceID, err)
	}
	return itemN(out.Attributes, dynAttrSeq), nil
}

// GetLastEvent returns the instance's last-event record, or nil when the
// broker holds no events for it (unknown, or expired past retention).
func (s *DynamoRegistryStore) GetLastEvent(ctx context.Context, instanceID string) (*LastEventRecord, error) {
	out, err := s.db.GetItem(ctx, &dynamodb.GetItemInput{
		TableName:      aws.String(s.cfg.TableName),
		Key:            pkKey(instKeyPrefix + instanceID),
		ConsistentRead: aws.Bool(true),
	})
	if err != nil {
		return nil, fmt.Errorf("awssqssns: get last event for %s: %w", instanceID, err)
	}
	if len(out.Item) == 0 {
		return nil, nil
	}
	rec := &LastEventRecord{
		InstanceID: instanceID,
		Key: bl.ProcessKey{
			Namespace: itemS(out.Item, dynAttrNamespace),
			ProcessID: itemS(out.Item, dynAttrProcessID),
			Version:   itemS(out.Item, dynAttrVersion),
		},
		Seq:             itemN(out.Item, dynAttrSeq),
		LatestLifecycle: itemB(out.Item, dynAttrLatest),
		Terminal:        itemB(out.Item, dynAttrTerminal),
	}
	if c, ok := out.Item[dynAttrCorr].(*ddbtypes.AttributeValueMemberS); ok {
		corr := c.Value
		rec.CorrelationKey = &corr
	}
	if f, ok := out.Item[dynAttrFinished].(*ddbtypes.AttributeValueMemberBOOL); ok {
		rec.Finished = f.Value
	}
	return rec, nil
}

// ===== attribute-value helpers =====

func pkKey(pk string) map[string]ddbtypes.AttributeValue {
	return map[string]ddbtypes.AttributeValue{dynAttrPK: avS(pk)}
}

func avS(v string) ddbtypes.AttributeValue {
	return &ddbtypes.AttributeValueMemberS{Value: v}
}

func avN(v int64) ddbtypes.AttributeValue {
	return &ddbtypes.AttributeValueMemberN{Value: strconv.FormatInt(v, 10)}
}

func avB(v []byte) ddbtypes.AttributeValue {
	return &ddbtypes.AttributeValueMemberB{Value: v}
}

func itemS(item map[string]ddbtypes.AttributeValue, name string) string {
	if v, ok := item[name].(*ddbtypes.AttributeValueMemberS); ok {
		return v.Value
	}
	return ""
}

func itemN(item map[string]ddbtypes.AttributeValue, name string) int64 {
	if v, ok := item[name].(*ddbtypes.AttributeValueMemberN); ok {
		n, _ := strconv.ParseInt(v.Value, 10, 64)
		return n
	}
	return 0
}

func itemB(item map[string]ddbtypes.AttributeValue, name string) []byte {
	if v, ok := item[name].(*ddbtypes.AttributeValueMemberB); ok {
		return v.Value
	}
	return nil
}

func isConditionalCheckFailed(err error) bool {
	var cond *ddbtypes.ConditionalCheckFailedException
	return errors.As(err, &cond)
}
