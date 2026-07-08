// Package awssqssns implements blkit's MessageBroker against AWS SQS, SNS,
// and DynamoDB (specs/message-brokers/aws-sqs-sns-message-broker.spec.md):
// one SQS queue per process key carries jobs (the visibility timeout is the
// in-flight slot), an SNS FIFO topic with per-subscriber filtered SQS queues
// fans out instance events, and a RegistryStore backed by DynamoDB holds the
// worker registry, long timers, and last-event records for late-subscriber
// replay.
package awssqssns

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"strconv"
	"sync"
	"time"

	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/config"
	"github.com/aws/aws-sdk-go-v2/service/sns"
	snstypes "github.com/aws/aws-sdk-go-v2/service/sns/types"
	"github.com/aws/aws-sdk-go-v2/service/sqs"
	sqstypes "github.com/aws/aws-sdk-go-v2/service/sqs/types"

	bl "github.com/friendly-business-machines/blkit/core"
)

// SNS/SQS message attributes — cleartext routing metadata duplicated out of
// the envelope, per the wire-format spec.
const (
	msgAttrKind       = "kind"
	msgAttrInstanceID = "instanceID"
	msgAttrSeq        = "seq"   // per-instance event sequence, for replay dedupe
	msgAttrFinal      = "final" // set on the instance's last event; subscribers close after it
)

const (
	// maxSQSDelay is SQS's DelaySeconds ceiling; longer waits become timer
	// records driven by the scheduler loop.
	maxSQSDelay = 15 * time.Minute

	// timerPollInterval is the scheduler loop's DueTimers polling cadence.
	timerPollInterval = 200 * time.Millisecond

	// eventBufferSize is the per-subscriber event buffer. A full buffer drops
	// events and synthesizes BACKPRESSURE_DROP on recovery (overview spec
	// default).
	eventBufferSize = 64

	// receiveWaitSeconds is the SQS long-poll wait per ReceiveMessage.
	receiveWaitSeconds = 2
)

// InstanceEventStore is the last-event-record surface the broker needs beyond
// the core bl.RegistryStore: SNS retains nothing for late subscribers, so
// every lifecycle/terminal publish also upserts a per-instance record, and
// SubscribeToInstance replays from it. Config.Registry must implement it;
// DynamoRegistryStore does.
type InstanceEventStore interface {
	// InitInstance records the instance's process key and correlation key at
	// Submit, for RespondToInputRequest / timer routing and event stamping.
	InitInstance(ctx context.Context, instanceID string, key bl.ProcessKey, correlationKey *string, retention time.Duration) error

	// UpdateLastEvent bumps the instance's event sequence atomically and
	// stores the supplied envelope-encoded latest lifecycle event, terminal
	// event, and finished flag. Returns the new sequence.
	UpdateLastEvent(ctx context.Context, instanceID string, latest, terminal []byte, finish bool, retention time.Duration) (int64, error)

	// GetLastEvent returns the instance's record, or nil when unknown.
	GetLastEvent(ctx context.Context, instanceID string) (*LastEventRecord, error)
}

// LastEventRecord is the per-instance replay record: routing metadata plus
// the envelope-encoded latest lifecycle and terminal events.
type LastEventRecord struct {
	InstanceID      string
	Key             bl.ProcessKey
	CorrelationKey  *string
	Seq             int64  // sequence of the newest published event
	LatestLifecycle []byte // envelope-encoded latest lifecycle InstanceEvent
	Terminal        []byte // envelope-encoded terminal Result/Error InstanceEvent
	Finished        bool
}

// Config configures New.
type Config struct {
	Region      string                  // e.g. "eu-west-2"
	Credentials aws.CredentialsProvider // nil = default AWS credential chain
	QueuePrefix string                  // default "blkit"; isolates deployments sharing an account
	Registry    bl.RegistryStore        // required; must also implement InstanceEventStore (DynamoRegistryStore does)
	Endpoint    string                  // development only; LocalStack endpoint override
	Cipher      bl.PayloadCipher        // optional end-to-end payload encryption; default nil

	RegistrationTTL time.Duration // registration TTL; default 90s (3x the default heartbeat interval)
	InFlightTimeout time.Duration // job redelivery timeout = SQS visibility timeout; default 150s; rounded up to whole seconds
	EventRetention  time.Duration // last-event replay window; default 1h
}

// Broker is the AWS SQS/SNS MessageBroker backend. Construct with New; the
// same value serves both the producer and worker roles.
type Broker struct {
	cfg      Config
	sqs      *sqs.Client
	sns      *sns.Client
	registry bl.RegistryStore
	events   InstanceEventStore
	topicARN string

	ctx    context.Context
	cancel context.CancelFunc
	wg     sync.WaitGroup

	mu        sync.Mutex
	closed    bool
	queueURLs map[bl.ProcessKey]string
	inFlight  map[string][]*inFlightReceipt          // receipts per instanceID
	firstReg  map[string]map[bl.ProcessKey]time.Time // preserves RegisteredAt across re-registration
}

// inFlightReceipt tracks one delivered, unsettled job message: its queue and
// receipt handle for settling, and the cancel func of the lease goroutine
// that keeps extending its visibility.
type inFlightReceipt struct {
	key      bl.ProcessKey
	queueURL string
	handle   string
	stop     context.CancelFunc
}

// New connects to SQS and SNS, creates the instance-event topic, and starts
// the timer scheduler. Release its goroutines with Close. The caller owns
// cfg.Registry and closes it separately.
func New(cfg Config) (*Broker, error) {
	if cfg.Registry == nil {
		return nil, errors.New("awssqssns: Config.Registry is required")
	}
	events, ok := cfg.Registry.(InstanceEventStore)
	if !ok {
		return nil, errors.New("awssqssns: Config.Registry must implement InstanceEventStore (DynamoRegistryStore does)")
	}
	if cfg.Region == "" {
		return nil, errors.New("awssqssns: Config.Region is required")
	}
	if cfg.QueuePrefix == "" {
		cfg.QueuePrefix = "blkit"
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

	setupCtx, setupCancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer setupCancel()
	awsCfg, err := loadAWSConfig(setupCtx, cfg.Region, cfg.Credentials)
	if err != nil {
		return nil, fmt.Errorf("awssqssns: load AWS config: %w", err)
	}
	sqsClient := sqs.NewFromConfig(awsCfg, func(o *sqs.Options) {
		if cfg.Endpoint != "" {
			o.BaseEndpoint = aws.String(cfg.Endpoint)
		}
	})
	snsClient := sns.NewFromConfig(awsCfg, func(o *sns.Options) {
		if cfg.Endpoint != "" {
			o.BaseEndpoint = aws.String(cfg.Endpoint)
		}
	})

	// The instance-event fabric is a FIFO topic so each instance's events
	// (MessageGroupId = instanceID) arrive in publish order — terminal
	// lifecycle, then Result/Error, then channel close.
	topic, err := snsClient.CreateTopic(setupCtx, &sns.CreateTopicInput{
		Name:       aws.String(cfg.QueuePrefix + "-inst-events.fifo"),
		Attributes: map[string]string{"FifoTopic": "true"},
	})
	if err != nil {
		return nil, fmt.Errorf("awssqssns: create instance-event topic: %w", err)
	}

	ctx, cancel := context.WithCancel(context.Background())
	b := &Broker{
		cfg:       cfg,
		sqs:       sqsClient,
		sns:       snsClient,
		registry:  cfg.Registry,
		events:    events,
		topicARN:  aws.ToString(topic.TopicArn),
		ctx:       ctx,
		cancel:    cancel,
		queueURLs: map[bl.ProcessKey]string{},
		inFlight:  map[string][]*inFlightReceipt{},
		firstReg:  map[string]map[bl.ProcessKey]time.Time{},
	}
	b.wg.Add(1)
	go b.runTimerScheduler()
	return b, nil
}

func loadAWSConfig(ctx context.Context, region string, creds aws.CredentialsProvider) (aws.Config, error) {
	opts := []func(*config.LoadOptions) error{config.WithRegion(region)}
	if creds != nil {
		opts = append(opts, config.WithCredentialsProvider(creds))
	}
	return config.LoadDefaultConfig(ctx, opts...)
}

// Close stops the scheduler, fetch loops, lease goroutines, and subscriber
// pumps (closing their channels and deleting their queues), then fails
// subsequent calls with ErrBrokerClosed. Idempotent. Close does not close
// cfg.Registry — the caller owns it.
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
	return nil
}

func (b *Broker) checkOpen(ctx context.Context) error {
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

// mergedContext derives a context cancelled by ctx, the broker's Close, or
// the returned cancel func.
func (b *Broker) mergedContext(ctx context.Context) (context.Context, context.CancelFunc) {
	mctx, cancel := context.WithCancel(ctx)
	stop := context.AfterFunc(b.ctx, cancel)
	return mctx, func() { stop(); cancel() }
}

// ===== Producer side =====

// Submit resolves the process from the registry snapshot, validates the
// request against the registration's contracts, initializes the instance's
// last-event record, queues a JobStart, and publishes the Pending lifecycle
// event. The snapshot is read directly from the RegistryStore, so there is no
// cold-start wait: an unadvertised process is genuinely unknown.
func (b *Broker) Submit(ctx context.Context, req bl.StartRequest) (string, error) {
	if err := b.checkOpen(ctx); err != nil {
		return "", err
	}
	reg, err := b.resolveRegistration(ctx, req.Key())
	if err != nil {
		return "", err
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
	if err := b.events.InitInstance(ctx, instanceID, req.Key(), req.CorrelationKey, b.cfg.EventRetention); err != nil {
		return "", err
	}
	if err := b.sendJob(ctx, req.Key(), bl.Job{
		Kind: bl.JobStart,
		Key:  req.Key(),
		Start: &bl.StartJob{
			InstanceID:     instanceID,
			StartID:        req.StartID,
			Input:          req.Input,
			CorrelationKey: req.CorrelationKey,
		},
	}, 0); err != nil {
		return "", err
	}
	if err := b.publishEvent(ctx, instanceID, bl.InstanceEvent{
		Kind:      bl.InstanceEventLifecycle,
		Lifecycle: &bl.LifecycleChange{Phase: bl.ProcessStatusPending},
	}, eventDisposition{lifecycle: true}); err != nil {
		return "", err
	}
	return instanceID, nil
}

// Cancel publishes a JobCancel for a worker to honor. SQS cannot selectively
// remove a queued message (DeleteMessage needs a receipt handle only a
// receiver holds), so Cancel always takes the JobCancel route and always
// requires AllowExternalCancel.
func (b *Broker) Cancel(ctx context.Context, req bl.CancelRequest) error {
	if err := b.checkOpen(ctx); err != nil {
		return err
	}
	reg, err := b.resolveRegistration(ctx, req.Key())
	if err != nil {
		return err
	}
	if !reg.AllowExternalCancel {
		return bl.ErrCancelNotAllowed
	}
	return b.sendJob(ctx, req.Key(), bl.Job{
		Kind:   bl.JobCancel,
		Key:    req.Key(),
		Cancel: &bl.CancelJob{InstanceID: req.InstanceID, Reason: req.Reason},
	}, 0)
}

// Terminate publishes a JobTerminate. Requires AllowExternalTerminate.
func (b *Broker) Terminate(ctx context.Context, req bl.TerminateRequest) error {
	if err := b.checkOpen(ctx); err != nil {
		return err
	}
	reg, err := b.resolveRegistration(ctx, req.Key())
	if err != nil {
		return err
	}
	if !reg.AllowExternalTerminate {
		return bl.ErrTerminateNotAllowed
	}
	return b.sendJob(ctx, req.Key(), bl.Job{
		Kind:      bl.JobTerminate,
		Key:       req.Key(),
		Terminate: &bl.TerminateJob{InstanceID: req.InstanceID, Reason: req.Reason},
	}, 0)
}

// RespondToInputRequest queues a JobRespondToInput on the instance's process
// queue, routed via the last-event record. An unknown instance surfaces
// asynchronously as INSTANCE_NOT_FOUND on the instance's topic, per the
// error model.
func (b *Broker) RespondToInputRequest(ctx context.Context, instanceID, requestID string, payload map[string]any) error {
	if err := b.checkOpen(ctx); err != nil {
		return err
	}
	rec, err := b.events.GetLastEvent(ctx, instanceID)
	if err != nil {
		return err
	}
	if rec == nil || rec.Key == (bl.ProcessKey{}) {
		return b.publishEvent(ctx, instanceID, bl.InstanceEvent{
			Kind:  bl.InstanceEventError,
			Error: &bl.InstanceError{Code: bl.ErrCodeInstanceNotFound, Message: "respond-to-input for an instance the broker does not hold"},
		}, eventDisposition{})
	}
	if rec.Finished {
		// Mirrors the in-memory broker: the instance's subscriber channels
		// are already closed, so the ALREADY_FINISHED event would be
		// invisible; the response is simply dropped.
		return nil
	}
	return b.sendJob(ctx, rec.Key, bl.Job{
		Kind:           bl.JobRespondToInput,
		Key:            rec.Key,
		RespondToInput: &bl.RespondToInputJob{InstanceID: instanceID, RequestID: requestID, Payload: payload},
	}, 0)
}

// SubscribeToProcessRegistry delegates to the RegistryStore's polling change
// feed: a snapshot, the SnapshotComplete sentinel, then incremental updates
// until ctx is cancelled.
func (b *Broker) SubscribeToProcessRegistry(ctx context.Context) (<-chan bl.RegistryUpdate, error) {
	if err := b.checkOpen(ctx); err != nil {
		return nil, err
	}
	return b.registry.Watch(ctx)
}

// SubscribeToInstance creates a subscriber-owned FIFO queue, subscribes it to
// the instance-event topic with a filter policy on the instanceID attribute,
// replays the latest lifecycle / terminal event from the last-event record,
// then follows the queue live. The queue is deleted when the subscription
// ends. An instance with no record gets INSTANCE_NOT_FOUND as its first
// event; a finished instance replays and closes immediately.
func (b *Broker) SubscribeToInstance(ctx context.Context, instanceID string) (<-chan bl.InstanceEvent, error) {
	if err := b.checkOpen(ctx); err != nil {
		return nil, err
	}
	queueURL, subARN, err := b.createSubscriberQueue(ctx, instanceID)
	if err != nil {
		return nil, err
	}
	rec, err := b.events.GetLastEvent(ctx, instanceID)
	if err != nil {
		b.cleanupSubscription(subARN, queueURL)
		return nil, err
	}

	ch := make(chan bl.InstanceEvent, eventBufferSize)
	var replayedSeq int64
	if rec == nil {
		ch <- bl.InstanceEvent{
			InstanceID: instanceID,
			Kind:       bl.InstanceEventError,
			OccurredAt: time.Now(),
			Error:      &bl.InstanceError{Code: bl.ErrCodeInstanceNotFound, Message: "no events visible for this instance"},
		}
	} else {
		replayedSeq = rec.Seq
		if rec.LatestLifecycle != nil {
			if evt, err := b.decodeEventEnvelope(rec.LatestLifecycle); err == nil {
				ch <- evt
			}
		}
		if rec.Finished {
			if rec.Terminal != nil {
				if evt, err := b.decodeEventEnvelope(rec.Terminal); err == nil {
					ch <- evt
				}
			}
			close(ch)
			b.cleanupSubscription(subARN, queueURL)
			return ch, nil
		}
	}

	pumpCtx, pumpCancel := b.mergedContext(ctx)
	b.wg.Add(1)
	go b.pumpInstanceEvents(pumpCtx, pumpCancel, ch, instanceID, queueURL, subARN, replayedSeq)
	return ch, nil
}

// createSubscriberQueue creates a fresh FIFO queue and subscribes it to the
// instance-event topic with raw delivery and an instanceID filter policy.
func (b *Broker) createSubscriberQueue(ctx context.Context, instanceID string) (queueURL, subARN string, err error) {
	name := b.cfg.QueuePrefix + "-s-" + randHex(10) + ".fifo"
	q, err := b.sqs.CreateQueue(ctx, &sqs.CreateQueueInput{
		QueueName:  aws.String(name),
		Attributes: map[string]string{"FifoQueue": "true"},
	})
	if err != nil {
		return "", "", fmt.Errorf("awssqssns: create subscriber queue: %w", err)
	}
	queueURL = aws.ToString(q.QueueUrl)

	attrs, err := b.sqs.GetQueueAttributes(ctx, &sqs.GetQueueAttributesInput{
		QueueUrl:       q.QueueUrl,
		AttributeNames: []sqstypes.QueueAttributeName{sqstypes.QueueAttributeNameQueueArn},
	})
	if err != nil {
		b.cleanupSubscription("", queueURL)
		return "", "", fmt.Errorf("awssqssns: subscriber queue ARN: %w", err)
	}
	queueARN := attrs.Attributes[string(sqstypes.QueueAttributeNameQueueArn)]

	policy := fmt.Sprintf(`{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"Service":"sns.amazonaws.com"},"Action":"sqs:SendMessage","Resource":%q,"Condition":{"ArnEquals":{"aws:SourceArn":%q}}}]}`, queueARN, b.topicARN)
	if _, err := b.sqs.SetQueueAttributes(ctx, &sqs.SetQueueAttributesInput{
		QueueUrl:   q.QueueUrl,
		Attributes: map[string]string{string(sqstypes.QueueAttributeNamePolicy): policy},
	}); err != nil {
		b.cleanupSubscription("", queueURL)
		return "", "", fmt.Errorf("awssqssns: subscriber queue policy: %w", err)
	}

	filter, err := json.Marshal(map[string][]string{msgAttrInstanceID: {instanceID}})
	if err != nil {
		b.cleanupSubscription("", queueURL)
		return "", "", err
	}
	sub, err := b.sns.Subscribe(ctx, &sns.SubscribeInput{
		TopicArn: aws.String(b.topicARN),
		Protocol: aws.String("sqs"),
		Endpoint: aws.String(queueARN),
		Attributes: map[string]string{
			"RawMessageDelivery": "true",
			"FilterPolicy":       string(filter),
		},
		ReturnSubscriptionArn: true,
	})
	if err != nil {
		b.cleanupSubscription("", queueURL)
		return "", "", fmt.Errorf("awssqssns: subscribe queue to topic: %w", err)
	}
	return queueURL, aws.ToString(sub.SubscriptionArn), nil
}

// pumpInstanceEvents follows the subscriber queue, dedupes against the replay
// (and redeliveries) by sequence, applies the backpressure-drop policy, and
// closes the channel right after delivering the final event.
func (b *Broker) pumpInstanceEvents(ctx context.Context, cancel context.CancelFunc, ch chan bl.InstanceEvent, instanceID, queueURL, subARN string, replayedSeq int64) {
	defer b.wg.Done()
	defer cancel()
	defer b.cleanupSubscription(subARN, queueURL)
	defer close(ch)

	seen := map[int64]struct{}{}
	dropped := false
	for {
		if ctx.Err() != nil {
			return
		}
		out, err := b.sqs.ReceiveMessage(ctx, &sqs.ReceiveMessageInput{
			QueueUrl:              aws.String(queueURL),
			MaxNumberOfMessages:   10,
			WaitTimeSeconds:       receiveWaitSeconds,
			MessageAttributeNames: []string{"All"},
		})
		if err != nil {
			if ctx.Err() != nil {
				return
			}
			time.Sleep(100 * time.Millisecond)
			continue
		}
		for _, m := range out.Messages {
			evt, seq, final, decodeErr := b.decodeQueuedEvent(m)
			_, _ = b.sqs.DeleteMessage(ctx, &sqs.DeleteMessageInput{
				QueueUrl:      aws.String(queueURL),
				ReceiptHandle: m.ReceiptHandle,
			})
			if decodeErr != nil {
				// Undecodable/undecryptable payloads are never processed;
				// surface the failure to the subscriber instead.
				deliver(ch, &dropped, bl.InstanceEvent{
					InstanceID: instanceID,
					Kind:       bl.InstanceEventError,
					OccurredAt: time.Now(),
					Error:      &bl.InstanceError{Code: "DECRYPTION_ERROR", Message: decodeErr.Error()},
				})
				continue
			}
			if seq > 0 {
				if seq <= replayedSeq {
					continue // already covered by the replay
				}
				if _, dup := seen[seq]; dup {
					continue // at-least-once redelivery
				}
				seen[seq] = struct{}{}
			}
			deliver(ch, &dropped, evt)
			if final {
				return
			}
		}
	}
}

// deliver applies the overview spec's backpressure default: a full buffer
// drops the event, and a synthetic BACKPRESSURE_DROP is delivered when the
// buffer recovers.
func deliver(ch chan<- bl.InstanceEvent, dropped *bool, evt bl.InstanceEvent) {
	if *dropped {
		synthetic := bl.InstanceEvent{
			InstanceID: evt.InstanceID,
			Kind:       bl.InstanceEventError,
			OccurredAt: time.Now(),
			Error:      &bl.InstanceError{Code: bl.ErrCodeBackpressureDrop, Message: "subscriber buffer overflowed; events were dropped"},
		}
		select {
		case ch <- synthetic:
			*dropped = false
		default:
			return // still full; this event drops too
		}
	}
	select {
	case ch <- evt:
	default:
		*dropped = true
	}
}

func (b *Broker) decodeQueuedEvent(m sqstypes.Message) (evt bl.InstanceEvent, seq int64, final bool, err error) {
	evt, err = b.decodeEventEnvelope([]byte(aws.ToString(m.Body)))
	if err != nil {
		return bl.InstanceEvent{}, 0, false, err
	}
	if a, ok := m.MessageAttributes[msgAttrSeq]; ok {
		seq, _ = strconv.ParseInt(aws.ToString(a.StringValue), 10, 64)
	}
	_, final = m.MessageAttributes[msgAttrFinal]
	return evt, seq, final, nil
}

// decodeEventEnvelope decodes a base64'd, envelope-wrapped InstanceEvent.
func (b *Broker) decodeEventEnvelope(data []byte) (bl.InstanceEvent, error) {
	raw, err := base64.StdEncoding.DecodeString(string(data))
	if err != nil {
		return bl.InstanceEvent{}, fmt.Errorf("awssqssns: decode event body: %w", err)
	}
	env, err := bl.DecodeEnvelope(raw, b.cfg.Cipher)
	if err != nil {
		return bl.InstanceEvent{}, err
	}
	var evt bl.InstanceEvent
	if err := env.DecodePayload(&evt); err != nil {
		return bl.InstanceEvent{}, err
	}
	return evt, nil
}

// cleanupSubscription tears down a subscriber's SNS subscription and queue.
// Runs on a background context so it works during Close and after ctx
// cancellation.
func (b *Broker) cleanupSubscription(subARN, queueURL string) {
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	if subARN != "" {
		_, _ = b.sns.Unsubscribe(ctx, &sns.UnsubscribeInput{SubscriptionArn: aws.String(subARN)})
	}
	if queueURL != "" {
		_, _ = b.sqs.DeleteQueue(ctx, &sqs.DeleteQueueInput{QueueUrl: aws.String(queueURL)})
	}
}

// resolveRegistration returns one live registration for the key from the
// RegistryStore snapshot.
func (b *Broker) resolveRegistration(ctx context.Context, key bl.ProcessKey) (*bl.ProcessRegistration, error) {
	regs, err := b.registry.Snapshot(ctx)
	if err != nil {
		return nil, err
	}
	for i := range regs {
		if regs[i].Key() == key {
			return &regs[i], nil
		}
	}
	return nil, bl.ErrUnknownProcess
}

// ===== Worker side: registration =====

// RegisterProcesses stamps registry metadata (WorkerID, RegisteredAt
// preserved per key across re-registration, LastHeartbeat) and replaces the
// worker's registration set in the RegistryStore.
func (b *Broker) RegisterProcesses(ctx context.Context, workerID string, regs []bl.ProcessRegistration) error {
	if err := b.checkOpen(ctx); err != nil {
		return err
	}
	now := time.Now()
	b.mu.Lock()
	first := b.firstReg[workerID]
	if first == nil {
		first = map[bl.ProcessKey]time.Time{}
		b.firstReg[workerID] = first
	}
	stamped := make([]bl.ProcessRegistration, len(regs))
	for i, r := range regs {
		r.WorkerID = workerID
		registeredAt, ok := first[r.Key()]
		if !ok {
			registeredAt = now
			first[r.Key()] = registeredAt
		}
		r.RegisteredAt = registeredAt
		r.LastHeartbeat = now
		stamped[i] = r
	}
	b.mu.Unlock()
	return b.registry.PutRegistrations(ctx, workerID, stamped, b.cfg.RegistrationTTL)
}

// Heartbeat refreshes the worker's registration TTL.
func (b *Broker) Heartbeat(ctx context.Context, workerID string) error {
	if err := b.checkOpen(ctx); err != nil {
		return err
	}
	return b.registry.Touch(ctx, workerID, b.cfg.RegistrationTTL)
}

// Unregister removes the worker's registrations immediately.
func (b *Broker) Unregister(ctx context.Context, workerID string) error {
	if err := b.checkOpen(ctx); err != nil {
		return err
	}
	if err := b.registry.Delete(ctx, workerID); err != nil {
		return err
	}
	b.mu.Lock()
	delete(b.firstReg, workerID)
	b.mu.Unlock()
	return nil
}

// ===== Worker side: job queue =====

// FetchJobs long-polls the jobs queue of every requested key (created
// idempotently on demand) and yields decoded jobs. Delivery starts a lease
// goroutine that keeps extending the message's visibility until a Report*
// verb settles it; a worker that dies simply stops extending, the visibility
// lapses, and SQS redelivers. Closes when ctx is cancelled.
func (b *Broker) FetchJobs(ctx context.Context, keys []bl.ProcessKey) (<-chan bl.Job, error) {
	if err := b.checkOpen(ctx); err != nil {
		return nil, err
	}
	urls := make(map[bl.ProcessKey]string, len(keys))
	for _, key := range keys {
		url, err := b.ensureJobsQueue(ctx, key)
		if err != nil {
			return nil, err
		}
		urls[key] = url
	}

	out := make(chan bl.Job)
	loopCtx, cancel := b.mergedContext(ctx)
	var receivers sync.WaitGroup
	for key, url := range urls {
		receivers.Add(1)
		go b.receiveJobs(loopCtx, &receivers, key, url, out)
	}
	b.wg.Add(1)
	go func() {
		defer b.wg.Done()
		receivers.Wait() // immediate for empty keys: idle-wait on ctx below
		<-loopCtx.Done()
		cancel()
		close(out)
	}()
	return out, nil
}

func (b *Broker) receiveJobs(ctx context.Context, wg *sync.WaitGroup, key bl.ProcessKey, queueURL string, out chan<- bl.Job) {
	defer wg.Done()
	for {
		if ctx.Err() != nil {
			return
		}
		res, err := b.sqs.ReceiveMessage(ctx, &sqs.ReceiveMessageInput{
			QueueUrl:            aws.String(queueURL),
			MaxNumberOfMessages: 10,
			WaitTimeSeconds:     receiveWaitSeconds,
		})
		if err != nil {
			if ctx.Err() != nil {
				return
			}
			time.Sleep(100 * time.Millisecond)
			continue
		}
		for _, m := range res.Messages {
			job, err := b.decodeJob(aws.ToString(m.Body))
			if err != nil {
				// Poison message: drop it rather than redeliver forever.
				_, _ = b.sqs.DeleteMessage(ctx, &sqs.DeleteMessageInput{
					QueueUrl:      aws.String(queueURL),
					ReceiptHandle: m.ReceiptHandle,
				})
				continue
			}
			receipt := b.trackInFlight(ctx, key, queueURL, aws.ToString(m.ReceiptHandle), job.InstanceID())
			select {
			case out <- job:
			case <-ctx.Done():
				receipt.stop() // stop the lease; visibility lapses and SQS redelivers
				return
			}
		}
	}
}

// trackInFlight records the delivered message's receipt for settling and
// starts its visibility lease: ChangeMessageVisibility on a half-timeout
// cadence until the job settles or the fetch context dies.
func (b *Broker) trackInFlight(fetchCtx context.Context, key bl.ProcessKey, queueURL, handle, instanceID string) *inFlightReceipt {
	leaseCtx, stop := context.WithCancel(fetchCtx)
	r := &inFlightReceipt{key: key, queueURL: queueURL, handle: handle, stop: stop}
	b.mu.Lock()
	b.inFlight[instanceID] = append(b.inFlight[instanceID], r)
	b.mu.Unlock()

	visibility := visibilitySeconds(b.cfg.InFlightTimeout)
	interval := b.cfg.InFlightTimeout / 2
	if interval < 200*time.Millisecond {
		interval = 200 * time.Millisecond
	}
	b.wg.Add(1)
	go func() {
		defer b.wg.Done()
		ticker := time.NewTicker(interval)
		defer ticker.Stop()
		for {
			select {
			case <-leaseCtx.Done():
				return
			case <-ticker.C:
				_, err := b.sqs.ChangeMessageVisibility(leaseCtx, &sqs.ChangeMessageVisibilityInput{
					QueueUrl:          aws.String(queueURL),
					ReceiptHandle:     aws.String(handle),
					VisibilityTimeout: visibility,
				})
				if err != nil {
					return // handle expired or consumed; the lease is over either way
				}
			}
		}
	}()
	return r
}

// settleInstance deletes every in-flight message held for the instance and
// stops their leases. Returns the process key of one settled message, when
// any.
func (b *Broker) settleInstance(ctx context.Context, instanceID string) *bl.ProcessKey {
	b.mu.Lock()
	receipts := b.inFlight[instanceID]
	delete(b.inFlight, instanceID)
	b.mu.Unlock()
	var key *bl.ProcessKey
	for _, r := range receipts {
		r.stop()
		_, _ = b.sqs.DeleteMessage(ctx, &sqs.DeleteMessageInput{
			QueueUrl:      aws.String(r.queueURL),
			ReceiptHandle: aws.String(r.handle),
		})
		k := r.key
		key = &k
	}
	return key
}

// ===== Worker side: lifecycle reports and instance topic =====

// ReportRunning publishes the Running lifecycle event. Does not settle the
// in-flight job.
func (b *Broker) ReportRunning(ctx context.Context, instanceID string) error {
	if err := b.checkOpen(ctx); err != nil {
		return err
	}
	return b.publishEvent(ctx, instanceID, bl.InstanceEvent{
		Kind:      bl.InstanceEventLifecycle,
		Lifecycle: &bl.LifecycleChange{Phase: bl.ProcessStatusRunning},
	}, eventDisposition{lifecycle: true})
}

// ReportSuspended publishes the Suspended lifecycle event, settles the
// in-flight job, and — given a resumeAt — schedules the JobResume: SQS
// DelaySeconds when the wait fits its 15-minute ceiling, otherwise a timer
// record claimed by the scheduler loop. A nil resumeAt means the instance
// waits for an external signal (typically a RespondToInputRequest).
func (b *Broker) ReportSuspended(ctx context.Context, instanceID string, resumeAt *time.Time) error {
	if err := b.checkOpen(ctx); err != nil {
		return err
	}
	if err := b.publishEvent(ctx, instanceID, bl.InstanceEvent{
		Kind:      bl.InstanceEventLifecycle,
		Lifecycle: &bl.LifecycleChange{Phase: bl.ProcessStatusSuspended},
	}, eventDisposition{lifecycle: true}); err != nil {
		return err
	}
	key := b.settleInstance(ctx, instanceID)
	if resumeAt == nil {
		return nil
	}
	if key == nil {
		rec, err := b.events.GetLastEvent(ctx, instanceID)
		if err != nil {
			return err
		}
		if rec == nil || rec.Key == (bl.ProcessKey{}) {
			return fmt.Errorf("awssqssns: cannot schedule resume: no process key for instance %s", instanceID)
		}
		key = &rec.Key
	}
	delay := time.Until(*resumeAt)
	if delay > maxSQSDelay {
		return b.registry.PutTimer(ctx, instanceID, *resumeAt)
	}
	var delaySeconds int32
	if delay >= time.Second {
		delaySeconds = int32(math.Ceil(delay.Seconds()))
	}
	return b.sendJob(ctx, *key, bl.Job{
		Kind:   bl.JobResume,
		Key:    *key,
		Resume: &bl.ResumeJob{InstanceID: instanceID},
	}, delaySeconds)
}

// ReportCompleted publishes the Completed lifecycle event and the final
// Result event, settles the in-flight job, and closes subscriber channels.
func (b *Broker) ReportCompleted(ctx context.Context, instanceID string, result bl.EvaluationResult) error {
	if err := b.checkOpen(ctx); err != nil {
		return err
	}
	if err := b.publishEvent(ctx, instanceID, bl.InstanceEvent{
		Kind:      bl.InstanceEventLifecycle,
		Lifecycle: &bl.LifecycleChange{Phase: bl.ProcessStatusCompleted},
	}, eventDisposition{lifecycle: true}); err != nil {
		return err
	}
	if err := b.publishEvent(ctx, instanceID, bl.InstanceEvent{
		Kind:   bl.InstanceEventResult,
		Result: &result,
	}, eventDisposition{terminal: true, final: true}); err != nil {
		return err
	}
	b.settleInstance(ctx, instanceID)
	return nil
}

// ReportFailed publishes the Failed lifecycle event and the final Error
// event, settles the in-flight job, and closes subscriber channels.
func (b *Broker) ReportFailed(ctx context.Context, instanceID string, instErr bl.InstanceError) error {
	if err := b.checkOpen(ctx); err != nil {
		return err
	}
	if err := b.publishEvent(ctx, instanceID, bl.InstanceEvent{
		Kind:      bl.InstanceEventLifecycle,
		Lifecycle: &bl.LifecycleChange{Phase: bl.ProcessStatusFailed},
	}, eventDisposition{lifecycle: true}); err != nil {
		return err
	}
	if err := b.publishEvent(ctx, instanceID, bl.InstanceEvent{
		Kind:  bl.InstanceEventError,
		Error: &instErr,
	}, eventDisposition{terminal: true, final: true}); err != nil {
		return err
	}
	b.settleInstance(ctx, instanceID)
	return nil
}

// ReportCancelled publishes the terminal Cancelled lifecycle event, settles
// the in-flight job, and closes subscriber channels.
func (b *Broker) ReportCancelled(ctx context.Context, instanceID string) error {
	if err := b.checkOpen(ctx); err != nil {
		return err
	}
	if err := b.publishEvent(ctx, instanceID, bl.InstanceEvent{
		Kind:      bl.InstanceEventLifecycle,
		Lifecycle: &bl.LifecycleChange{Phase: bl.ProcessStatusCancelled},
	}, eventDisposition{lifecycle: true, final: true}); err != nil {
		return err
	}
	b.settleInstance(ctx, instanceID)
	return nil
}

// PostError publishes a non-terminal error event.
func (b *Broker) PostError(ctx context.Context, instanceID string, instErr bl.InstanceError) error {
	if err := b.checkOpen(ctx); err != nil {
		return err
	}
	return b.publishEvent(ctx, instanceID, bl.InstanceEvent{
		Kind:  bl.InstanceEventError,
		Error: &instErr,
	}, eventDisposition{})
}

// PostInputRequest publishes a RequestInputTask's request for input.
func (b *Broker) PostInputRequest(ctx context.Context, instanceID string, req bl.InputRequest) error {
	if err := b.checkOpen(ctx); err != nil {
		return err
	}
	return b.publishEvent(ctx, instanceID, bl.InstanceEvent{
		Kind:         bl.InstanceEventInputRequest,
		InputRequest: &req,
	}, eventDisposition{})
}

// PostNodeCompleted publishes a node-completion event.
func (b *Broker) PostNodeCompleted(ctx context.Context, instanceID string, nc bl.NodeCompleted) error {
	if err := b.checkOpen(ctx); err != nil {
		return err
	}
	return b.publishEvent(ctx, instanceID, bl.InstanceEvent{
		Kind:          bl.InstanceEventNodeCompleted,
		NodeCompleted: &nc,
	}, eventDisposition{})
}

// ===== Event publishing =====

// eventDisposition says how a published event lands in the last-event record
// and on the wire.
type eventDisposition struct {
	lifecycle bool // store the envelope as the record's latest lifecycle event
	terminal  bool // store the envelope as the record's terminal Result/Error event
	final     bool // the instance's last event: sets the finished flag and the final wire attribute
}

// publishEvent stamps the event, upserts the last-event record (allocating
// the event's sequence number), and publishes it on the instance-event topic.
// Events for an already-finished instance are dropped — its subscriber
// channels are closed and late subscribers replay-and-close, exactly as in
// the in-memory broker.
func (b *Broker) publishEvent(ctx context.Context, instanceID string, evt bl.InstanceEvent, d eventDisposition) error {
	rec, err := b.events.GetLastEvent(ctx, instanceID)
	if err != nil {
		return err
	}
	if rec != nil && rec.Finished {
		return nil
	}
	evt.InstanceID = instanceID
	evt.OccurredAt = time.Now()
	if rec != nil {
		evt.CorrelationKey = rec.CorrelationKey
	}
	env, err := bl.EncodeEnvelope(bl.KindInstanceEvent, instanceID, evt.CorrelationKey, evt, b.cfg.Cipher)
	if err != nil {
		return err
	}
	body := base64.StdEncoding.EncodeToString(env)

	var latest, terminal []byte
	if d.lifecycle {
		latest = []byte(body)
	}
	if d.terminal {
		terminal = []byte(body)
	}
	seq, err := b.events.UpdateLastEvent(ctx, instanceID, latest, terminal, d.final, b.cfg.EventRetention)
	if err != nil {
		return err
	}

	attrs := map[string]snstypes.MessageAttributeValue{
		msgAttrKind:       {DataType: aws.String("String"), StringValue: aws.String(bl.KindInstanceEvent)},
		msgAttrInstanceID: {DataType: aws.String("String"), StringValue: aws.String(instanceID)},
		msgAttrSeq:        {DataType: aws.String("Number"), StringValue: aws.String(strconv.FormatInt(seq, 10))},
	}
	if d.final {
		attrs[msgAttrFinal] = snstypes.MessageAttributeValue{DataType: aws.String("String"), StringValue: aws.String("1")}
	}
	_, err = b.sns.Publish(ctx, &sns.PublishInput{
		TopicArn:               aws.String(b.topicARN),
		Message:                aws.String(body),
		MessageAttributes:      attrs,
		MessageGroupId:         aws.String(instanceID),
		MessageDeduplicationId: aws.String(instanceID + "-" + strconv.FormatInt(seq, 10)),
	})
	if err != nil {
		return fmt.Errorf("awssqssns: publish instance event: %w", err)
	}
	return nil
}

// ===== Jobs queue helpers =====

var jobKindNames = map[bl.JobKind]string{
	bl.JobStart:          bl.KindJobStart,
	bl.JobRespondToInput: bl.KindJobRespondToInput,
	bl.JobCancel:         bl.KindJobCancel,
	bl.JobTerminate:      bl.KindJobTerminate,
	bl.JobResume:         bl.KindJobResume,
}

// sendJob envelope-encodes the job and sends it to its key's queue, creating
// the queue idempotently on demand.
func (b *Broker) sendJob(ctx context.Context, key bl.ProcessKey, job bl.Job, delaySeconds int32) error {
	queueURL, err := b.ensureJobsQueue(ctx, key)
	if err != nil {
		return err
	}
	data, err := bl.EncodeEnvelope(jobKindNames[job.Kind], job.InstanceID(), nil, job, b.cfg.Cipher)
	if err != nil {
		return err
	}
	_, err = b.sqs.SendMessage(ctx, &sqs.SendMessageInput{
		QueueUrl:     aws.String(queueURL),
		MessageBody:  aws.String(base64.StdEncoding.EncodeToString(data)),
		DelaySeconds: delaySeconds,
		MessageAttributes: map[string]sqstypes.MessageAttributeValue{
			msgAttrKind:       {DataType: aws.String("String"), StringValue: aws.String(jobKindNames[job.Kind])},
			msgAttrInstanceID: {DataType: aws.String("String"), StringValue: aws.String(job.InstanceID())},
		},
	})
	if err != nil {
		return fmt.Errorf("awssqssns: send %s job: %w", jobKindNames[job.Kind], err)
	}
	return nil
}

func (b *Broker) decodeJob(body string) (bl.Job, error) {
	raw, err := base64.StdEncoding.DecodeString(body)
	if err != nil {
		return bl.Job{}, fmt.Errorf("awssqssns: decode job body: %w", err)
	}
	env, err := bl.DecodeEnvelope(raw, b.cfg.Cipher)
	if err != nil {
		return bl.Job{}, err
	}
	var job bl.Job
	if err := env.DecodePayload(&job); err != nil {
		return bl.Job{}, err
	}
	return job, nil
}

// ensureJobsQueue creates (idempotently) and caches the jobs queue for a
// process key. The queue's visibility timeout is the in-flight timeout,
// rounded up to whole seconds.
func (b *Broker) ensureJobsQueue(ctx context.Context, key bl.ProcessKey) (string, error) {
	b.mu.Lock()
	if url, ok := b.queueURLs[key]; ok {
		b.mu.Unlock()
		return url, nil
	}
	b.mu.Unlock()

	name := jobsQueueName(b.cfg.QueuePrefix, key)
	out, err := b.sqs.CreateQueue(ctx, &sqs.CreateQueueInput{
		QueueName: aws.String(name),
		Attributes: map[string]string{
			string(sqstypes.QueueAttributeNameVisibilityTimeout): strconv.Itoa(int(visibilitySeconds(b.cfg.InFlightTimeout))),
		},
	})
	if err != nil {
		return "", fmt.Errorf("awssqssns: create jobs queue %s: %w", name, err)
	}
	url := aws.ToString(out.QueueUrl)
	b.mu.Lock()
	b.queueURLs[key] = url
	b.mu.Unlock()
	return url, nil
}

// jobsQueueName derives a valid SQS queue name (alphanumerics, hyphens,
// underscores, max 80 chars) from a process key. Keys contain characters SQS
// forbids ('/', '.'), so the name pairs a sanitized slug with a hash of the
// full key for uniqueness.
func jobsQueueName(prefix string, key bl.ProcessKey) string {
	sum := sha256.Sum256([]byte(key.Namespace + "\x00" + key.ProcessID + "\x00" + key.Version))
	slug := sanitizeQueuePart(key.ProcessID)
	if len(slug) > 24 {
		slug = slug[:24]
	}
	name := prefix + "-jobs-" + slug + "-" + hex.EncodeToString(sum[:8])
	if len(name) > 80 {
		name = name[len(name)-80:]
	}
	return name
}

func sanitizeQueuePart(s string) string {
	out := []byte(s)
	for i, c := range out {
		switch {
		case c >= 'a' && c <= 'z', c >= 'A' && c <= 'Z', c >= '0' && c <= '9', c == '-', c == '_':
		default:
			out[i] = '-'
		}
	}
	return string(out)
}

// visibilitySeconds rounds the in-flight timeout UP to whole seconds (SQS
// visibility is integer seconds), minimum 1.
func visibilitySeconds(d time.Duration) int32 {
	s := int32(math.Ceil(d.Seconds()))
	if s < 1 {
		s = 1
	}
	return s
}

// ===== Timer scheduler =====

// runTimerScheduler polls DueTimers and turns each claimed timer into a
// JobResume on the instance's jobs queue. Only suspends longer than SQS's
// 15-minute DelaySeconds ceiling land here.
func (b *Broker) runTimerScheduler() {
	defer b.wg.Done()
	ticker := time.NewTicker(timerPollInterval)
	defer ticker.Stop()
	for {
		select {
		case <-b.ctx.Done():
			return
		case <-ticker.C:
			ids, err := b.registry.DueTimers(b.ctx, time.Now())
			if err != nil {
				continue
			}
			for _, instanceID := range ids {
				rec, err := b.events.GetLastEvent(b.ctx, instanceID)
				if err != nil || rec == nil || rec.Finished || rec.Key == (bl.ProcessKey{}) {
					continue
				}
				_ = b.sendJob(b.ctx, rec.Key, bl.Job{
					Kind:   bl.JobResume,
					Key:    rec.Key,
					Resume: &bl.ResumeJob{InstanceID: instanceID},
				}, 0)
			}
		}
	}
}

func randHex(n int) string {
	buf := make([]byte, n)
	if _, err := rand.Read(buf); err != nil {
		panic(err) // crypto/rand never fails on supported platforms
	}
	return hex.EncodeToString(buf)
}
