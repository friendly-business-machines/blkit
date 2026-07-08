package core

import (
	"context"
	"errors"
	"testing"
	"time"
)

// The in-memory backend runs the shared conformance suite in-process — no
// setup, part of the normal test run. Queue removal is exact for the
// in-memory queues, and short TTL/timeout knobs keep the timing subtests
// fast.
func TestInMemoryMessageBrokerConformance(t *testing.T) {
	const (
		ttl      = 150 * time.Millisecond
		inFlight = 300 * time.Millisecond
	)
	RunMessageBrokerConformance(t, func(t *testing.T) MessageBroker {
		return NewInMemoryMessageBroker(
			WithRegistrationTTL(ttl),
			WithInFlightTimeout(inFlight),
		)
	}, BrokerConformanceOptions{
		SupportsQueueRemoval: true,
		RegistrationTTL:      ttl,
		InFlightTimeout:      inFlight,
		OpenWithCipher: func(t *testing.T) MessageBroker {
			cipher, err := NewAESGCMPayloadCipher("k1", make([]byte, 32))
			if err != nil {
				t.Fatalf("cipher: %v", err)
			}
			return NewInMemoryMessageBroker(
				WithRegistrationTTL(ttl),
				WithInFlightTimeout(inFlight),
				WithPayloadCipher(cipher),
			)
		},
	})
}

// A full subscriber buffer drops events and synthesizes BACKPRESSURE_DROP
// when the buffer recovers.
func TestInMemoryMessageBrokerBackpressureDrop(t *testing.T) {
	b := NewInMemoryMessageBroker(WithEventBufferSize(2))
	defer func() { _ = b.Close() }()
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	reg := conformanceRegistration(t, "a", false, false)
	mustRegister(t, ctx, b, "w1", reg)
	id, err := b.Submit(ctx, conformanceStart("a", "start", map[string]any{"amount": 1.0}))
	if err != nil {
		t.Fatalf("submit: %v", err)
	}
	events, err := b.SubscribeToInstance(ctx, id)
	if err != nil {
		t.Fatalf("subscribe: %v", err)
	}
	// Nothing is reading: flood past the 2-slot buffer so events drop.
	for range 8 {
		if err := b.PostError(ctx, id, InstanceError{Code: ErrCodeTaskFailed, Message: "retryable"}); err != nil {
			t.Fatalf("post error: %v", err)
		}
	}
	drainDeadline := time.After(3 * time.Second)
	sawDrop := false
	for !sawDrop {
		select {
		case e := <-events:
			if e.Kind == InstanceEventError && e.Error.Code == ErrCodeBackpressureDrop {
				sawDrop = true
				continue
			}
			// Keep draining; the synthetic drop marker arrives once a slot
			// frees up for the next publish.
			if err := b.PostError(ctx, id, InstanceError{Code: ErrCodeTaskFailed, Message: "more"}); err != nil {
				t.Fatalf("post error: %v", err)
			}
		case <-drainDeadline:
			t.Fatal("never saw BACKPRESSURE_DROP")
		}
	}
}

// Close is idempotent, releases subscribers, and fails subsequent calls with
// ErrBrokerClosed.
func TestInMemoryMessageBrokerClose(t *testing.T) {
	b := NewInMemoryMessageBroker()
	ctx := context.Background()

	events, err := b.SubscribeToInstance(ctx, "some-instance")
	if err != nil {
		t.Fatalf("subscribe: %v", err)
	}
	if err := b.Close(); err != nil {
		t.Fatalf("close: %v", err)
	}
	if err := b.Close(); err != nil {
		t.Fatalf("second close: %v", err)
	}
	awaitClose(t, events)
	if _, err := b.Submit(ctx, conformanceStart("a", "start", nil)); !errors.Is(err, ErrBrokerClosed) {
		t.Fatalf("submit after close: want ErrBrokerClosed, got %v", err)
	}
	if _, err := b.SubscribeToInstance(ctx, "x"); !errors.Is(err, ErrBrokerClosed) {
		t.Fatalf("subscribe after close: want ErrBrokerClosed, got %v", err)
	}
}

// An unknown instance's first event is INSTANCE_NOT_FOUND.
func TestInMemoryMessageBrokerUnknownInstanceSubscription(t *testing.T) {
	b := NewInMemoryMessageBroker()
	defer func() { _ = b.Close() }()
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	events, err := b.SubscribeToInstance(ctx, "never-existed")
	if err != nil {
		t.Fatalf("subscribe: %v", err)
	}
	e := awaitEvent(t, events, func(e InstanceEvent) bool { return e.Kind == InstanceEventError })
	if e.Error.Code != ErrCodeInstanceNotFound {
		t.Fatalf("want INSTANCE_NOT_FOUND, got %+v", e.Error)
	}
}
