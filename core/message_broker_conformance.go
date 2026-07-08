package core

import (
	"context"
	"errors"
	"testing"
	"time"
)

// BrokerConformanceOptions tells the conformance suite what the opened broker
// was configured with and which optional capabilities it has, so per-backend
// differences the overview spec allows (queue removal, timing knobs) are
// declared rather than guessed.
type BrokerConformanceOptions struct {
	// SupportsQueueRemoval: Cancel of a still-queued JobStart removes it
	// from the queue (Redis, NATS, in-memory). Backends without selective
	// removal (RabbitMQ, the cloud brokers) route every cancel as a
	// JobCancel and skip the queue-removal subtest.
	SupportsQueueRemoval bool

	// RegistrationTTL is the registration TTL the opened broker uses. Keep
	// it short (~150ms) so the heartbeat-loss subtest is fast; zero skips
	// that subtest.
	RegistrationTTL time.Duration

	// InFlightTimeout is the in-flight redelivery timeout the opened broker
	// uses. Keep it short (~300ms) so the redelivery subtest is fast; zero
	// skips that subtest.
	InFlightTimeout time.Duration

	// OpenWithCipher, when non-nil, returns a broker configured with a
	// PayloadCipher; the suite re-runs a full round trip through it.
	OpenWithCipher func(t *testing.T) MessageBroker
}

// RunMessageBrokerConformance runs the shared message-broker conformance
// suite against a backend. Every backend module calls it from its own tests,
// so all backends are held to the identical client↔worker semantics
// (specs/message-brokers/overview.spec.md § Testing).
//
// open is called once per subtest and must return a fresh broker on empty
// underlying infrastructure (or an isolating prefix), configured with the
// TTL/timeout values declared in opts.
func RunMessageBrokerConformance(t *testing.T, open func(t *testing.T) MessageBroker, opts BrokerConformanceOptions) {
	t.Helper()

	t.Run("RegistrySnapshotAndUpdates", func(t *testing.T) {
		b := open(t)
		defer func() { _ = b.Close() }()
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()

		ch, err := b.SubscribeToProcessRegistry(ctx)
		if err != nil {
			t.Fatalf("subscribe registry: %v", err)
		}
		if u := nextRegistryUpdate(t, ch); u.Kind != RegistryUpdateSnapshotComplete {
			t.Fatalf("empty registry: want immediate SnapshotComplete, got kind %d", u.Kind)
		}

		regs := []ProcessRegistration{conformanceRegistration(t, "a", false, false), conformanceRegistration(t, "b", false, false)}
		if err := b.RegisterProcesses(ctx, "w1", regs); err != nil {
			t.Fatalf("register: %v", err)
		}
		added := map[string]bool{}
		for range 2 {
			u := nextRegistryUpdate(t, ch)
			if u.Kind != RegistryUpdateAdded || u.Registration == nil {
				t.Fatalf("want Added update, got %+v", u)
			}
			if u.Registration.WorkerID != "w1" {
				t.Fatalf("Added update missing WorkerID, got %q", u.Registration.WorkerID)
			}
			added[u.Registration.ProcessID] = true
		}
		if !added["proc-a"] || !added["proc-b"] {
			t.Fatalf("want Added for proc-a and proc-b, got %v", added)
		}

		if err := b.Heartbeat(ctx, "w1"); err != nil {
			t.Fatalf("heartbeat: %v", err)
		}
		if err := b.Heartbeat(ctx, "nobody"); !errors.Is(err, ErrUnknownWorker) {
			t.Fatalf("heartbeat unknown worker: want ErrUnknownWorker, got %v", err)
		}

		if err := b.Unregister(ctx, "w1"); err != nil {
			t.Fatalf("unregister: %v", err)
		}
		for range 2 {
			if u := nextRegistryUpdate(t, ch); u.Kind != RegistryUpdateRemoved {
				t.Fatalf("want Removed update, got %+v", u)
			}
		}
		if err := b.Unregister(ctx, "w1"); !errors.Is(err, ErrUnknownWorker) {
			t.Fatalf("unregister twice: want ErrUnknownWorker, got %v", err)
		}
	})

	t.Run("RegistrySnapshotContents", func(t *testing.T) {
		b := open(t)
		defer func() { _ = b.Close() }()
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()

		if err := b.RegisterProcesses(ctx, "w1", []ProcessRegistration{conformanceRegistration(t, "a", false, false)}); err != nil {
			t.Fatalf("register: %v", err)
		}
		ch, err := b.SubscribeToProcessRegistry(ctx)
		if err != nil {
			t.Fatalf("subscribe registry: %v", err)
		}
		u := nextRegistryUpdate(t, ch)
		if u.Kind != RegistryUpdateSnapshot || u.Registration == nil || u.Registration.ProcessID != "proc-a" {
			t.Fatalf("want snapshot entry for proc-a, got %+v", u)
		}
		if len(u.Registration.StartEvents) == 0 || u.Registration.StartEvents[0].InputContract == nil {
			t.Fatal("snapshot registration must carry the start event's InputContract")
		}
		if u := nextRegistryUpdate(t, ch); u.Kind != RegistryUpdateSnapshotComplete {
			t.Fatalf("want SnapshotComplete sentinel, got %+v", u)
		}
	})

	if opts.RegistrationTTL > 0 {
		t.Run("HeartbeatLoss", func(t *testing.T) {
			b := open(t)
			defer func() { _ = b.Close() }()
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()

			ch, err := b.SubscribeToProcessRegistry(ctx)
			if err != nil {
				t.Fatalf("subscribe registry: %v", err)
			}
			if u := nextRegistryUpdate(t, ch); u.Kind != RegistryUpdateSnapshotComplete {
				t.Fatalf("want SnapshotComplete, got %+v", u)
			}
			if err := b.RegisterProcesses(ctx, "w1", []ProcessRegistration{conformanceRegistration(t, "a", false, false)}); err != nil {
				t.Fatalf("register: %v", err)
			}
			if u := nextRegistryUpdate(t, ch); u.Kind != RegistryUpdateAdded {
				t.Fatalf("want Added, got %+v", u)
			}
			// No heartbeat: the TTL must lapse and the loss must be broadcast.
			if u := nextRegistryUpdate(t, ch); u.Kind != RegistryUpdateHeartbeatLost {
				t.Fatalf("want HeartbeatLost, got %+v", u)
			}
		})
	}

	t.Run("SubmitValidation", func(t *testing.T) {
		b := open(t)
		defer func() { _ = b.Close() }()
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()

		// Cold start / nothing registered: Submit must not hang forever.
		shortCtx, shortCancel := context.WithTimeout(ctx, 500*time.Millisecond)
		_, err := b.Submit(shortCtx, conformanceStart("a", "start", map[string]any{"amount": 10.5}))
		shortCancel()
		if err == nil {
			t.Fatal("submit with no live workers must fail")
		}

		if err := b.RegisterProcesses(ctx, "w1", []ProcessRegistration{conformanceRegistration(t, "a", false, false)}); err != nil {
			t.Fatalf("register: %v", err)
		}

		if _, err := b.Submit(ctx, conformanceStart("zzz", "start", nil)); !errors.Is(err, ErrUnknownProcess) {
			t.Fatalf("unknown process: want ErrUnknownProcess, got %v", err)
		}
		if _, err := b.Submit(ctx, conformanceStart("a", "no-such-start", nil)); !errors.Is(err, ErrUnknownStartID) {
			t.Fatalf("unknown start id: want ErrUnknownStartID, got %v", err)
		}
		var dcv *DataContractValidationError
		if _, err := b.Submit(ctx, conformanceStart("a", "start", map[string]any{})); !errors.As(err, &dcv) {
			t.Fatalf("missing required field: want DataContractValidationError, got %v", err)
		}
		if _, err := b.Submit(ctx, conformanceStart("a", "start", map[string]any{"amount": 1.0, "bogus": true})); !errors.As(err, &dcv) {
			t.Fatalf("undeclared field: want DataContractValidationError, got %v", err)
		}
		if _, err := b.Submit(ctx, conformanceStart("a", "start", map[string]any{"amount": "not a number"})); !errors.As(err, &dcv) {
			t.Fatalf("type mismatch: want DataContractValidationError, got %v", err)
		}
		id, err := b.Submit(ctx, conformanceStart("a", "start", map[string]any{"amount": 42.5, "note": "hi"}))
		if err != nil || id == "" {
			t.Fatalf("valid submit: got id %q, err %v", id, err)
		}
	})

	t.Run("JobDeliveryAndSelectiveConsumption", func(t *testing.T) {
		b := open(t)
		defer func() { _ = b.Close() }()
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()

		regA := conformanceRegistration(t, "a", false, false)
		regB := conformanceRegistration(t, "b", false, false)
		if err := b.RegisterProcesses(ctx, "w1", []ProcessRegistration{regA, regB}); err != nil {
			t.Fatalf("register: %v", err)
		}
		jobsA, err := b.FetchJobs(ctx, []ProcessKey{regA.Key()})
		if err != nil {
			t.Fatalf("fetch A: %v", err)
		}
		jobsB, err := b.FetchJobs(ctx, []ProcessKey{regB.Key()})
		if err != nil {
			t.Fatalf("fetch B: %v", err)
		}

		idA, err := b.Submit(ctx, conformanceStart("a", "start", map[string]any{"amount": 42.5, "note": "hello"}))
		if err != nil {
			t.Fatalf("submit A: %v", err)
		}
		idB, err := b.Submit(ctx, conformanceStart("b", "start", map[string]any{"amount": 7.25}))
		if err != nil {
			t.Fatalf("submit B: %v", err)
		}

		jobA := nextJob(t, jobsA)
		if jobA.Kind != JobStart || jobA.Start.InstanceID != idA {
			t.Fatalf("fetcher A: want A's JobStart, got %+v", jobA)
		}
		if got := jobA.Start.Input["note"]; got != "hello" {
			t.Fatalf("input round trip: note = %v", got)
		}
		if got, ok := jobA.Start.Input["amount"].(float64); !ok || got != 42.5 {
			t.Fatalf("input round trip: amount = %v", jobA.Start.Input["amount"])
		}
		jobB := nextJob(t, jobsB)
		if jobB.Kind != JobStart || jobB.Start.InstanceID != idB {
			t.Fatalf("fetcher B: want B's JobStart, got %+v", jobB)
		}
		select {
		case j, ok := <-jobsA:
			if ok {
				t.Fatalf("fetcher A must not see B's jobs, got %+v", j)
			}
		case <-time.After(300 * time.Millisecond):
		}
	})

	t.Run("LifecycleHappyPath", func(t *testing.T) {
		b := open(t)
		defer func() { _ = b.Close() }()
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()

		reg := conformanceRegistration(t, "a", false, false)
		mustRegister(t, ctx, b, "w1", reg)
		jobs := mustFetch(t, ctx, b, reg.Key())

		corr := "corr-123"
		req := conformanceStart("a", "start", map[string]any{"amount": 1.0})
		req.CorrelationKey = &corr
		id, err := b.Submit(ctx, req)
		if err != nil {
			t.Fatalf("submit: %v", err)
		}
		events, err := b.SubscribeToInstance(ctx, id)
		if err != nil {
			t.Fatalf("subscribe: %v", err)
		}
		if evt := awaitLifecycle(t, events, ProcessStatusPending); evt.CorrelationKey == nil || *evt.CorrelationKey != corr {
			t.Fatalf("Pending event must mirror the correlation key, got %+v", evt.CorrelationKey)
		}

		job := nextJob(t, jobs)
		if job.Kind != JobStart || job.Start.InstanceID != id {
			t.Fatalf("want JobStart for %s, got %+v", id, job)
		}
		mustReport(t, b.ReportRunning(ctx, id))
		awaitLifecycle(t, events, ProcessStatusRunning)

		if err := b.PostNodeCompleted(ctx, id, NodeCompleted{NodeID: "score", NodeKind: "Task", Outputs: map[string]any{"ok": true}}); err != nil {
			t.Fatalf("post node completed: %v", err)
		}
		nc := awaitEvent(t, events, func(e InstanceEvent) bool { return e.Kind == InstanceEventNodeCompleted })
		if nc.NodeCompleted.NodeID != "score" {
			t.Fatalf("node completed round trip: %+v", nc.NodeCompleted)
		}

		mustReport(t, b.ReportCompleted(ctx, id, EvaluationResult{Status: ProcessStatusCompleted, Context: map[string]any{"offer": "yes"}}))
		awaitLifecycle(t, events, ProcessStatusCompleted)
		res := awaitEvent(t, events, func(e InstanceEvent) bool { return e.Kind == InstanceEventResult })
		if res.Result.Status != ProcessStatusCompleted || res.Result.Context["offer"] != "yes" {
			t.Fatalf("result round trip: %+v", res.Result)
		}
		awaitClose(t, events)
	})

	t.Run("LateSubscriberReplay", func(t *testing.T) {
		b := open(t)
		defer func() { _ = b.Close() }()
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()

		reg := conformanceRegistration(t, "a", false, false)
		mustRegister(t, ctx, b, "w1", reg)
		jobs := mustFetch(t, ctx, b, reg.Key())
		id, err := b.Submit(ctx, conformanceStart("a", "start", map[string]any{"amount": 1.0}))
		if err != nil {
			t.Fatalf("submit: %v", err)
		}
		if job := nextJob(t, jobs); job.Start.InstanceID != id {
			t.Fatalf("want JobStart for %s", id)
		}
		mustReport(t, b.ReportRunning(ctx, id))
		mustReport(t, b.ReportCompleted(ctx, id, EvaluationResult{Status: ProcessStatusCompleted}))

		// Subscribe only now: latest lifecycle + terminal result must replay.
		events, err := b.SubscribeToInstance(ctx, id)
		if err != nil {
			t.Fatalf("late subscribe: %v", err)
		}
		awaitLifecycle(t, events, ProcessStatusCompleted)
		if evt := awaitEvent(t, events, func(e InstanceEvent) bool { return e.Kind == InstanceEventResult }); evt.Result.Status != ProcessStatusCompleted {
			t.Fatalf("late subscriber result: %+v", evt.Result)
		}
		awaitClose(t, events)
	})

	t.Run("SuspendResume", func(t *testing.T) {
		b := open(t)
		defer func() { _ = b.Close() }()
		ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
		defer cancel()

		reg := conformanceRegistration(t, "a", false, false)
		mustRegister(t, ctx, b, "w1", reg)
		jobs := mustFetch(t, ctx, b, reg.Key())
		id, err := b.Submit(ctx, conformanceStart("a", "start", map[string]any{"amount": 1.0}))
		if err != nil {
			t.Fatalf("submit: %v", err)
		}
		events, err := b.SubscribeToInstance(ctx, id)
		if err != nil {
			t.Fatalf("subscribe: %v", err)
		}
		if job := nextJob(t, jobs); job.Kind != JobStart {
			t.Fatalf("want JobStart, got %+v", job)
		}
		mustReport(t, b.ReportRunning(ctx, id))
		resumeAt := time.Now().Add(200 * time.Millisecond)
		mustReport(t, b.ReportSuspended(ctx, id, &resumeAt))
		awaitLifecycle(t, events, ProcessStatusSuspended)

		job := nextJob(t, jobs)
		if job.Kind != JobResume || job.Resume.InstanceID != id {
			t.Fatalf("want JobResume for %s, got %+v", id, job)
		}
		mustReport(t, b.ReportRunning(ctx, id))
		mustReport(t, b.ReportCompleted(ctx, id, EvaluationResult{Status: ProcessStatusCompleted}))
		awaitLifecycle(t, events, ProcessStatusCompleted)
	})

	t.Run("InputRequestRoundTrip", func(t *testing.T) {
		b := open(t)
		defer func() { _ = b.Close() }()
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()

		reg := conformanceRegistration(t, "a", false, false)
		mustRegister(t, ctx, b, "w1", reg)
		jobs := mustFetch(t, ctx, b, reg.Key())
		id, err := b.Submit(ctx, conformanceStart("a", "start", map[string]any{"amount": 1.0}))
		if err != nil {
			t.Fatalf("submit: %v", err)
		}
		events, err := b.SubscribeToInstance(ctx, id)
		if err != nil {
			t.Fatalf("subscribe: %v", err)
		}
		if job := nextJob(t, jobs); job.Kind != JobStart {
			t.Fatalf("want JobStart, got %+v", job)
		}
		mustReport(t, b.ReportRunning(ctx, id))
		if err := b.PostInputRequest(ctx, id, InputRequest{NodeID: "approve", RequestID: "req-1", Payload: map[string]any{"prompt": "ok?"}}); err != nil {
			t.Fatalf("post input request: %v", err)
		}
		mustReport(t, b.ReportSuspended(ctx, id, nil))

		ir := awaitEvent(t, events, func(e InstanceEvent) bool { return e.Kind == InstanceEventInputRequest })
		if ir.InputRequest.RequestID != "req-1" || ir.InputRequest.Payload["prompt"] != "ok?" {
			t.Fatalf("input request round trip: %+v", ir.InputRequest)
		}
		if err := b.RespondToInputRequest(ctx, id, "req-1", map[string]any{"approved": true}); err != nil {
			t.Fatalf("respond: %v", err)
		}
		job := nextJob(t, jobs)
		if job.Kind != JobRespondToInput || job.RespondToInput.RequestID != "req-1" {
			t.Fatalf("want JobRespondToInput req-1, got %+v", job)
		}
		if got := job.RespondToInput.Payload["approved"]; got != true {
			t.Fatalf("response payload round trip: %v", got)
		}
	})

	if opts.SupportsQueueRemoval {
		t.Run("CancelQueuedJobRemoved", func(t *testing.T) {
			b := open(t)
			defer func() { _ = b.Close() }()
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()

			// AllowExternalCancel is false: queue-side removal needs no opt-in.
			reg := conformanceRegistration(t, "a", false, false)
			mustRegister(t, ctx, b, "w1", reg)
			id, err := b.Submit(ctx, conformanceStart("a", "start", map[string]any{"amount": 1.0}))
			if err != nil {
				t.Fatalf("submit: %v", err)
			}
			if err := b.Cancel(ctx, CancelRequest{Namespace: reg.Namespace, ProcessID: reg.ProcessID, Version: reg.Version, InstanceID: id}); err != nil {
				t.Fatalf("cancel queued: %v", err)
			}
			events, err := b.SubscribeToInstance(ctx, id)
			if err != nil {
				t.Fatalf("subscribe: %v", err)
			}
			awaitLifecycle(t, events, ProcessStatusCancelled)
			awaitClose(t, events)

			jobs := mustFetch(t, ctx, b, reg.Key())
			select {
			case j, ok := <-jobs:
				if ok {
					t.Fatalf("removed job must not be delivered, got %+v", j)
				}
			case <-time.After(300 * time.Millisecond):
			}
		})
	}

	t.Run("CancelInFlightNeedsOptIn", func(t *testing.T) {
		b := open(t)
		defer func() { _ = b.Close() }()
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()

		reg := conformanceRegistration(t, "a", false, false) // AllowExternalCancel: false
		mustRegister(t, ctx, b, "w1", reg)
		jobs := mustFetch(t, ctx, b, reg.Key())
		id, err := b.Submit(ctx, conformanceStart("a", "start", map[string]any{"amount": 1.0}))
		if err != nil {
			t.Fatalf("submit: %v", err)
		}
		if job := nextJob(t, jobs); job.Start.InstanceID != id {
			t.Fatalf("want JobStart for %s", id)
		}
		mustReport(t, b.ReportRunning(ctx, id))
		err = b.Cancel(ctx, CancelRequest{Namespace: reg.Namespace, ProcessID: reg.ProcessID, Version: reg.Version, InstanceID: id})
		if !errors.Is(err, ErrCancelNotAllowed) {
			t.Fatalf("cancel without opt-in: want ErrCancelNotAllowed, got %v", err)
		}
	})

	t.Run("CancelInFlightOptedIn", func(t *testing.T) {
		b := open(t)
		defer func() { _ = b.Close() }()
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()

		reg := conformanceRegistration(t, "a", true, false) // AllowExternalCancel: true
		mustRegister(t, ctx, b, "w1", reg)
		jobs := mustFetch(t, ctx, b, reg.Key())
		id, err := b.Submit(ctx, conformanceStart("a", "start", map[string]any{"amount": 1.0}))
		if err != nil {
			t.Fatalf("submit: %v", err)
		}
		events, err := b.SubscribeToInstance(ctx, id)
		if err != nil {
			t.Fatalf("subscribe: %v", err)
		}
		if job := nextJob(t, jobs); job.Start.InstanceID != id {
			t.Fatalf("want JobStart for %s", id)
		}
		mustReport(t, b.ReportRunning(ctx, id))
		reason := "changed my mind"
		if err := b.Cancel(ctx, CancelRequest{Namespace: reg.Namespace, ProcessID: reg.ProcessID, Version: reg.Version, InstanceID: id, Reason: &reason}); err != nil {
			t.Fatalf("cancel opted-in: %v", err)
		}
		job := nextJob(t, jobs)
		if job.Kind != JobCancel || job.Cancel.InstanceID != id || job.Cancel.Reason == nil || *job.Cancel.Reason != reason {
			t.Fatalf("want JobCancel with reason, got %+v", job)
		}
		mustReport(t, b.ReportCancelled(ctx, id))
		awaitLifecycle(t, events, ProcessStatusCancelled)
		awaitClose(t, events)
	})

	t.Run("TerminateNeedsOptIn", func(t *testing.T) {
		b := open(t)
		defer func() { _ = b.Close() }()
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()

		noOptIn := conformanceRegistration(t, "a", false, false)
		optedIn := conformanceRegistration(t, "b", false, true)
		mustRegister(t, ctx, b, "w1", noOptIn, optedIn)
		jobs := mustFetch(t, ctx, b, optedIn.Key())

		idA, err := b.Submit(ctx, conformanceStart("a", "start", map[string]any{"amount": 1.0}))
		if err != nil {
			t.Fatalf("submit a: %v", err)
		}
		err = b.Terminate(ctx, TerminateRequest{Namespace: noOptIn.Namespace, ProcessID: noOptIn.ProcessID, Version: noOptIn.Version, InstanceID: idA})
		if !errors.Is(err, ErrTerminateNotAllowed) {
			t.Fatalf("terminate without opt-in: want ErrTerminateNotAllowed, got %v", err)
		}

		idB, err := b.Submit(ctx, conformanceStart("b", "start", map[string]any{"amount": 1.0}))
		if err != nil {
			t.Fatalf("submit b: %v", err)
		}
		if job := nextJob(t, jobs); job.Start.InstanceID != idB {
			t.Fatalf("want JobStart for %s", idB)
		}
		if err := b.Terminate(ctx, TerminateRequest{Namespace: optedIn.Namespace, ProcessID: optedIn.ProcessID, Version: optedIn.Version, InstanceID: idB}); err != nil {
			t.Fatalf("terminate opted-in: %v", err)
		}
		if job := nextJob(t, jobs); job.Kind != JobTerminate || job.Terminate.InstanceID != idB {
			t.Fatalf("want JobTerminate for %s, got %+v", idB, job)
		}
	})

	if opts.InFlightTimeout > 0 {
		t.Run("RedeliveryAfterWorkerDeath", func(t *testing.T) {
			b := open(t)
			defer func() { _ = b.Close() }()
			ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
			defer cancel()

			reg := conformanceRegistration(t, "a", false, false)
			mustRegister(t, ctx, b, "w1", reg)

			dyingCtx, die := context.WithCancel(ctx)
			jobs1, err := b.FetchJobs(dyingCtx, []ProcessKey{reg.Key()})
			if err != nil {
				t.Fatalf("fetch 1: %v", err)
			}
			id, err := b.Submit(ctx, conformanceStart("a", "start", map[string]any{"amount": 1.0}))
			if err != nil {
				t.Fatalf("submit: %v", err)
			}
			if job := nextJob(t, jobs1); job.Start.InstanceID != id {
				t.Fatalf("want JobStart for %s", id)
			}
			die() // worker dies holding the job: no report, no settle

			jobs2 := mustFetch(t, ctx, b, reg.Key())
			job := nextJob(t, jobs2)
			if job.Kind != JobStart || job.Start.InstanceID != id {
				t.Fatalf("want redelivered JobStart for %s, got %+v", id, job)
			}
		})
	}

	if opts.OpenWithCipher != nil {
		t.Run("PayloadCipherRoundTrip", func(t *testing.T) {
			b := opts.OpenWithCipher(t)
			defer func() { _ = b.Close() }()
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()

			reg := conformanceRegistration(t, "a", false, false)
			mustRegister(t, ctx, b, "w1", reg)
			jobs := mustFetch(t, ctx, b, reg.Key())
			id, err := b.Submit(ctx, conformanceStart("a", "start", map[string]any{"amount": 9.75, "note": "secret"}))
			if err != nil {
				t.Fatalf("submit: %v", err)
			}
			events, err := b.SubscribeToInstance(ctx, id)
			if err != nil {
				t.Fatalf("subscribe: %v", err)
			}
			job := nextJob(t, jobs)
			if job.Start.Input["note"] != "secret" {
				t.Fatalf("encrypted payload round trip: %+v", job.Start.Input)
			}
			mustReport(t, b.ReportRunning(ctx, id))
			mustReport(t, b.ReportCompleted(ctx, id, EvaluationResult{Status: ProcessStatusCompleted, Context: map[string]any{"ok": true}}))
			awaitLifecycle(t, events, ProcessStatusCompleted)
			if evt := awaitEvent(t, events, func(e InstanceEvent) bool { return e.Kind == InstanceEventResult }); evt.Result.Context["ok"] != true {
				t.Fatalf("encrypted result round trip: %+v", evt.Result)
			}
		})
	}
}

// ===== conformance helpers =====

const conformanceWait = 8 * time.Second

func conformanceRegistration(t *testing.T, suffix string, allowCancel, allowTerminate bool) ProcessRegistration {
	t.Helper()
	contract, err := NewInputContract(
		RequiredField("amount", TypeNumber),
		OptionalField("note", TypeString),
	)
	if err != nil {
		t.Fatalf("contract: %v", err)
	}
	name := "Conformance Process " + suffix
	return ProcessRegistration{
		Namespace:              "example.com/conformance",
		ProcessID:              "proc-" + suffix,
		Version:                "1.0",
		Name:                   &name,
		StartEvents:            []StartEventInfo{{Id: "start", Name: "Start", InputContract: contract}},
		AllowExternalCancel:    allowCancel,
		AllowExternalTerminate: allowTerminate,
	}
}

func conformanceStart(suffix, startID string, input map[string]any) StartRequest {
	return StartRequest{
		Namespace: "example.com/conformance",
		ProcessID: "proc-" + suffix,
		Version:   "1.0",
		StartID:   startID,
		Input:     input,
	}
}

func mustRegister(t *testing.T, ctx context.Context, b MessageBroker, workerID string, regs ...ProcessRegistration) {
	t.Helper()
	if err := b.RegisterProcesses(ctx, workerID, regs); err != nil {
		t.Fatalf("register: %v", err)
	}
}

func mustFetch(t *testing.T, ctx context.Context, b MessageBroker, keys ...ProcessKey) <-chan Job {
	t.Helper()
	jobs, err := b.FetchJobs(ctx, keys)
	if err != nil {
		t.Fatalf("fetch jobs: %v", err)
	}
	return jobs
}

func mustReport(t *testing.T, err error) {
	t.Helper()
	if err != nil {
		t.Fatalf("report: %v", err)
	}
}

func nextRegistryUpdate(t *testing.T, ch <-chan RegistryUpdate) RegistryUpdate {
	t.Helper()
	select {
	case u, ok := <-ch:
		if !ok {
			t.Fatal("registry channel closed unexpectedly")
		}
		return u
	case <-time.After(conformanceWait):
		t.Fatal("timed out waiting for a registry update")
		return RegistryUpdate{}
	}
}

func nextJob(t *testing.T, ch <-chan Job) Job {
	t.Helper()
	select {
	case j, ok := <-ch:
		if !ok {
			t.Fatal("job channel closed unexpectedly")
		}
		return j
	case <-time.After(conformanceWait):
		t.Fatal("timed out waiting for a job")
		return Job{}
	}
}

// awaitEvent returns the first event matching pred, skipping others.
func awaitEvent(t *testing.T, ch <-chan InstanceEvent, pred func(InstanceEvent) bool) InstanceEvent {
	t.Helper()
	deadline := time.After(conformanceWait)
	for {
		select {
		case e, ok := <-ch:
			if !ok {
				t.Fatal("event channel closed while waiting for an event")
			}
			if pred(e) {
				return e
			}
		case <-deadline:
			t.Fatal("timed out waiting for an instance event")
		}
	}
}

func awaitLifecycle(t *testing.T, ch <-chan InstanceEvent, phase ProcessStatus) InstanceEvent {
	t.Helper()
	return awaitEvent(t, ch, func(e InstanceEvent) bool {
		return e.Kind == InstanceEventLifecycle && e.Lifecycle.Phase == phase
	})
}

func awaitClose(t *testing.T, ch <-chan InstanceEvent) {
	t.Helper()
	deadline := time.After(conformanceWait)
	for {
		select {
		case _, ok := <-ch:
			if !ok {
				return
			}
		case <-deadline:
			t.Fatal("timed out waiting for the event channel to close")
		}
	}
}
