package core

import (
	"fmt"
	"reflect"

	"github.com/fxamacker/cbor/v2"
)

var mapStringAnyType = reflect.TypeOf(map[string]any(nil))

// The message-broker wire format (specs/message-brokers/overview.spec.md
// § Wire format): every published message — jobs, lifecycle events, registry
// entries — is a CBOR-encoded Envelope whose Payload is the CBOR encoding of
// the kind-specific body (ciphertext when a PayloadCipher is configured).
// Routing metadata is duplicated into broker-native message metadata by each
// backend and stays cleartext.

// EnvelopeVersion is the current envelope version. Backends reject envelopes
// with a version they do not understand.
const EnvelopeVersion = 1

// Envelope kinds. The kind names the payload's body type.
const (
	KindJobStart          = "job.start"       // StartJob
	KindJobRespondToInput = "job.respond"     // RespondToInputJob
	KindJobCancel         = "job.cancel"      // CancelJob
	KindJobTerminate      = "job.terminate"   // TerminateJob
	KindJobResume         = "job.resume"      // ResumeJob
	KindInstanceEvent     = "inst.event"      // InstanceEvent
	KindRegistration      = "registry.reg"    // ProcessRegistration
	KindRegistryUpdate    = "registry.update" // RegistryUpdate
)

// Envelope is the versioned wire wrapper around every broker message.
type Envelope struct {
	V              uint8   `cbor:"v"`
	Kind           string  `cbor:"k"`
	InstanceID     string  `cbor:"i,omitempty"`
	CorrelationKey *string `cbor:"c,omitempty"`
	KeyID          *string `cbor:"kid,omitempty"` // set when Payload is E2E-encrypted
	Nonce          []byte  `cbor:"n,omitempty"`   // set when Payload is E2E-encrypted
	Payload        []byte  `cbor:"p"`             // CBOR-encoded body (ciphertext when encrypted)
}

// UnknownEnvelopeVersionError reports an envelope whose version this build
// does not understand.
type UnknownEnvelopeVersionError struct {
	V uint8
}

func (e *UnknownEnvelopeVersionError) Error() string {
	return fmt.Sprintf("message broker: unknown envelope version %d", e.V)
}

// cborEnc / cborDec are the broker wire modes: RFC 3339 tagged times, and
// maps decoded as map[string]any so payload variable maps round-trip into
// the shapes producers submitted.
var cborEnc, cborDec = func() (cbor.EncMode, cbor.DecMode) {
	enc, err := cbor.EncOptions{
		Time:    cbor.TimeRFC3339Nano,
		TimeTag: cbor.EncTagRequired,
	}.EncMode()
	if err != nil {
		panic(err)
	}
	dec, err := cbor.DecOptions{
		DefaultMapType: mapStringAnyType,
	}.DecMode()
	if err != nil {
		panic(err)
	}
	return enc, dec
}()

// EncodeEnvelope encodes body as the CBOR payload of a version-1 envelope and
// returns the CBOR encoding of the whole envelope. With a non-nil cipher the
// payload is encrypted and the envelope carries the cipher's key id and the
// nonce.
func EncodeEnvelope(kind, instanceID string, correlationKey *string, body any, cipher PayloadCipher) ([]byte, error) {
	payload, err := cborEnc.Marshal(body)
	if err != nil {
		return nil, fmt.Errorf("message broker: encode %s payload: %w", kind, err)
	}
	env := Envelope{
		V:              EnvelopeVersion,
		Kind:           kind,
		InstanceID:     instanceID,
		CorrelationKey: correlationKey,
		Payload:        payload,
	}
	if cipher != nil {
		ciphertext, nonce, err := cipher.Encrypt(payload)
		if err != nil {
			return nil, fmt.Errorf("message broker: encrypt %s payload: %w", kind, err)
		}
		keyID := cipher.KeyID()
		env.KeyID = &keyID
		env.Nonce = nonce
		env.Payload = ciphertext
	}
	data, err := cborEnc.Marshal(env)
	if err != nil {
		return nil, fmt.Errorf("message broker: encode envelope: %w", err)
	}
	return data, nil
}

// DecodeEnvelope decodes an envelope, rejecting unknown versions, and — when
// the payload is encrypted — decrypts it with the supplied cipher. A nil
// cipher on an encrypted envelope, or a key id the cipher cannot resolve, is
// an error; the payload is never processed.
func DecodeEnvelope(data []byte, cipher PayloadCipher) (*Envelope, error) {
	var env Envelope
	if err := cborDec.Unmarshal(data, &env); err != nil {
		return nil, fmt.Errorf("message broker: decode envelope: %w", err)
	}
	if env.V != EnvelopeVersion {
		return nil, &UnknownEnvelopeVersionError{V: env.V}
	}
	if env.KeyID != nil {
		if cipher == nil {
			return nil, &UnknownKeyIDError{KeyID: *env.KeyID}
		}
		plaintext, err := cipher.Decrypt(*env.KeyID, env.Nonce, env.Payload)
		if err != nil {
			return nil, fmt.Errorf("message broker: decrypt payload: %w", err)
		}
		env.KeyID = nil
		env.Nonce = nil
		env.Payload = plaintext
	}
	return &env, nil
}

// DecodePayload decodes the (decrypted) envelope payload into the kind's body
// type.
func (e *Envelope) DecodePayload(into any) error {
	if err := cborDec.Unmarshal(e.Payload, into); err != nil {
		return fmt.Errorf("message broker: decode %s payload: %w", e.Kind, err)
	}
	return nil
}
