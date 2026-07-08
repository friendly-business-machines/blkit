package core

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"time"
)

// MessageBroker is how blkit's clients and workers talk to each other
// (specs/message-brokers/overview.spec.md). It presents a producer-side API
// (Submit, Cancel, Terminate, RespondToInputRequest, SubscribeToInstance,
// SubscribeToProcessRegistry) and a worker-side API (RegisterProcesses,
// Heartbeat, Unregister, FetchJobs, the Report* lifecycle verbs, and the
// Post* instance-topic verbs). The same implementation satisfies both roles.
//
// The broker holds no process state: execution history and current/historical
// state live only in the state store. Lifecycle events on the instance topic
// — not any broker-held record — are how clients tell where an instance is.
type MessageBroker interface {
	// ===== Producer-side =====

	// Submit a new process run. The broker resolves the process from its
	// registry snapshot, resolves StartID, validates Input against the
	// registration's InputContract, generates the instance id, publishes a
	// JobStart and a Pending lifecycle event, and returns the id. If the
	// registry snapshot has not arrived yet (cold start), Submit blocks
	// until it does, bounded by ctx.
	//
	// Synchronous errors: ErrUnknownProcess, ErrUnknownStartID,
	// *DataContractValidationError, broker-publish errors.
	Submit(ctx context.Context, req StartRequest) (instanceID string, err error)

	// Cancel is fire-and-forget. If the JobStart is still findable in the
	// queue it is removed (no opt-in required) and the broker itself
	// publishes the terminal Cancelled lifecycle event; otherwise a
	// JobCancel is published, which requires AllowExternalCancel on the
	// registration (else ErrCancelNotAllowed). Whether the instance is
	// already finished is not checked here — a worker that receives a
	// JobCancel for a finished instance posts ALREADY_FINISHED.
	//
	// Other synchronous errors: ErrUnknownProcess, broker-publish errors.
	Cancel(ctx context.Context, req CancelRequest) error

	// Terminate is fire-and-forget: publish a JobTerminate for a worker to
	// honor. Requires AllowExternalTerminate on the registration.
	//
	// Synchronous errors: ErrUnknownProcess, ErrTerminateNotAllowed,
	// broker-publish errors.
	Terminate(ctx context.Context, req TerminateRequest) error

	// RespondToInputRequest delivers input to a process waiting on a
	// RequestInputTask, keyed by the requestID emitted on the instance's
	// topic when the task fired. Whether the instance exists or is waiting
	// on the given requestID is not checked at the broker — those errors
	// flow back as InstanceEvents of kind Error.
	RespondToInputRequest(ctx context.Context, instanceID, requestID string, payload map[string]any) error

	// SubscribeToInstance subscribes to events for a single process
	// instance. The channel closes when the instance enters a finished
	// status or ctx is cancelled. A late subscriber first receives the
	// instance's latest lifecycle event — and the terminal event, if the
	// instance already finished within the backend's retention window.
	SubscribeToInstance(ctx context.Context, instanceID string) (<-chan InstanceEvent, error)

	// SubscribeToProcessRegistry subscribes to the process registry. The
	// first messages are a full snapshot (one RegistryUpdateSnapshot per
	// live registration, then RegistryUpdateSnapshotComplete); subsequent
	// messages are incremental changes. Closes only on ctx cancellation.
	SubscribeToProcessRegistry(ctx context.Context) (<-chan RegistryUpdate, error)

	// ===== Worker-side: registration lifecycle =====

	// RegisterProcesses registers this worker's capability set — which
	// processes and versions it can execute, including start-event input
	// contracts so producers can validate Submits. Each call replaces any
	// prior registration set for the same workerID. Idempotent.
	RegisterProcesses(ctx context.Context, workerID string, regs []ProcessRegistration) error

	// Heartbeat refreshes the TTL on this worker's registrations.
	//
	// Synchronous errors: ErrUnknownWorker, broker-publish errors.
	Heartbeat(ctx context.Context, workerID string) error

	// Unregister removes this worker's registrations on graceful shutdown.
	//
	// Synchronous errors: ErrUnknownWorker, broker-publish errors.
	Unregister(ctx context.Context, workerID string) error

	// ===== Worker-side: job queue =====

	// FetchJobs returns a channel of jobs targeted at processes in keys.
	// The broker keeps a delivered job in-flight until the worker calls a
	// terminal Report* verb or ReportSuspended for the instance; a worker
	// that dies loses the job to redelivery after the in-flight timeout.
	// Closes when ctx is cancelled.
	FetchJobs(ctx context.Context, keys []ProcessKey) (<-chan Job, error)

	// ===== Worker-side: lifecycle reports =====
	//
	// Each report publishes a Lifecycle event on the instance's topic.
	// Terminal reports and ReportSuspended also settle the in-flight job.
	// The worker persists the authoritative status to the state store
	// separately, before reporting.

	// ReportRunning is the first verb a worker calls after fetching a
	// JobStart or JobResume. Does NOT settle the in-flight job.
	ReportRunning(ctx context.Context, instanceID string) error

	// ReportSuspended: the process suspended. Publishes the Suspended
	// lifecycle event and settles the in-flight job. A non-nil resumeAt
	// schedules a JobResume for that time (Suspend*/Pause* waits); with a
	// nil resumeAt the instance waits for an external signal — typically a
	// RespondToInputRequest, which arrives as its own job.
	ReportSuspended(ctx context.Context, instanceID string, resumeAt *time.Time) error

	// Terminal outcomes. Each publishes the corresponding lifecycle event
	// (plus the Result / Error event), settles the in-flight job, and
	// closes SubscribeToInstance channels after the final event delivers.
	ReportCompleted(ctx context.Context, instanceID string, result EvaluationResult) error
	ReportFailed(ctx context.Context, instanceID string, instErr InstanceError) error
	ReportCancelled(ctx context.Context, instanceID string) error

	// ===== Worker-side: instance topic =====

	// PostError surfaces a non-terminal error to subscribers. The
	// lifecycle does not change — use ReportFailed for a terminal failure.
	PostError(ctx context.Context, instanceID string, instErr InstanceError) error

	// PostInputRequest publishes a RequestInputTask's request for input on
	// the instance's topic; the requestID is what a client passes back to
	// RespondToInputRequest.
	PostInputRequest(ctx context.Context, instanceID string, req InputRequest) error

	// PostNodeCompleted publishes a node-completion event on the
	// instance's topic.
	PostNodeCompleted(ctx context.Context, instanceID string, nc NodeCompleted) error

	// Close releases the broker's resources (connections, goroutines).
	Close() error
}

// Synchronous broker errors (specs/message-brokers/overview.spec.md § Error
// model). There are no ErrAlready* errors: detecting a finished instance
// would need a broker-held status record, and the broker holds none —
// already-finished instances surface asynchronously as ALREADY_FINISHED.
var (
	ErrUnknownProcess      = errors.New("message broker: unknown process")
	ErrUnknownStartID      = errors.New("message broker: unknown start id")
	ErrCancelNotAllowed    = errors.New("message broker: process does not allow external cancel")
	ErrTerminateNotAllowed = errors.New("message broker: process does not allow external terminate")
	ErrUnknownWorker       = errors.New("message broker: unknown worker")
	ErrBrokerClosed        = errors.New("message broker: closed")
)

// Asynchronous error codes, delivered as InstanceEvents of kind Error.
const (
	ErrCodeInstanceNotFound    = "INSTANCE_NOT_FOUND"
	ErrCodeNotWaiting          = "NOT_WAITING"
	ErrCodeAlreadyFinished     = "ALREADY_FINISHED"
	ErrCodeAlreadyInterrupting = "ALREADY_INTERRUPTING"
	ErrCodeTaskFailed          = "TASK_FAILED"
	ErrCodeProcessFailed       = "PROCESS_FAILED"
	ErrCodeBackpressureDrop    = "BACKPRESSURE_DROP"
)

// ===== Request types =====

// StartRequest asks for a new run of a registered process.
type StartRequest struct {
	Namespace string         // package import path of the registered process
	ProcessID string         // the registered process's Id
	Version   string         // the registered process's Version
	StartID   string         // which StartEvent to use as the entrypoint
	Input     map[string]any // input variables; validated against the registry-carried InputContract

	// CorrelationKey is an optional client-side key recorded in every
	// InstanceEvent emitted for this instance.
	CorrelationKey *string
}

// Key returns the request's process key.
func (r StartRequest) Key() ProcessKey {
	return ProcessKey{Namespace: r.Namespace, ProcessID: r.ProcessID, Version: r.Version}
}

// CancelRequest asks to cancel a run. The process key is needed so the broker
// can check AllowExternalCancel in the registry snapshot.
type CancelRequest struct {
	Namespace string
	ProcessID string
	Version   string

	InstanceID string
	Reason     *string
}

// Key returns the request's process key.
func (r CancelRequest) Key() ProcessKey {
	return ProcessKey{Namespace: r.Namespace, ProcessID: r.ProcessID, Version: r.Version}
}

// TerminateRequest asks to hard-stop a run. The process key is needed so the
// broker can check AllowExternalTerminate in the registry snapshot.
type TerminateRequest struct {
	Namespace string
	ProcessID string
	Version   string

	InstanceID string
	Reason     *string
}

// Key returns the request's process key.
func (r TerminateRequest) Key() ProcessKey {
	return ProcessKey{Namespace: r.Namespace, ProcessID: r.ProcessID, Version: r.Version}
}

// ===== Registry types =====

// ProcessRegistration is one worker's advertisement that it can execute one
// process version. The contracts are runtime-derived from the process
// definition and travel through the broker in the wire format, so producers
// never import the process packages.
type ProcessRegistration struct {
	Namespace   string  `cbor:"namespace"`
	ProcessID   string  `cbor:"processId"`
	Version     string  `cbor:"version"`
	Name        *string `cbor:"name,omitempty"`
	Description *string `cbor:"description,omitempty"`

	// Boundary surface — what producers need to construct and validate a
	// Submit.
	StartEvents []StartEventInfo `cbor:"startEvents,omitempty"`
	EndEvents   []EndEventInfo   `cbor:"endEvents,omitempty"`

	// Operation hints — what an MCP / web UI can offer for this process.
	AllowExternalCancel    bool `cbor:"allowExternalCancel,omitempty"`
	AllowExternalTerminate bool `cbor:"allowExternalTerminate,omitempty"`

	// Markdown is pre-rendered documentation for tools like the MCP
	// describe_process built-in.
	Markdown string `cbor:"markdown,omitempty"`

	// For observability.
	WorkerID      string    `cbor:"workerId,omitempty"`      // set by the worker on RegisterProcesses
	RegisteredAt  time.Time `cbor:"registeredAt,omitempty"`  // set by the broker on first RegisterProcesses
	LastHeartbeat time.Time `cbor:"lastHeartbeat,omitempty"` // set by the broker on each Heartbeat / Register
}

// Key returns the registration's process key.
func (r ProcessRegistration) Key() ProcessKey {
	return ProcessKey{Namespace: r.Namespace, ProcessID: r.ProcessID, Version: r.Version}
}

// StartEventInfo describes one StartEvent on a registered process.
type StartEventInfo struct {
	Id            string         `cbor:"id"`
	Name          string         `cbor:"name,omitempty"`
	InputContract *InputContract `cbor:"inputContract,omitempty"`
}

// EndEventInfo describes one EndEvent on a registered process.
type EndEventInfo struct {
	Id       string          `cbor:"id"`
	Name     string          `cbor:"name,omitempty"`
	Contract *OutputContract `cbor:"contract,omitempty"`
}

// ProcessKey identifies one process version — the broker's routing key.
type ProcessKey struct {
	Namespace string `cbor:"namespace"`
	ProcessID string `cbor:"processId"`
	Version   string `cbor:"version"`
}

// RegistryUpdate is one message on the SubscribeToProcessRegistry channel.
type RegistryUpdate struct {
	Kind         RegistryUpdateKind   `cbor:"kind"`
	Registration *ProcessRegistration `cbor:"registration,omitempty"` // nil only for RegistryUpdateSnapshotComplete
}

// RegistryUpdateKind identifies what a RegistryUpdate reports.
type RegistryUpdateKind int

const (
	// RegistryUpdateSnapshot entries open every subscription — one per
	// currently-live registration — followed by a single
	// RegistryUpdateSnapshotComplete sentinel with a nil Registration.
	RegistryUpdateSnapshot RegistryUpdateKind = iota
	RegistryUpdateSnapshotComplete
	RegistryUpdateAdded         // a worker freshly registered this process
	RegistryUpdateRemoved       // a worker called Unregister
	RegistryUpdateHeartbeatLost // TTL expired without a Heartbeat
)

// ===== Job types (worker-side) =====

// Job is a tagged union of broker-published work items the worker dispatches
// to executors. Exactly one kind-specific payload is set.
type Job struct {
	Kind JobKind    `cbor:"kind"`
	Key  ProcessKey `cbor:"key"` // routing key

	Start          *StartJob          `cbor:"start,omitempty"`
	RespondToInput *RespondToInputJob `cbor:"respondToInput,omitempty"`
	Cancel         *CancelJob         `cbor:"cancel,omitempty"`
	Terminate      *TerminateJob      `cbor:"terminate,omitempty"`
	Resume         *ResumeJob         `cbor:"resume,omitempty"`
}

// InstanceID returns the process instance the job targets.
func (j Job) InstanceID() string {
	switch j.Kind {
	case JobStart:
		return j.Start.InstanceID
	case JobRespondToInput:
		return j.RespondToInput.InstanceID
	case JobCancel:
		return j.Cancel.InstanceID
	case JobTerminate:
		return j.Terminate.InstanceID
	case JobResume:
		return j.Resume.InstanceID
	default:
		return ""
	}
}

// JobKind identifies a Job's payload.
type JobKind int

const (
	JobStart JobKind = iota
	JobRespondToInput
	JobCancel
	JobTerminate
	JobResume
)

// StartJob starts a new run.
type StartJob struct {
	InstanceID     string         `cbor:"instanceId"` // broker-generated UUID
	StartID        string         `cbor:"startId"`
	Input          map[string]any `cbor:"input,omitempty"`
	CorrelationKey *string        `cbor:"correlationKey,omitempty"`
}

// RespondToInputJob delivers a client's response to a RequestInputTask.
type RespondToInputJob struct {
	InstanceID string         `cbor:"instanceId"`
	RequestID  string         `cbor:"requestId"`
	Payload    map[string]any `cbor:"payload,omitempty"`
}

// CancelJob asks the worker holding the instance to cancel it.
type CancelJob struct {
	InstanceID string  `cbor:"instanceId"`
	Reason     *string `cbor:"reason,omitempty"`
}

// TerminateJob asks the worker holding the instance to hard-stop it.
type TerminateJob struct {
	InstanceID string  `cbor:"instanceId"`
	Reason     *string `cbor:"reason,omitempty"`
}

// ResumeJob re-evaluates a suspended instance whose wait condition was
// satisfied.
type ResumeJob struct {
	InstanceID string `cbor:"instanceId"`
}

// ===== Event types =====

// InstanceEvent is what SubscribeToInstance delivers: a tagged union covering
// lifecycle transitions, input requests, node completions, errors, and final
// results. Exactly one kind-specific payload is set.
type InstanceEvent struct {
	InstanceID     string            `cbor:"instanceId"`
	CorrelationKey *string           `cbor:"correlationKey,omitempty"` // mirrored from StartRequest if set
	Kind           InstanceEventKind `cbor:"kind"`
	OccurredAt     time.Time         `cbor:"occurredAt"`

	Lifecycle     *LifecycleChange  `cbor:"lifecycle,omitempty"`
	InputRequest  *InputRequest     `cbor:"inputRequest,omitempty"`
	NodeCompleted *NodeCompleted    `cbor:"nodeCompleted,omitempty"`
	Error         *InstanceError    `cbor:"error,omitempty"`
	Result        *EvaluationResult `cbor:"result,omitempty"` // emitted as the channel closes
}

// InstanceEventKind identifies an InstanceEvent's payload.
type InstanceEventKind int

const (
	InstanceEventLifecycle InstanceEventKind = iota
	InstanceEventInputRequest
	InstanceEventNodeCompleted
	InstanceEventError
	InstanceEventResult
)

// LifecycleChange reports a lifecycle transition. The instance's current
// phase is its latest lifecycle event.
type LifecycleChange struct {
	Phase ProcessStatus `cbor:"phase"`
}

// InputRequest is a RequestInputTask's request for input, surfaced to
// clients; RequestID is what a client passes back to RespondToInputRequest.
type InputRequest struct {
	NodeID    string         `cbor:"nodeId"`
	RequestID string         `cbor:"requestId"`
	Payload   map[string]any `cbor:"payload,omitempty"` // form schema, prompt text, current case state, ...
}

// NodeCompleted reports one node finishing during evaluation.
type NodeCompleted struct {
	NodeID   string         `cbor:"nodeId"`
	NodeKind string         `cbor:"nodeKind"` // "Task", "DecisionTask", "EndEvent", ...
	Outputs  map[string]any `cbor:"outputs,omitempty"`
}

// InstanceError is an asynchronous error on the instance topic. Code is one
// of the ErrCode* constants.
type InstanceError struct {
	Code    string `cbor:"code"`
	Message string `cbor:"message,omitempty"`
}

// ===== RegistryStore =====

// RegistryStore is the side-store used by backends whose broker has no
// native KV facility (Azure Service Bus, Google Pub/Sub, AWS SQS/SNS). It
// holds ONLY broker-coordination data — never process state:
//
//   - the worker registry: live ProcessRegistrations with TTL expiry and a
//     change feed
//   - timer records: durable "deliver a JobResume at time T" entries, for
//     backends without native delayed delivery
//   - last-event records, for backends whose event fabric cannot deliver
//     messages published before a subscriber attached
type RegistryStore interface {
	PutRegistrations(ctx context.Context, workerID string, regs []ProcessRegistration, ttl time.Duration) error
	Touch(ctx context.Context, workerID string, ttl time.Duration) error // heartbeat; ErrUnknownWorker if absent
	Delete(ctx context.Context, workerID string) error                   // unregister; ErrUnknownWorker if absent
	Snapshot(ctx context.Context) ([]ProcessRegistration, error)
	Watch(ctx context.Context) (<-chan RegistryUpdate, error)

	PutTimer(ctx context.Context, instanceID string, fireAt time.Time) error
	DueTimers(ctx context.Context, now time.Time) ([]string, error) // instanceIDs; claimed atomically
	DeleteTimer(ctx context.Context, instanceID string) error

	Close() error
}

// NewProcessInstanceID generates a UUIDv7 process instance id: time-ordered,
// globally unique, generated broker-side at Submit.
func NewProcessInstanceID() string {
	var b [16]byte
	ms := uint64(time.Now().UnixMilli()) //nolint:gosec // wall-clock millis fit 48 bits until year 10889
	b[0] = byte(ms >> 40)
	b[1] = byte(ms >> 32)
	b[2] = byte(ms >> 24)
	b[3] = byte(ms >> 16)
	b[4] = byte(ms >> 8)
	b[5] = byte(ms)
	if _, err := rand.Read(b[6:]); err != nil {
		panic(err) // crypto/rand never fails on supported platforms
	}
	b[6] = (b[6] & 0x0f) | 0x70 // version 7
	b[8] = (b[8] & 0x3f) | 0x80 // RFC 4122 variant
	dst := make([]byte, 36)
	hex.Encode(dst[0:8], b[0:4])
	dst[8] = '-'
	hex.Encode(dst[9:13], b[4:6])
	dst[13] = '-'
	hex.Encode(dst[14:18], b[6:8])
	dst[18] = '-'
	hex.Encode(dst[19:23], b[8:10])
	dst[23] = '-'
	hex.Encode(dst[24:36], b[10:16])
	return string(dst)
}
