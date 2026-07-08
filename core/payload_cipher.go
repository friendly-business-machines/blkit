package core

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
	"fmt"
)

// PayloadCipher encrypts envelope payloads end-to-end: producers and workers
// hold the cipher; the broker only ever sees ciphertext. Routing metadata
// stays cleartext so the broker can route what it cannot read
// (specs/message-brokers/overview.spec.md § Transport security).
type PayloadCipher interface {
	// KeyID identifies the key in use; it rides in the envelope so the
	// receiving side can select the right key (and keys can rotate).
	KeyID() string
	Encrypt(plaintext []byte) (ciphertext, nonce []byte, err error)
	Decrypt(keyID string, nonce, ciphertext []byte) ([]byte, error)
}

// ErrUnknownKeyID is wrapped by Decrypt when an envelope names a key the
// cipher does not hold. The receiver never processes such a payload.
type UnknownKeyIDError struct {
	KeyID string
}

func (e *UnknownKeyIDError) Error() string {
	return fmt.Sprintf("payload cipher: unknown key id %q", e.KeyID)
}

type aesGCMPayloadCipher struct {
	keyID string
	aead  cipher.AEAD
}

// NewAESGCMPayloadCipher returns a PayloadCipher using AES-256-GCM. key must
// be exactly 32 bytes; keys are distributed out-of-band by the application.
func NewAESGCMPayloadCipher(keyID string, key []byte) (PayloadCipher, error) {
	if keyID == "" {
		return nil, fmt.Errorf("payload cipher: empty key id")
	}
	if len(key) != 32 {
		return nil, fmt.Errorf("payload cipher: AES-256-GCM needs a 32-byte key, got %d bytes", len(key))
	}
	block, err := aes.NewCipher(key)
	if err != nil {
		return nil, err
	}
	aead, err := cipher.NewGCM(block)
	if err != nil {
		return nil, err
	}
	return &aesGCMPayloadCipher{keyID: keyID, aead: aead}, nil
}

func (c *aesGCMPayloadCipher) KeyID() string { return c.keyID }

func (c *aesGCMPayloadCipher) Encrypt(plaintext []byte) (ciphertext, nonce []byte, err error) {
	nonce = make([]byte, c.aead.NonceSize())
	if _, err := rand.Read(nonce); err != nil {
		return nil, nil, err
	}
	return c.aead.Seal(nil, nonce, plaintext, nil), nonce, nil
}

func (c *aesGCMPayloadCipher) Decrypt(keyID string, nonce, ciphertext []byte) ([]byte, error) {
	if keyID != c.keyID {
		return nil, &UnknownKeyIDError{KeyID: keyID}
	}
	if len(nonce) != c.aead.NonceSize() {
		return nil, fmt.Errorf("payload cipher: bad nonce length %d", len(nonce))
	}
	return c.aead.Open(nil, nonce, ciphertext, nil)
}
