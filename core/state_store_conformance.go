package core

import (
	"bytes"
	"fmt"
	"sync"
	"testing"
	"time"
)

// RunStateStoreConformance runs the shared state-store conformance suite
// against a backend. Every backend module calls it from its own tests, so all
// backends are held to the identical write contract
// (specs/state-stores/overview.spec.md § Testing).
//
// open is called once per subtest and must return a fresh, empty store plus a
// reopen function. reopen, when non-nil, is called after the store has been
// Closed and must return a new handle onto the SAME underlying storage — this
// is how the suite checks read-your-writes across handles (durability).
// Backends with no reopenable storage (in-memory) return a nil reopen.
func RunStateStoreConformance(t *testing.T, open func(t *testing.T) (store StateStore, reopen func() StateStore)) {
	t.Helper()

	ts := func(ms int) time.Time {
		return time.Date(2026, 7, 3, 10, 0, 0, 0, time.UTC).Add(time.Duration(ms) * time.Millisecond)
	}
	vw := func(runID, taskID, execID, field, val string, at time.Time) WriteOp {
		return WriteOp{RunID: runID, Kind: OpValueWrite, ValueWrite: &ValueWrite{
			TaskID: taskID, ExecutionID: execID, Field: field, Value: []byte(val), Timestamp: at,
		}}
	}
	flip := func(runID, taskID string, status ValueStatus, at time.Time) WriteOp {
		return WriteOp{RunID: runID, Kind: OpStatusFlip, StatusFlip: &StatusFlip{
			TaskID: taskID, NewStatus: status, Timestamp: at,
		}}
	}
	he := func(runID, kind, execID string, at time.Time) WriteOp {
		return WriteOp{RunID: runID, Kind: OpHistoryEntry, HistoryEntry: &HistoryEntry{
			Kind: kind, ExecutionID: execID, Payload: []byte(`{}`), Timestamp: at,
		}}
	}
	mustBatch := func(t *testing.T, s StateStore, ops ...WriteOp) {
		t.Helper()
		if err := s.WriteBatch(ops); err != nil {
			t.Fatalf("WriteBatch: %v", err)
		}
	}
	value := func(t *testing.T, cv *CurrentVersion, taskID, field string) []byte {
		t.Helper()
		if cv == nil {
			t.Fatalf("nil CurrentVersion")
		}
		return cv.Values[taskID][field]
	}

	t.Run("NotFound", func(t *testing.T) {
		s, _ := open(t)
		defer func() { _ = s.Close() }()
		cv, err := s.CurrentVersion("no-such-run")
		if err != nil || cv != nil {
			t.Fatalf("CurrentVersion(unknown) = %v, %v; want nil, nil", cv, err)
		}
		fh, err := s.FullHistory("no-such-run")
		if err != nil || fh != nil {
			t.Fatalf("FullHistory(unknown) = %v, %v; want nil, nil", fh, err)
		}
	})

	t.Run("MetadataRoundtrip", func(t *testing.T) {
		s, _ := open(t)
		defer func() { _ = s.Close() }()
		started := ts(0)
		meta := RunMetadata{
			RunID: "run-meta", ProcessID: "admissions", ProcessVersion: "1.0",
			Status: "running", StartedAt: &started, EvaluationCount: 1,
		}
		if err := s.Save(meta); err != nil {
			t.Fatalf("Save: %v", err)
		}
		cv, err := s.CurrentVersion("run-meta")
		if err != nil {
			t.Fatalf("CurrentVersion: %v", err)
		}
		if cv.Meta.ProcessID != "admissions" || cv.Meta.Status != "running" || cv.Meta.EvaluationCount != 1 {
			t.Fatalf("meta roundtrip mismatch: %+v", cv.Meta)
		}
		if cv.Meta.StartedAt == nil || !cv.Meta.StartedAt.Equal(started) {
			t.Fatalf("StartedAt mismatch: %v", cv.Meta.StartedAt)
		}
		// Upsert: Save again with updated fields.
		done := ts(500)
		meta.Status = "completed"
		meta.CompletedAt = &done
		meta.EvaluationCount = 3
		if err := s.Save(meta); err != nil {
			t.Fatalf("Save(update): %v", err)
		}
		cv, _ = s.CurrentVersion("run-meta")
		if cv.Meta.Status != "completed" || cv.Meta.EvaluationCount != 3 || cv.Meta.CompletedAt == nil || !cv.Meta.CompletedAt.Equal(done) {
			t.Fatalf("meta upsert mismatch: %+v", cv.Meta)
		}
	})

	t.Run("PendingInvisibility", func(t *testing.T) {
		s, _ := open(t)
		defer func() { _ = s.Close() }()
		run := "run-pending"
		if err := s.Save(RunMetadata{RunID: run, ProcessID: "admissions", ProcessVersion: "1.0", Status: "running"}); err != nil {
			t.Fatalf("Save: %v", err)
		}
		mustBatch(t, s,
			vw(run, "screen", "exec-1", "Score", `88`, ts(10)),
			vw(run, "screen", "exec-1", "Band", `"high"`, ts(10)),
		)
		cv, _ := s.CurrentVersion(run)
		if len(cv.Values["screen"]) != 0 {
			t.Fatalf("pending values leaked into current version: %v", cv.Values)
		}
		fh, _ := s.FullHistory(run)
		if len(fh.Values) != 2 {
			t.Fatalf("full history should hold pending writes, got %d", len(fh.Values))
		}
		for _, r := range fh.Values {
			if r.Status != ValueStatusPending {
				t.Fatalf("expected pending, got %s", r.Status)
			}
		}
		mustBatch(t, s, flip(run, "screen", ValueStatusCommitted, ts(20)))
		cv, _ = s.CurrentVersion(run)
		if !bytes.Equal(value(t, cv, "screen", "Score"), []byte(`88`)) ||
			!bytes.Equal(value(t, cv, "screen", "Band"), []byte(`"high"`)) {
			t.Fatalf("committed values missing or wrong: %v", cv.Values)
		}
		fh, _ = s.FullHistory(run)
		for _, r := range fh.Values {
			if r.Status != ValueStatusCommitted {
				t.Fatalf("expected committed after flip, got %s", r.Status)
			}
		}
	})

	t.Run("AbortRetention", func(t *testing.T) {
		s, _ := open(t)
		defer func() { _ = s.Close() }()
		run := "run-abort"
		mustBatch(t, s,
			vw(run, "decide", "exec-1", "Admit", `true`, ts(10)),
			flip(run, "decide", ValueStatusAborted, ts(20)),
		)
		cv, _ := s.CurrentVersion(run)
		if len(cv.Values["decide"]) != 0 {
			t.Fatalf("aborted values leaked into current version: %v", cv.Values)
		}
		fh, _ := s.FullHistory(run)
		if len(fh.Values) != 1 || fh.Values[0].Status != ValueStatusAborted {
			t.Fatalf("aborted write must be retained in full history: %+v", fh.Values)
		}
	})

	t.Run("LatestPerField", func(t *testing.T) {
		s, _ := open(t)
		defer func() { _ = s.Close() }()
		run := "run-latest"
		mustBatch(t, s,
			vw(run, "review", "exec-1", "Status", `"needs_revision"`, ts(10)),
			flip(run, "review", ValueStatusCommitted, ts(11)),
			vw(run, "review", "exec-2", "Status", `"approved"`, ts(20)),
			flip(run, "review", ValueStatusCommitted, ts(21)),
		)
		cv, _ := s.CurrentVersion(run)
		if !bytes.Equal(value(t, cv, "review", "Status"), []byte(`"approved"`)) {
			t.Fatalf("latest committed write must win: %s", value(t, cv, "review", "Status"))
		}
		fh, _ := s.FullHistory(run)
		if len(fh.Values) != 2 {
			t.Fatalf("superseded writes must remain in full history, got %d", len(fh.Values))
		}
	})

	t.Run("ReplayOrdering", func(t *testing.T) {
		s, _ := open(t)
		defer func() { _ = s.Close() }()
		run := "run-order"
		// Arrive out of chronological order, across separate batches.
		mustBatch(t, s, he(run, "NODE_COMPLETED", "exec-1", ts(30)))
		mustBatch(t, s, he(run, "PROCESS_STARTED", "exec-1", ts(0)))
		mustBatch(t, s, he(run, "NODE_STARTED", "exec-1", ts(15)))
		// Same-timestamp entries must come back in arrival order.
		mustBatch(t, s,
			he(run, "TIE_A", "exec-2", ts(40)),
			he(run, "TIE_B", "exec-2", ts(40)),
			he(run, "TIE_C", "exec-2", ts(40)),
		)
		fh, _ := s.FullHistory(run)
		var kinds []string
		for _, r := range fh.History {
			kinds = append(kinds, r.Kind)
		}
		want := []string{"PROCESS_STARTED", "NODE_STARTED", "NODE_COMPLETED", "TIE_A", "TIE_B", "TIE_C"}
		if fmt.Sprint(kinds) != fmt.Sprint(want) {
			t.Fatalf("replay order = %v, want %v", kinds, want)
		}
	})

	t.Run("ReadYourWrites", func(t *testing.T) {
		s, reopen := open(t)
		run := "run-durable"
		if err := s.Save(RunMetadata{RunID: run, ProcessID: "admissions", ProcessVersion: "1.0", Status: "running"}); err != nil {
			t.Fatalf("Save: %v", err)
		}
		mustBatch(t, s,
			vw(run, "screen", "exec-1", "Score", `88`, ts(10)),
			flip(run, "screen", ValueStatusCommitted, ts(11)),
			he(run, "NODE_COMPLETED", "exec-1", ts(11)),
		)
		if err := s.Flush(run); err != nil {
			t.Fatalf("Flush: %v", err)
		}
		check := func(t *testing.T, s StateStore) {
			t.Helper()
			cv, err := s.CurrentVersion(run)
			if err != nil {
				t.Fatalf("CurrentVersion: %v", err)
			}
			if !bytes.Equal(value(t, cv, "screen", "Score"), []byte(`88`)) {
				t.Fatalf("read-your-writes failed: %v", cv.Values)
			}
			fh, _ := s.FullHistory(run)
			if len(fh.Values) != 1 || len(fh.History) != 1 {
				t.Fatalf("full history incomplete after flush: %d values, %d entries", len(fh.Values), len(fh.History))
			}
		}
		check(t, s)
		if reopen == nil {
			if err := s.Close(); err != nil {
				t.Fatalf("Close: %v", err)
			}
			return
		}
		if err := s.Close(); err != nil {
			t.Fatalf("Close: %v", err)
		}
		s2 := reopen()
		defer func() { _ = s2.Close() }()
		check(t, s2)
	})

	t.Run("ConcurrentBatches", func(t *testing.T) {
		s, _ := open(t)
		defer func() { _ = s.Close() }()
		const workers, perWorker = 8, 20
		runs := []string{"run-conc-a", "run-conc-b"}
		var wg sync.WaitGroup
		errs := make(chan error, workers)
		for w := 0; w < workers; w++ {
			wg.Add(1)
			go func(w int) {
				defer wg.Done()
				run := runs[w%len(runs)]
				for i := 0; i < perWorker; i++ {
					task := fmt.Sprintf("task-%d", w)
					exec := fmt.Sprintf("exec-%d-%d", w, i)
					ops := []WriteOp{
						vw(run, task, exec, "N", fmt.Sprintf(`%d`, i), ts(w*100+i)),
						flip(run, task, ValueStatusCommitted, ts(w*100+i)),
						he(run, "NODE_COMPLETED", exec, ts(w*100+i)),
					}
					if err := s.WriteBatch(ops); err != nil {
						errs <- err
						return
					}
				}
			}(w)
		}
		wg.Wait()
		close(errs)
		for err := range errs {
			t.Fatalf("concurrent WriteBatch: %v", err)
		}
		total := 0
		for _, run := range runs {
			fh, err := s.FullHistory(run)
			if err != nil || fh == nil {
				t.Fatalf("FullHistory(%s): %v, %v", run, fh, err)
			}
			total += len(fh.Values)
			if len(fh.Values) != len(fh.History) {
				t.Fatalf("run %s: %d values vs %d history entries", run, len(fh.Values), len(fh.History))
			}
		}
		if total != workers*perWorker {
			t.Fatalf("lost writes: got %d value records, want %d", total, workers*perWorker)
		}
	})
}
