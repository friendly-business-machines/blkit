package blkit

import "fmt"

// ParseError is returned by Expr / UnaryTest when the source cannot be parsed
// or type-checked.
type ParseError struct {
	Source string
	Err    error
}

func (e *ParseError) Error() string {
	if e.Err != nil {
		return fmt.Sprintf("parse error in %q: %v", e.Source, e.Err)
	}
	return fmt.Sprintf("parse error in %q", e.Source)
}

func (e *ParseError) Unwrap() error { return e.Err }

// TypeError is returned by Evaluate (or a host constructor) on a runtime type
// mismatch.
type TypeError struct {
	Op     string
	Detail string
}

func (e *TypeError) Error() string {
	if e.Detail != "" {
		return fmt.Sprintf("type error in %s: %s", e.Op, e.Detail)
	}
	return fmt.Sprintf("type error in %s", e.Op)
}

// RegexError is returned when a pattern in matches / replace / extract /
// pattern(...) is malformed.
type RegexError struct {
	Pattern string
	Err     error
}

func (e *RegexError) Error() string {
	return fmt.Sprintf("regex error in %q: %v", e.Pattern, e.Err)
}

func (e *RegexError) Unwrap() error { return e.Err }

// CalendarRangeError is returned when business-day iteration steps past a
// calendar's validity bounds under strictCalendarRange.
type CalendarRangeError struct {
	Detail string
}

func (e *CalendarRangeError) Error() string {
	return fmt.Sprintf("calendar range error: %s", e.Detail)
}

// SchemaError is returned by BlSchema validation, carrying a path to the
// offending node.
type SchemaError struct {
	Path string
	Err  error
}

func (e *SchemaError) Error() string {
	if e.Path == "" {
		return fmt.Sprintf("schema error: %v", e.Err)
	}
	return fmt.Sprintf("schema error at %s: %v", e.Path, e.Err)
}

func (e *SchemaError) Unwrap() error { return e.Err }
