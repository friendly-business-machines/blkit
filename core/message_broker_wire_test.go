package core

import (
	"bytes"
	"errors"
	"testing"
)

func TestEnvelopeRoundTrip(t *testing.T) {
	body := StartJob{
		InstanceID: "i-1",
		StartID:    "start",
		Input:      map[string]any{"amount": 42.5, "note": "hello", "nested": map[string]any{"ok": true}},
	}
	corr := "corr-1"
	data, err := EncodeEnvelope(KindJobStart, "i-1", &corr, body, nil)
	if err != nil {
		t.Fatalf("encode: %v", err)
	}
	env, err := DecodeEnvelope(data, nil)
	if err != nil {
		t.Fatalf("decode: %v", err)
	}
	if env.Kind != KindJobStart || env.InstanceID != "i-1" || env.CorrelationKey == nil || *env.CorrelationKey != corr {
		t.Fatalf("envelope metadata round trip: %+v", env)
	}
	var out StartJob
	if err := env.DecodePayload(&out); err != nil {
		t.Fatalf("decode payload: %v", err)
	}
	if out.Input["note"] != "hello" || out.Input["amount"] != 42.5 {
		t.Fatalf("payload round trip: %+v", out.Input)
	}
	nested, ok := out.Input["nested"].(map[string]any)
	if !ok || nested["ok"] != true {
		t.Fatalf("nested map must decode as map[string]any, got %T", out.Input["nested"])
	}
}

func TestEnvelopeCipher(t *testing.T) {
	key := bytes.Repeat([]byte{7}, 32)
	cipher, err := NewAESGCMPayloadCipher("k1", key)
	if err != nil {
		t.Fatalf("cipher: %v", err)
	}
	body := StartJob{InstanceID: "i-1", StartID: "start", Input: map[string]any{"secret": "s3cr3t"}}
	data, err := EncodeEnvelope(KindJobStart, "i-1", nil, body, cipher)
	if err != nil {
		t.Fatalf("encode: %v", err)
	}
	if bytes.Contains(data, []byte("s3cr3t")) {
		t.Fatal("ciphertext envelope must not contain the plaintext payload")
	}

	// Without the cipher the payload must be rejected, not processed.
	if _, err := DecodeEnvelope(data, nil); err == nil {
		t.Fatal("decoding an encrypted envelope without a cipher must fail")
	}
	// A different key id must be rejected.
	other, _ := NewAESGCMPayloadCipher("other", bytes.Repeat([]byte{9}, 32))
	var unknown *UnknownKeyIDError
	if _, err := DecodeEnvelope(data, other); !errors.As(err, &unknown) {
		t.Fatalf("want UnknownKeyIDError, got %v", err)
	}
	// The right cipher round-trips.
	env, err := DecodeEnvelope(data, cipher)
	if err != nil {
		t.Fatalf("decode: %v", err)
	}
	var out StartJob
	if err := env.DecodePayload(&out); err != nil {
		t.Fatalf("decode payload: %v", err)
	}
	if out.Input["secret"] != "s3cr3t" {
		t.Fatalf("payload round trip: %+v", out.Input)
	}
}

func TestEnvelopeUnknownVersion(t *testing.T) {
	data, err := cborEnc.Marshal(Envelope{V: 99, Kind: "x", Payload: []byte{0xf6}})
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	var unknown *UnknownEnvelopeVersionError
	if _, err := DecodeEnvelope(data, nil); !errors.As(err, &unknown) {
		t.Fatalf("want UnknownEnvelopeVersionError, got %v", err)
	}
}

// Contracts travel through the wire format: a registration's InputContract
// must survive CBOR and still validate.
func TestProcessRegistrationContractRoundTrip(t *testing.T) {
	contract, err := NewInputContract(
		RequiredField("amount", TypeNumber),
		OptionalField("note", TypeString),
		Field{Name: "applicant", Type: TypeDictionary, Fields: BlSchema{
			{Name: "name", Type: TypeString},
		}},
	)
	if err != nil {
		t.Fatalf("contract: %v", err)
	}
	reg := ProcessRegistration{
		Namespace:   "example.com/x",
		ProcessID:   "p",
		Version:     "1.0",
		StartEvents: []StartEventInfo{{Id: "start", InputContract: contract}},
	}
	data, err := EncodeEnvelope(KindRegistration, "", nil, reg, nil)
	if err != nil {
		t.Fatalf("encode: %v", err)
	}
	env, err := DecodeEnvelope(data, nil)
	if err != nil {
		t.Fatalf("decode: %v", err)
	}
	var out ProcessRegistration
	if err := env.DecodePayload(&out); err != nil {
		t.Fatalf("decode payload: %v", err)
	}
	c := out.StartEvents[0].InputContract
	if c == nil {
		t.Fatal("contract lost in transit")
	}
	valid := map[string]any{"amount": 10.0, "applicant": map[string]any{"name": "Ada"}}
	if err := c.Validate(valid); err != nil {
		t.Fatalf("valid input rejected after round trip: %v", err)
	}
	var dcv *DataContractValidationError
	if err := c.Validate(map[string]any{"amount": "nope", "applicant": map[string]any{"name": "Ada"}}); !errors.As(err, &dcv) {
		t.Fatalf("want DataContractValidationError after round trip, got %v", err)
	}
}
