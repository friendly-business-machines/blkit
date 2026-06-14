package core

import "github.com/expr-lang/expr"

// Null-handling built-ins. BlNull itself and the shared helpers (propagatesNull)
// live in value.go alongside the BlValue interface.

// isNullFn reports whether v is BlNull.
func isNullFn(v BlValue) BlBoolean { return BlBoolean{v.IsNull()} }

// getOrElseFn returns fallback when v is BlNull, otherwise v unchanged. Only
// fires on BlNull — a defined-but-empty value is returned as-is.
func getOrElseFn(v, fallback BlValue) BlValue {
	if v.IsNull() {
		return fallback
	}
	return v
}

func nullOptions() []expr.Option {
	return []expr.Option{
		expr.Function("isNull", typed1(isNullFn), new(func(BlValue) BlBoolean)),
		expr.Function("getOrElse", typed2(getOrElseFn), new(func(BlValue, BlValue) BlValue)),
	}
}
