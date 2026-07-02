package core

import "time"

// RetryConfig governs re-running fallible work on error. It is the process
// layer's shared retry policy — used by the native-function task and other
// retrying nodes when that layer lands (see process.spec.md). The decision
// layer's native-function node deliberately does not retry: it is for pure
// algorithms and models, so I/O or otherwise fallible work belongs in a
// process-layer native-function task instead.
type RetryConfig struct {
	MaxRetries         int           // maximum re-attempts after the first try; 0 = unset
	RetryFor           time.Duration // total time budget for retries; 0 = unset
	RetryDelay         time.Duration // delay between attempts
	ExponentialBackoff bool          // double the delay after each attempt
}

// RetryOpts configures NewRetryConfig.
type RetryOpts struct {
	MaxRetries         int
	RetryFor           time.Duration
	RetryDelay         time.Duration
	ExponentialBackoff bool
}

// NewRetryConfig builds a RetryConfig from options.
func NewRetryConfig(opts RetryOpts) *RetryConfig {
	return &RetryConfig{
		MaxRetries:         opts.MaxRetries,
		RetryFor:           opts.RetryFor,
		RetryDelay:         opts.RetryDelay,
		ExponentialBackoff: opts.ExponentialBackoff,
	}
}

// bounded reports whether the config sets at least one retry limit. A config with
// neither is rejected at node construction (it would mean unbounded retries).
func (c *RetryConfig) bounded() bool { return c.MaxRetries > 0 || c.RetryFor > 0 }
