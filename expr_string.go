package blkit

import (
	"regexp"
	"strings"
	"unicode/utf8"

	"github.com/expr-lang/expr"
)

// BlString wraps a native Go string. Positions and length are counted in
// Unicode code points, not bytes.
type BlString struct{ s string }

func (BlString) Type() Type { return TypeString }

func (s BlString) Equal(other BlValue) BlValue {
	o, ok := other.(BlString)
	if !ok {
		return BlBoolean{false}
	}
	return BlBoolean{s.s == o.s}
}

func (s BlString) String() string { return s.s }

func (BlString) IsNull() bool { return false }

func (BlString) isBlValue() {}

// Native hands the underlying Go string back to host code.
func (s BlString) Native() string { return s.s }

// StringInput is the compile-time gate on host inputs to String.
type StringInput interface{ string | []byte }

// String constructs a BlString from a Go string (infallible) or a UTF-8
// []byte (validated; invalid UTF-8 returns an error).
func String[T StringInput](v T) (BlString, error) {
	switch x := any(v).(type) {
	case string:
		return BlString{x}, nil
	case []byte:
		if !utf8.Valid(x) {
			return BlString{}, &TypeError{Op: "String", Detail: "invalid UTF-8"}
		}
		return BlString{string(x)}, nil
	default:
		return BlString{}, &TypeError{Op: "String", Detail: "unsupported input"}
	}
}

// str is an internal infallible constructor.
func str(s string) BlString { return BlString{s} }

// --- operator impls -------------------------------------------------------

func concatStrings(a, b BlString) BlValue { return str(a.s + b.s) }

// --- library impls --------------------------------------------------------

func stringLengthFn(s BlString) BlNumber {
	return numFromInt(utf8.RuneCountInString(s.s))
}

func substringBeforeFn(s, match BlString) BlString {
	i := strings.Index(s.s, match.s)
	if i < 0 {
		return str("")
	}
	return str(s.s[:i])
}

func substringAfterFn(s, match BlString) BlString {
	i := strings.Index(s.s, match.s)
	if i < 0 {
		return str("")
	}
	return str(s.s[i+len(match.s):])
}

func upperCaseFn(s BlString) BlString    { return str(strings.ToUpper(s.s)) }
func lowerCaseFn(s BlString) BlString    { return str(strings.ToLower(s.s)) }
func trimFn(s BlString) BlString         { return str(strings.TrimSpace(s.s)) }
func trimLeadingFn(s BlString) BlString  { return str(strings.TrimLeftFunc(s.s, unicodeSpace)) }
func trimTrailingFn(s BlString) BlString { return str(strings.TrimRightFunc(s.s, unicodeSpace)) }

func unicodeSpace(r rune) bool {
	return strings.ContainsRune(" \t\n\r\f\v", r) || r == 0x85 || r == 0xA0
}

func containsFn(s, match BlString) BlBoolean   { return BlBoolean{strings.Contains(s.s, match.s)} }
func startsWithFn(s, match BlString) BlBoolean { return BlBoolean{strings.HasPrefix(s.s, match.s)} }
func endsWithFn(s, match BlString) BlBoolean   { return BlBoolean{strings.HasSuffix(s.s, match.s)} }

func isBlankFn(s BlString) BlBoolean {
	return BlBoolean{strings.TrimSpace(s.s) == ""}
}

func strIndexOfFn(s, match BlString) BlValue {
	i := strings.Index(s.s, match.s)
	if i < 0 {
		return Null()
	}
	return numFromInt(utf8.RuneCountInString(s.s[:i]) + 1)
}

func strIsEmptyFn(s BlString) BlBoolean { return BlBoolean{s.s == ""} }

func charAtFn(s BlString, pos BlNumber) BlString {
	runes := []rune(s.s)
	p := int(pos.d.IntPart())
	if p < 1 || p > len(runes) {
		return str("")
	}
	return str(string(runes[p-1]))
}

func strReverseFn(s BlString) BlString {
	runes := []rune(s.s)
	for i, j := 0, len(runes)-1; i < j; i, j = i+1, j-1 {
		runes[i], runes[j] = runes[j], runes[i]
	}
	return str(string(runes))
}

func repeatFn(s BlString, times BlNumber) BlString {
	n := int(times.d.IntPart())
	if n < 0 {
		return str("") // arity guard handled in registration; negative → empty
	}
	return str(strings.Repeat(s.s, n))
}

// variadic impls.

func substringFn(args ...any) (any, error) {
	s, ok := args[0].(BlString)
	if !ok {
		return nil, argTypeError(args[0])
	}
	start, ok := args[1].(BlNumber)
	if !ok {
		return nil, argTypeError(args[1])
	}
	runes := []rune(s.s)
	n := len(runes)
	st := int(start.d.IntPart())
	if st < 0 {
		st = n + st + 1
	}
	if st < 1 {
		st = 1
	}
	if st > n {
		return str(""), nil
	}
	end := n
	if len(args) > 2 {
		length, ok := args[2].(BlNumber)
		if !ok {
			return nil, argTypeError(args[2])
		}
		end = st - 1 + int(length.d.IntPart())
		if end > n {
			end = n
		}
	}
	if end < st-1 {
		end = st - 1
	}
	return str(string(runes[st-1 : end])), nil
}

func splitFn(args ...any) (any, error) {
	s, ok := args[0].(BlString)
	if !ok {
		return nil, argTypeError(args[0])
	}
	switch d := args[1].(type) {
	case BlString:
		parts := strings.Split(s.s, d.s)
		return listOfStrings(parts), nil
	case BlList:
		delims := make([]string, 0, len(d.items))
		for _, e := range d.items {
			ds, ok := e.(BlString)
			if !ok || ds.s == "" {
				return nil, &TypeError{Op: "split", Detail: "delimiter must be non-empty string"}
			}
			delims = append(delims, regexp.QuoteMeta(ds.s))
		}
		if len(delims) == 0 {
			return BlList{[]BlValue{s}}, nil
		}
		// longest-first alternation gives longest-match-wins.
		sortLongestFirst(delims)
		re := regexp.MustCompile(strings.Join(delims, "|"))
		return listOfStrings(re.Split(s.s, -1)), nil
	default:
		return nil, argTypeError(args[1])
	}
}

func matchesFn(args ...any) (any, error) {
	re, err := patternArg(args, "matches")
	if err != nil {
		return nil, err
	}
	s, ok := args[0].(BlString)
	if !ok {
		return nil, argTypeError(args[0])
	}
	anchored, err := regexp.Compile("^(?:" + re.source + ")$")
	if err != nil {
		return nil, &RegexError{Pattern: re.source, Err: err}
	}
	if re.flags != "" {
		anchored, err = regexp.Compile("(?" + re.flags + ")^(?:" + re.source + ")$")
		if err != nil {
			return nil, &RegexError{Pattern: re.source, Err: err}
		}
	}
	return BlBoolean{anchored.MatchString(s.s)}, nil
}

func replaceFn(args ...any) (any, error) {
	s, ok := args[0].(BlString)
	if !ok {
		return nil, argTypeError(args[0])
	}
	repl, ok := args[2].(BlString)
	if !ok {
		return nil, argTypeError(args[2])
	}
	re, err := patternArgAt(args, 1, len(args) == 4, "replace")
	if err != nil {
		return nil, err
	}
	return str(re.ReplaceAllString(s.s, repl.s)), nil
}

func extractFn(args ...any) (any, error) {
	s, ok := args[0].(BlString)
	if !ok {
		return nil, argTypeError(args[0])
	}
	re, err := patternArgAt(args, 1, len(args) == 3, "extract")
	if err != nil {
		return nil, err
	}
	matches := re.FindAllString(s.s, -1)
	return listOfStrings(matches), nil
}

func padLeadingFn(args ...any) (any, error)  { return padFn(args, true) }
func padTrailingFn(args ...any) (any, error) { return padFn(args, false) }

func padFn(args []any, leading bool) (any, error) {
	s, ok := args[0].(BlString)
	if !ok {
		return nil, argTypeError(args[0])
	}
	length, ok := args[1].(BlNumber)
	if !ok {
		return nil, argTypeError(args[1])
	}
	pad := " "
	if len(args) > 2 {
		pc, ok := args[2].(BlString)
		if !ok {
			return nil, argTypeError(args[2])
		}
		if utf8.RuneCountInString(pc.s) != 1 {
			return nil, &TypeError{Op: "pad", Detail: "padChar must be a single code point"}
		}
		pad = pc.s
	}
	target := int(length.d.IntPart())
	cur := utf8.RuneCountInString(s.s)
	if cur >= target {
		return s, nil
	}
	fill := strings.Repeat(pad, target-cur)
	if leading {
		return str(fill + s.s), nil
	}
	return str(s.s + fill), nil
}

func stringConvFn(args ...any) (any, error) {
	v := asBl(args[0])
	return str(v.String()), nil
}

// --- BlRegex --------------------------------------------------------------

// BlRegex is the precompiled-regex value type produced by pattern(s).
type BlRegex struct {
	source   string
	flags    string
	compiled *regexp.Regexp
}

func (BlRegex) Type() Type { return TypeRegex }

func (r BlRegex) Equal(other BlValue) BlValue {
	o, ok := other.(BlRegex)
	if !ok {
		return BlBoolean{false}
	}
	return BlBoolean{r.source == o.source}
}

func (r BlRegex) String() string { return r.source }

func (BlRegex) IsNull() bool { return false }

func (BlRegex) isBlValue() {}

// Pattern compiles a regex source into a BlRegex; a malformed source returns a
// RegexError.
func Pattern(source string) (BlRegex, error) {
	re, err := regexp.Compile(source)
	if err != nil {
		return BlRegex{}, &RegexError{Pattern: source, Err: err}
	}
	return BlRegex{source: source, compiled: re}, nil
}

// Source returns the original source string.
func (r BlRegex) Source() string { return r.source }

// Native returns the compiled program; do not mutate.
func (r BlRegex) Native() *regexp.Regexp { return r.compiled }

// regexSpec carries a pattern source plus optional flags for the match funcs.
type regexSpec struct {
	source string
	flags  string
}

// patternArg resolves the 2nd argument (index 1) of matches into a regexSpec,
// accepting a BlString source (optional 3rd flags arg) or a BlRegex.
func patternArg(args []any, fn string) (regexSpec, error) {
	switch p := args[1].(type) {
	case BlString:
		flags := ""
		if len(args) > 2 {
			f, ok := args[2].(BlString)
			if !ok {
				return regexSpec{}, argTypeError(args[2])
			}
			flags = f.s
		}
		return regexSpec{source: p.s, flags: flags}, nil
	case BlRegex:
		if len(args) > 2 {
			return regexSpec{}, &TypeError{Op: fn, Detail: "precompiled pattern cannot take flags"}
		}
		return regexSpec{source: p.source}, nil
	default:
		return regexSpec{}, argTypeError(args[1])
	}
}

// patternArgAt resolves a pattern argument at index idx into a compiled
// *regexp.Regexp for replace/extract (which don't anchor).
func patternArgAt(args []any, idx int, hasFlags bool, fn string) (*regexp.Regexp, error) {
	switch p := args[idx].(type) {
	case BlString:
		pat := p.s
		if hasFlags {
			f, ok := args[len(args)-1].(BlString)
			if !ok {
				return nil, argTypeError(args[len(args)-1])
			}
			pat = "(?" + f.s + ")" + pat
		}
		re, err := regexp.Compile(pat)
		if err != nil {
			return nil, &RegexError{Pattern: p.s, Err: err}
		}
		return re, nil
	case BlRegex:
		if hasFlags {
			return nil, &TypeError{Op: fn, Detail: "precompiled pattern cannot take flags"}
		}
		return p.compiled, nil
	default:
		return nil, argTypeError(args[idx])
	}
}

// stringOptions registers the string library. Parameter hints are BlValue
// (operator results flow through the VM as the interface); impls assert their
// concrete operand types at runtime. Same-arity overloads (e.g. the BlString vs
// BlRegex pattern slot) collapse to one BlValue hint and dispatch at runtime.
func stringOptions() []expr.Option {
	return []expr.Option{
		expr.Function("stringLength", typed1(stringLengthFn), new(func(BlValue) BlNumber)),
		expr.Function("substring", substringFn,
			new(func(BlValue, BlValue) BlString),
			new(func(BlValue, BlValue, BlValue) BlString)),
		expr.Function("substringBefore", typed2(substringBeforeFn), new(func(BlValue, BlValue) BlString)),
		expr.Function("substringAfter", typed2(substringAfterFn), new(func(BlValue, BlValue) BlString)),
		expr.Function("upperCase", typed1(upperCaseFn), new(func(BlValue) BlString)),
		expr.Function("lowerCase", typed1(lowerCaseFn), new(func(BlValue) BlString)),
		expr.Function("trim", typed1(trimFn), new(func(BlValue) BlString)),
		expr.Function("trimLeading", typed1(trimLeadingFn), new(func(BlValue) BlString)),
		expr.Function("trimTrailing", typed1(trimTrailingFn), new(func(BlValue) BlString)),
		// contains/startsWith/endsWith/matches are hard-coded as infix operators
		// in expr's lexer; normalise renames their call-form to these internal
		// names so they parse as ordinary function calls.
		expr.Function("__contains", typed2(containsFn), new(func(BlValue, BlValue) BlBoolean)),
		expr.Function("__startsWith", typed2(startsWithFn), new(func(BlValue, BlValue) BlBoolean)),
		expr.Function("__endsWith", typed2(endsWithFn), new(func(BlValue, BlValue) BlBoolean)),
		expr.Function("__matches", matchesFn,
			new(func(BlValue, BlValue) BlBoolean),
			new(func(BlValue, BlValue, BlValue) BlBoolean)),
		expr.Function("replace", replaceFn,
			new(func(BlValue, BlValue, BlValue) BlString),
			new(func(BlValue, BlValue, BlValue, BlValue) BlString)),
		expr.Function("split", splitFn, new(func(BlValue, BlValue) BlList)),
		expr.Function("extract", extractFn,
			new(func(BlValue, BlValue) BlList),
			new(func(BlValue, BlValue, BlValue) BlList)),
		expr.Function("pattern", typed1bl(patternFnWrap), new(func(BlValue) BlRegex)),
		expr.Function("isBlank", typed1(isBlankFn), new(func(BlValue) BlBoolean)),
		expr.Function("charAt", typed2(charAtFn), new(func(BlValue, BlValue) BlString)),
		// isEmpty / reverse / indexOf are unified cross-type dispatchers in
		// expr_overloads.go.
		expr.Function("padLeading", padLeadingFn,
			new(func(BlValue, BlValue) BlString),
			new(func(BlValue, BlValue, BlValue) BlString)),
		expr.Function("padTrailing", padTrailingFn,
			new(func(BlValue, BlValue) BlString),
			new(func(BlValue, BlValue, BlValue) BlString)),
		expr.Function("repeat", typed2(repeatFn), new(func(BlValue, BlValue) BlString)),
		expr.Function("string", stringConvFn, new(func(BlValue) BlString)),
	}
}

// patternFnWrap adapts Pattern's (BlRegex, error) shape to the engine.
func patternFnWrap(s BlString) (BlRegex, error) { return Pattern(s.s) }

// typed1bl wraps a func(BlString) (BlRegex, error) into the expr shape.
func typed1bl(f func(BlString) (BlRegex, error)) func(...any) (any, error) {
	return func(args ...any) (any, error) {
		s, ok := args[0].(BlString)
		if !ok {
			return nil, argTypeError(args[0])
		}
		return f(s)
	}
}
