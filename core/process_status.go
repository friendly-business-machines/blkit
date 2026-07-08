package core

// ProcessStatus is the lifecycle phase of one process run. The full process
// layer (specs/processes/process.spec.md) is not implemented yet; the message
// broker (specs/message-brokers/overview.spec.md) carries these phases in its
// lifecycle events today.
type ProcessStatus int

const (
	ProcessStatusPending   ProcessStatus = iota // submitted but not yet started
	ProcessStatusRunning                        // at least one task is in progress or pending
	ProcessStatusSuspended                      // paused waiting for an external event; resumable via re-evaluation
	ProcessStatusCompleted                      // reached an EndEvent or TerminateEvent
	ProcessStatusCancelled                      // reached a CancelEvent — clean abort, distinct from Completed and Failed
	ProcessStatusFailed                         // an unrecoverable error occurred
)

func (s ProcessStatus) String() string {
	switch s {
	case ProcessStatusPending:
		return "Pending"
	case ProcessStatusRunning:
		return "Running"
	case ProcessStatusSuspended:
		return "Suspended"
	case ProcessStatusCompleted:
		return "Completed"
	case ProcessStatusCancelled:
		return "Cancelled"
	case ProcessStatusFailed:
		return "Failed"
	default:
		return "Unknown"
	}
}

// Finished reports whether the status is terminal (Completed, Cancelled, or
// Failed).
func (s ProcessStatus) Finished() bool {
	return s == ProcessStatusCompleted || s == ProcessStatusCancelled || s == ProcessStatusFailed
}

// EvaluationResult is the outcome of a process evaluation, delivered to
// clients in the terminal Result instance event. The full shape (execution
// context, history, steps — see specs/processes/process.spec.md) arrives with
// the process layer; today it carries the fields clients consume from outcome
// events.
type EvaluationResult struct {
	Status  ProcessStatus  `cbor:"status"`
	Context map[string]any `cbor:"context,omitempty"` // output variables visible at the end event
}
