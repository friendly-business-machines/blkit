package core

import "strings"

// normalise performs the source-level rewrites expr's fixed lexer/parser needs,
// before parsing: FEEL single-equals to ==, if/then/else to expr's block form,
// and exact decimal-literal capture.
func normalise(source string) (string, error) {
	s := eqNorm(source)
	s = lowerNamedArgs(s)
	s = lowerInlinePredicates(s)
	s = lowerInlineFunctions(s)
	s = lowerSequence(s)
	s = lowerTableIndex(s)
	s = lowerInstanceOf(s)
	s = lowerIsDefined(s)
	s = rewriteIdentifiers(s)
	s = lowerRanges(s)
	s = lowerBetween(s)
	s = lowerComprehensions(s)
	s = convertConditionals(s)
	s = captureDecimals(s)
	return s, nil
}

// namedArgParams maps a function name to its ordered parameter names, enabling
// named-argument calls like substring(string: "x", startPosition: 3).
var namedArgParams = map[string][]string{
	"substring":       {"string", "startPosition", "length"},
	"substringBefore": {"string", "match"},
	"substringAfter":  {"string", "match"},
	"padLeading":      {"string", "length", "padChar"},
	"padTrailing":     {"string", "length", "padChar"},
	"repeat":          {"string", "times"},
	"charAt":          {"string", "position"},
	"contains":        {"string", "match"},
	"startsWith":      {"string", "match"},
	"endsWith":        {"string", "match"},
	"matches":         {"input", "pattern", "flags"},
	"replace":         {"input", "pattern", "replacement", "flags"},
	"split":           {"string", "delimiter"},
	"extract":         {"string", "pattern", "flags"},
	"round":           {"n", "scale"},
	"roundUp":         {"n", "scale"},
	"roundDown":       {"n", "scale"},
	"roundHalfUp":     {"n", "scale"},
	"roundHalfDown":   {"n", "scale"},
	"roundHalfEven":   {"n", "scale"},
	"clamp":           {"n", "min", "max"},
	"modulo":          {"dividend", "divisor"},
	"log":             {"n", "base"},
	"number":          {"from", "groupingSeparator", "decimalSeparator"},
	"sublist":         {"list", "start", "length"},
}

// lowerNamedArgs rewrites named-argument calls (`fn(name: value, …)`) into
// positional form using the parameter-name registry. Runs before the `:`
// sequence rewrite so named-arg colons aren't mistaken for sequence operators.
func lowerNamedArgs(s string) string {
	for fn, params := range namedArgParams {
		s = rewriteNamedCall(s, fn, params)
	}
	return s
}

func rewriteNamedCall(s, fn string, params []string) string {
	from := 0
	for {
		idx := indexTopLevelCallFrom(s, fn, from)
		if idx < 0 {
			return s
		}
		j := idx + len(fn)
		for j < len(s) && (s[j] == ' ' || s[j] == '\t') {
			j++
		}
		close := matchGroupEnd(s, j, '(', ')')
		if close < 0 {
			return s
		}
		args := splitTopLevelCommas(s[j+1 : close])
		named := map[string]string{}
		allNamed := len(args) > 0
		for _, a := range args {
			name, val, ok := parseNamedArg(a)
			if !ok {
				allNamed = false
				break
			}
			named[name] = val
		}
		if allNamed {
			var ordered []string
			for _, p := range params {
				if v, ok := named[p]; ok {
					ordered = append(ordered, v)
				}
			}
			if len(ordered) == len(named) {
				repl := fn + "(" + strings.Join(ordered, ", ") + ")"
				s = s[:idx] + repl + s[close+1:]
				from = idx + len(repl)
				continue
			}
		}
		from = idx + len(fn)
	}
}

// parseNamedArg parses `name: value` (name a bare identifier, value the rest);
// ok is false if the argument isn't in named form.
func parseNamedArg(arg string) (name, value string, ok bool) {
	t := strings.TrimSpace(arg)
	i := 0
	for i < len(t) && isIdentByte(t[i]) {
		i++
	}
	if i == 0 || !isIdentStart(t[0]) {
		return "", "", false
	}
	name = t[:i]
	for i < len(t) && (t[i] == ' ' || t[i] == '\t') {
		i++
	}
	if i >= len(t) || t[i] != ':' {
		return "", "", false
	}
	value = strings.TrimSpace(t[i+1:])
	if value == "" {
		return "", "", false
	}
	return name, value, true
}

// lowerSequence rewrites the `start:end` sequence operator to seq(start, end,
// 1). A `:` whose innermost enclosing bracket is `{` is a dictionary
// key-separator and is left alone; every other top-level `:` is a sequence
// operator. Operands bind looser than arithmetic but tighter than comparison /
// boolean operators.
func lowerSequence(s string) string {
	for {
		colon := firstSequenceColon(s)
		if colon < 0 {
			return s
		}
		ls := seqOperandLeft(s, colon)
		re := seqOperandRight(s, colon+1)
		// keep separating whitespace in the surrounding text so adjacent
		// keywords/tokens don't merge with the replacement
		for ls < colon && (s[ls] == ' ' || s[ls] == '\t') {
			ls++
		}
		for re > colon+1 && (s[re-1] == ' ' || s[re-1] == '\t') {
			re--
		}
		left := strings.TrimSpace(s[ls:colon])
		right := strings.TrimSpace(s[colon+1 : re])
		if left == "" || right == "" {
			return s
		}
		replacement := "seq(" + left + ", " + right + ", 1)"
		s = s[:ls] + replacement + s[re:]
	}
}

// firstSequenceColon finds the first `:` that is a sequence operator (innermost
// enclosing bracket is not `{`), skipping strings; -1 if none.
func firstSequenceColon(s string) int {
	var stack []byte
	for i := 0; i < len(s); {
		c := s[i]
		switch c {
		case '"':
			i = skipString(s, i)
			continue
		case '(', '[', '{':
			stack = append(stack, c)
		case ')', ']', '}':
			if len(stack) > 0 {
				stack = stack[:len(stack)-1]
			}
		case ':':
			if len(stack) == 0 || stack[len(stack)-1] != '{' {
				return i
			}
		}
		i++
	}
	return -1
}

// seqBoundaryKeywords break a sequence operand (they bind looser than `:`).
var seqBoundaryKeywords = []string{"and", "or", "in", "between", "return", "satisfies", "then", "else"}

func seqKeywordAt(s string, i int) (int, bool) {
	for _, k := range seqBoundaryKeywords {
		if matchKeyword(s, i, k) {
			return len(k), true
		}
	}
	return 0, false
}

// seqGroupStart returns the index just inside the innermost bracket enclosing
// the colon (or 0 at top level).
func seqGroupStart(s string, colon int) int {
	var stack []int
	for i := 0; i < colon; {
		c := s[i]
		if c == '"' {
			i = skipString(s, i)
			continue
		}
		switch c {
		case '(', '[', '{':
			stack = append(stack, i)
		case ')', ']', '}':
			if len(stack) > 0 {
				stack = stack[:len(stack)-1]
			}
		}
		i++
	}
	if len(stack) == 0 {
		return 0
	}
	return stack[len(stack)-1] + 1
}

// seqOperandLeft finds the start of the colon's left operand, scoped to the
// colon's enclosing bracket group.
func seqOperandLeft(s string, colon int) int {
	start := seqGroupStart(s, colon)
	depth := 0
	for i := start; i < colon; {
		c := s[i]
		switch {
		case c == '"':
			i = skipString(s, i)
			continue
		case c == '(' || c == '[' || c == '{':
			depth++
		case c == ')' || c == ']' || c == '}':
			depth--
		case depth == 0 && (c == ',' || c == ':'):
			start = i + 1
		case depth == 0 && (c == '<' || c == '>' || c == '=' || c == '!'):
			start = i + 1
			if i+1 < colon && s[i+1] == '=' {
				start = i + 2
			}
		default:
			if depth == 0 {
				if n, ok := seqKeywordAt(s, i); ok {
					start = i + n
					i += n
					continue
				}
			}
		}
		i++
	}
	return start
}

// seqOperandRight finds the end of the colon's right operand.
func seqOperandRight(s string, from int) int {
	depth := 0
	for i := from; i < len(s); {
		c := s[i]
		switch {
		case c == '"':
			i = skipString(s, i)
			continue
		case c == '(' || c == '[' || c == '{':
			depth++
		case c == ')' || c == ']' || c == '}':
			if depth == 0 {
				return i
			}
			depth--
		case depth == 0 && (c == ',' || c == ':'):
			return i
		case depth == 0 && (c == '<' || c == '>' || c == '=' || c == '!'):
			return i
		default:
			if depth == 0 {
				if _, ok := seqKeywordAt(s, i); ok {
					return i
				}
			}
		}
		i++
	}
	return len(s)
}

// lowerInlineFunctions rewrites a standalone inline function `function(p1, p2)
// body` to __mkfunc("p1,p2", "body") (the body becomes an escaped string
// literal, recompiled when the function is applied). Runs after the
// remove/listReplace predicate-form rewrite, which consumes those usages first.
func lowerInlineFunctions(s string) string {
	for {
		idx := findKeywordCall(s, "function", 0)
		if idx < 0 {
			return s
		}
		open := idx + len("function")
		for open < len(s) && (s[open] == ' ' || s[open] == '\t') {
			open++
		}
		paramClose := matchGroupEnd(s, open, '(', ')')
		if paramClose < 0 {
			return s
		}
		params := strings.TrimSpace(s[open+1 : paramClose])
		bodyStart := paramClose + 1
		bodyEnd := funcBodyEnd(s, bodyStart)
		body := strings.TrimSpace(s[bodyStart:bodyEnd])
		if body == "" {
			return s
		}
		repl := `__mkfunc("` + params + `", "` + escapeStringLiteral(body) + `")`
		s = s[:idx] + repl + s[bodyEnd:]
	}
}

// funcBodyEnd finds the end of an inline-function body: the enclosing group's
// close, a top-level comma, or end of input.
func funcBodyEnd(s string, from int) int {
	depth := 0
	for i := from; i < len(s); {
		c := s[i]
		switch {
		case c == '"':
			i = skipString(s, i)
			continue
		case c == '(' || c == '[' || c == '{':
			depth++
		case c == ')' || c == ']' || c == '}':
			if depth == 0 {
				return i
			}
			depth--
		case depth == 0 && c == ',':
			return i
		}
		i++
	}
	return len(s)
}

func escapeStringLiteral(s string) string {
	var b strings.Builder
	for _, r := range s {
		switch r {
		case '\\':
			b.WriteString(`\\`)
		case '"':
			b.WriteString(`\"`)
		default:
			b.WriteRune(r)
		}
	}
	return b.String()
}

// lowerInlinePredicates rewrites the predicate (inline-function) forms of
// remove / listReplace, the one documented use of `function(p) body`:
//
//	remove(L, function(i) PRED)        → (L)[not (PRED[i→item])]
//	listReplace(L, function(i) P, R)   → (for __lr in L return (if P[i→__lr] then R else __lr))
//
// The positional forms (numeric position) are left untouched. General
// first-class function values are not supported.
func lowerInlinePredicates(s string) string {
	s = rewriteInlinePredicate(s, "remove")
	s = rewriteInlinePredicate(s, "listReplace")
	return s
}

func rewriteInlinePredicate(s, fn string) string {
	from := 0
	for {
		idx := indexTopLevelCallFrom(s, fn, from)
		if idx < 0 {
			return s
		}
		j := idx + len(fn)
		for j < len(s) && (s[j] == ' ' || s[j] == '\t') {
			j++
		}
		close := matchGroupEnd(s, j, '(', ')')
		if close < 0 {
			return s
		}
		args := splitTopLevelCommas(s[j+1 : close])
		if len(args) >= 2 && strings.HasPrefix(strings.TrimSpace(args[1]), "function(") {
			list := strings.TrimSpace(args[0])
			param, body, ok := parseInlineFunction(strings.TrimSpace(args[1]))
			if ok {
				var repl string
				if fn == "remove" {
					repl = "(" + list + ")[not (" + replaceIdentifier(body, param, "item") + ")]"
				} else if len(args) >= 3 {
					rep := strings.TrimSpace(args[2])
					repl = "(for __lr in " + list + " return (if " + replaceIdentifier(body, param, "__lr") + " then " + rep + " else __lr))"
				}
				if repl != "" {
					s = s[:idx] + repl + s[close+1:]
					from = idx + len(repl)
					continue
				}
			}
		}
		from = idx + len(fn)
	}
}

// findKeywordCall finds a standalone keyword `name` (at any bracket depth)
// immediately followed by `(`, skipping strings; -1 if none.
func findKeywordCall(s, name string, from int) int {
	for i := from; i < len(s); {
		if s[i] == '"' {
			i = skipString(s, i)
			continue
		}
		if matchKeyword(s, i, name) {
			j := i + len(name)
			for j < len(s) && (s[j] == ' ' || s[j] == '\t') {
				j++
			}
			if j < len(s) && s[j] == '(' {
				return i
			}
		}
		i++
	}
	return -1
}

// parseInlineFunction parses `function(p) body` into its first parameter name
// and body text.
func parseInlineFunction(s string) (param, body string, ok bool) {
	open := strings.IndexByte(s, '(')
	if open < 0 {
		return "", "", false
	}
	close := matchGroupEnd(s, open, '(', ')')
	if close < 0 {
		return "", "", false
	}
	params := splitTopLevelCommas(s[open+1 : close])
	if len(params) == 0 {
		return "", "", false
	}
	param = strings.TrimSpace(params[0])
	body = strings.TrimSpace(s[close+1:])
	if param == "" || body == "" {
		return "", "", false
	}
	return param, body, true
}

// replaceIdentifier replaces whole-word occurrences of `from` with `to`,
// skipping string literals.
func replaceIdentifier(s, from, to string) string {
	var b strings.Builder
	for i := 0; i < len(s); {
		c := s[i]
		if c == '"' {
			end := skipString(s, i)
			b.WriteString(s[i:end])
			i = end
			continue
		}
		if isIdentStart(c) {
			k := i + 1
			for k < len(s) && isIdentByte(s[k]) {
				k++
			}
			word := s[i:k]
			if word == from {
				b.WriteString(to)
			} else {
				b.WriteString(word)
			}
			i = k
			continue
		}
		b.WriteByte(c)
		i++
	}
	return b.String()
}

// indexTopLevelCallFrom is indexTopLevelCall starting the search at `from`.
func indexTopLevelCallFrom(s, name string, from int) int {
	for {
		idx := indexTopLevelKeyword(s, name, from)
		if idx < 0 {
			return -1
		}
		j := idx + len(name)
		for j < len(s) && (s[j] == ' ' || s[j] == '\t') {
			j++
		}
		if j < len(s) && s[j] == '(' {
			return idx
		}
		from = idx + len(name)
	}
}

// lowerTableIndex rewrites the two-slot bracket forms `t[r, c]` → tableIndex(t,
// r, c) and `t[, c]` → tableCols(t, c) before parsing (expr can't lex the comma
// inside an index). Only index brackets (preceded by a value) with a top-level
// comma are rewritten; list literals `[a, b]` are left alone.
func lowerTableIndex(s string) string {
	for {
		bracket := firstTwoSlotBracket(s)
		if bracket < 0 {
			return s
		}
		close := matchGroupEnd(s, bracket, '[', ']')
		if close < 0 {
			return s
		}
		inner := s[bracket+1 : close]
		commaIdx := indexTopLevelComma(inner)
		recvStart := scanReceiverStart(s, bracket)
		recv := lowerTableIndex(s[recvStart:bracket])
		var replacement string
		if commaIdx == 0 || strings.TrimSpace(inner[:commaIdx]) == "" {
			col := lowerTableIndex(strings.TrimSpace(inner[commaIdx+1:]))
			replacement = "tableCols(" + recv + ", " + col + ")"
		} else {
			row := lowerTableIndex(strings.TrimSpace(inner[:commaIdx]))
			col := lowerTableIndex(strings.TrimSpace(inner[commaIdx+1:]))
			replacement = "tableIndex(" + recv + ", " + row + ", " + col + ")"
		}
		s = s[:recvStart] + replacement + s[close+1:]
	}
}

// firstTwoSlotBracket returns the index of the first index-position `[` whose
// content has a top-level comma, or -1.
func firstTwoSlotBracket(s string) int {
	for i := 0; i < len(s); {
		c := s[i]
		if c == '"' {
			i = skipString(s, i)
			continue
		}
		if c == '[' && isIndexBracket(s, i) {
			close := matchGroupEnd(s, i, '[', ']')
			if close > i && indexTopLevelComma(s[i+1:close]) >= 0 {
				return i
			}
		}
		i++
	}
	return -1
}

// bracketKeywords are operator/keyword tokens that may precede a `[` but do not
// make it an index — the bracket starts a fresh list/range operand.
var bracketKeywords = map[string]bool{
	"in": true, "and": true, "or": true, "not": true, "return": true,
	"satisfies": true, "then": true, "else": true, "if": true,
	"instance": true, "of": true, "between": true,
	"true": true, "false": true, "null": true, "nil": true,
}

// isIndexBracket reports whether the `[` at s[i] follows a value (making it an
// index/selector) rather than starting a list literal or range operand.
func isIndexBracket(s string, i int) bool {
	j := i - 1
	for j >= 0 && (s[j] == ' ' || s[j] == '\t') {
		j--
	}
	if j < 0 {
		return false
	}
	c := s[j]
	if c == ')' || c == ']' || c == '"' {
		return true
	}
	if !isIdentByte(c) {
		return false
	}
	// read the preceding identifier and reject operator keywords
	start := j
	for start >= 0 && isIdentByte(s[start]) {
		start--
	}
	return !bracketKeywords[s[start+1:j+1]]
}

// indexTopLevelComma returns the first top-level comma index, or -1.
func indexTopLevelComma(s string) int {
	depth := 0
	for i := 0; i < len(s); {
		c := s[i]
		switch {
		case c == '"':
			i = skipString(s, i)
			continue
		case c == '(' || c == '[' || c == '{':
			depth++
		case c == ')' || c == ']' || c == '}':
			depth--
		case depth == 0 && c == ',':
			return i
		}
		i++
	}
	return -1
}

// scanReceiverStart finds the start of the postfix-chain receiver expression
// ending just before the bracket at s[bracket].
func scanReceiverStart(s string, bracket int) int {
	i := bracket - 1
	for i >= 0 {
		c := s[i]
		switch {
		case c == ')' || c == ']' || c == '}':
			i = matchOpenBackward(s, i) - 1
		case isIdentByte(c) || c == '.':
			i--
		default:
			return i + 1
		}
	}
	return 0
}

// matchOpenBackward returns the index of the opening bracket matching the close
// at s[i], scanning backward (bracket kinds counted uniformly).
func matchOpenBackward(s string, i int) int {
	depth := 0
	for j := i; j >= 0; j-- {
		switch s[j] {
		case ')', ']', '}':
			depth++
		case '(', '[', '{':
			depth--
			if depth == 0 {
				return j
			}
		}
	}
	return 0
}

// lowerInstanceOf rewrites `X instance of T` to __instanceOf(X, "T"). Runs
// before rewriteIdentifiers so the type name `null` is captured verbatim.
func lowerInstanceOf(s string) string {
	for {
		idx := indexTopLevelKeyword(s, "instance", 0)
		if idx < 0 {
			return s
		}
		ofIdx := indexTopLevelKeyword(s, "of", idx+len("instance"))
		if ofIdx < 0 || ofIdx > idx+len("instance")+3 {
			return s // not the `instance of` pair
		}
		leftStart := leftOperandStart(s, idx)
		left := strings.TrimSpace(s[leftStart:idx])
		// read the type identifier after `of`
		i := ofIdx + len("of")
		for i < len(s) && (s[i] == ' ' || s[i] == '\t') {
			i++
		}
		tStart := i
		for i < len(s) && isIdentByte(s[i]) {
			i++
		}
		typeName := s[tStart:i]
		if typeName == "" {
			return s
		}
		replacement := "__instanceOf(" + left + `, "` + typeName + `")`
		s = s[:leftStart] + replacement + s[i:]
	}
}

// lowerIsDefined rewrites `isDefined(EXPR)`. A bare declared name always resolves
// (it is an env-struct field), so isDefined(name) → __isBound(name) — passing the
// value so the strict checker still rejects an undeclared name, while the impl
// reports true. A dotted path → __hasKey(parent, "leaf"), which probes whether
// the parent dictionary contains the leaf key (a missing key is "not defined",
// distinct from a present null).
func lowerIsDefined(s string) string {
	s = mapTopLevelGroups(s, lowerIsDefined)
	for {
		idx := indexTopLevelCall(s, "isDefined")
		if idx < 0 {
			return s
		}
		open := idx + len("isDefined")
		// skip spaces to '('
		j := open
		for j < len(s) && (s[j] == ' ' || s[j] == '\t') {
			j++
		}
		if j >= len(s) || s[j] != '(' {
			return s
		}
		end := matchGroupEnd(s, j, '(', ')')
		if end < 0 {
			return s
		}
		arg := strings.TrimSpace(s[j+1 : end])
		replacement := isDefinedReplacement(arg)
		if replacement == "" {
			return s
		}
		s = s[:idx] + replacement + s[end+1:]
	}
}

// isDefinedReplacement chooses the lowering for an isDefined argument: a dotted
// identifier path becomes a dictionary key probe; a bare name (or any other
// expression) becomes a bound check that resolves to true while still forcing
// the checker to validate the referenced name.
func isDefinedReplacement(arg string) string {
	if arg == "" {
		return ""
	}
	if parent, leaf, ok := splitTrailingKey(arg); ok {
		return `__hasKey(` + parent + `, "` + leaf + `")`
	}
	return "__isBound(" + arg + ")"
}

// splitTrailingKey splits a pure dotted identifier path `a.b.c` into its parent
// path `a.b` and trailing key `c`. It returns ok only for dotted identifier
// paths, not bracket indexes or calls.
func splitTrailingKey(arg string) (parent, leaf string, ok bool) {
	dot := strings.LastIndexByte(arg, '.')
	if dot <= 0 || dot == len(arg)-1 {
		return "", "", false
	}
	parent = strings.TrimSpace(arg[:dot])
	leaf = strings.TrimSpace(arg[dot+1:])
	if !isDottedIdentifierPath(parent) || !isIdentifier(leaf) {
		return "", "", false
	}
	return parent, leaf, true
}

// isDottedIdentifierPath reports whether s is one or more identifiers joined by
// dots (`a`, `a.b`, `applicant.address`).
func isDottedIdentifierPath(s string) bool {
	if s == "" {
		return false
	}
	for _, part := range strings.Split(s, ".") {
		if !isIdentifier(strings.TrimSpace(part)) {
			return false
		}
	}
	return true
}

// isIdentifier reports whether s is a single identifier (leading letter or
// underscore, then letters/digits/underscores).
func isIdentifier(s string) bool {
	if s == "" || (s[0] >= '0' && s[0] <= '9') {
		return false
	}
	for i := 0; i < len(s); i++ {
		if !isIdentByte(s[i]) {
			return false
		}
	}
	return true
}

// indexTopLevelCall finds a standalone function-name keyword at depth 0
// immediately followed (after optional spaces) by '('.
func indexTopLevelCall(s, name string) int {
	from := 0
	for {
		idx := indexTopLevelKeyword(s, name, from)
		if idx < 0 {
			return -1
		}
		j := idx + len(name)
		for j < len(s) && (s[j] == ' ' || s[j] == '\t') {
			j++
		}
		if j < len(s) && s[j] == '(' {
			return idx
		}
		from = idx + len(name)
	}
}

// callRenames maps the FEEL function names that collide with expr's hard-coded
// infix operators to internal names that parse as ordinary calls.
var callRenames = map[string]string{
	"contains":   "__contains",
	"startsWith": "__startsWith",
	"endsWith":   "__endsWith",
	"matches":    "__matches",
}

// rewriteIdentifiers canonicalises boolean/null literal casing (true/false/null
// in any case → true/false/nil) and renames collision built-ins used in call
// position, scanning identifier tokens and skipping strings.
func rewriteIdentifiers(s string) string {
	var b strings.Builder
	for i := 0; i < len(s); {
		c := s[i]
		if c == '"' {
			end := skipString(s, i)
			b.WriteString(s[i:end])
			i = end
			continue
		}
		if isIdentStart(c) {
			j := i + 1
			for j < len(s) && isIdentByte(s[j]) {
				j++
			}
			word := s[i:j]
			switch strings.ToLower(word) {
			case "true":
				b.WriteString("true")
			case "false":
				b.WriteString("false")
			case "null":
				b.WriteString("nil")
			default:
				if repl, ok := callRenames[word]; ok && nextNonSpaceIsParen(s, j) {
					b.WriteString(repl)
				} else {
					b.WriteString(word)
				}
			}
			i = j
			continue
		}
		b.WriteByte(c)
		i++
	}
	return b.String()
}

func isIdentStart(b byte) bool {
	return b == '_' || (b >= 'a' && b <= 'z') || (b >= 'A' && b <= 'Z')
}

func nextNonSpaceIsParen(s string, i int) bool {
	for i < len(s) && (s[i] == ' ' || s[i] == '\t') {
		i++
	}
	return i < len(s) && s[i] == '('
}

// lowerRanges rewrites range literals `[a..b]` / `(a..b)` / `[a..b)` / `(a..b]`
// to newRange(a, b, startIncluded, endIncluded). A bracket group whose direct
// content has a top-level `..` is a range; otherwise it is a list/paren group
// and is recursed into.
func lowerRanges(s string) string {
	var b strings.Builder
	for i := 0; i < len(s); {
		c := s[i]
		if c == '"' {
			end := skipString(s, i)
			b.WriteString(s[i:end])
			i = end
			continue
		}
		if c == '(' || c == '[' {
			open := c
			closeCh := matchingClose(open)
			j := matchGroupEnd(s, i, open, closeCh)
			if j < 0 {
				b.WriteByte(c)
				i++
				continue
			}
			inner := s[i+1 : j]
			if dd := indexTopLevelDotDot(inner); dd >= 0 {
				left := lowerRanges(strings.TrimSpace(inner[:dd]))
				right := lowerRanges(strings.TrimSpace(inner[dd+2:]))
				startInc := boolLit(open == '[')
				endInc := boolLit(s[j] == ']')
				b.WriteString("newRange(" + left + ", " + right + ", " + startInc + ", " + endInc + ")")
			} else {
				b.WriteByte(open)
				b.WriteString(lowerRanges(inner))
				b.WriteByte(s[j])
			}
			i = j + 1
			continue
		}
		b.WriteByte(c)
		i++
	}
	return b.String()
}

func boolLit(v bool) string {
	if v {
		return "true"
	}
	return "false"
}

// matchGroupEnd returns the index where the bracket group opened at s[i] closes.
// All bracket kinds count toward depth uniformly so a range literal's
// mismatched delimiters (`[a..b)`, `(a..b]`) are matched correctly.
func matchGroupEnd(s string, i int, open, closeCh byte) int {
	depth := 0
	for j := i; j < len(s); {
		switch s[j] {
		case '"':
			j = skipString(s, j)
			continue
		case '(', '[', '{':
			depth++
		case ')', ']', '}':
			depth--
			if depth == 0 {
				return j
			}
		}
		j++
	}
	return -1
}

// indexTopLevelDotDot returns the index of a top-level `..` in s, or -1.
func indexTopLevelDotDot(s string) int {
	depth := 0
	for i := 0; i+1 < len(s); {
		c := s[i]
		switch {
		case c == '"':
			i = skipString(s, i)
			continue
		case c == '(' || c == '[' || c == '{':
			depth++
		case c == ')' || c == ']' || c == '}':
			depth--
		case depth == 0 && c == '.' && s[i+1] == '.':
			return i
		}
		i++
	}
	return -1
}

// lowerBetween rewrites `X between A and B` to `(X >= A and X <= B)`, handling
// the operands at bracket depth 0.
func lowerBetween(s string) string {
	for {
		idx := indexTopLevelKeyword(s, "between", 0)
		if idx < 0 {
			return s
		}
		leftStart := leftOperandStart(s, idx)
		left := strings.TrimSpace(s[leftStart:idx])
		aStart := idx + len("between")
		andIdx := indexTopLevelKeyword(s, "and", aStart)
		if andIdx < 0 {
			return s // malformed; leave for the parser to reject
		}
		a := strings.TrimSpace(s[aStart:andIdx])
		bStart := andIdx + len("and")
		bEnd := rightOperandEnd(s, bStart)
		bExpr := strings.TrimSpace(s[bStart:bEnd])
		replacement := "(" + left + " >= " + a + " and " + left + " <= " + bExpr + ")"
		s = s[:leftStart] + replacement + s[bEnd:]
	}
}

// leftOperandStart scans back from a `between` keyword to the start of its left
// operand — the position just after the previous top-level boolean keyword or
// opening bracket / comma.
func leftOperandStart(s string, between int) int {
	depth := 0
	start := 0
	for i := 0; i < between; {
		c := s[i]
		switch {
		case c == '"':
			i = skipString(s, i)
			continue
		case c == '(' || c == '[' || c == '{':
			depth++
		case c == ')' || c == ']' || c == '}':
			depth--
		case depth == 0 && c == ',':
			start = i + 1
		case depth == 0 && (matchKeyword(s, i, "and") || matchKeyword(s, i, "or")):
			start = i + len("and")
			if matchKeyword(s, i, "or") {
				start = i + len("or")
			}
			i += 2
			continue
		}
		i++
	}
	return start
}

// rightOperandEnd scans forward from the start of `between`'s upper operand to
// its end — the next top-level boolean keyword, closing bracket, comma, or end.
func rightOperandEnd(s string, from int) int {
	depth := 0
	for i := from; i < len(s); {
		c := s[i]
		switch {
		case c == '"':
			i = skipString(s, i)
			continue
		case c == '(' || c == '[' || c == '{':
			depth++
		case c == ')' || c == ']' || c == '}':
			if depth == 0 {
				return i
			}
			depth--
		case depth == 0 && c == ',':
			return i
		case depth == 0 && (matchKeyword(s, i, "and") || matchKeyword(s, i, "or")):
			return i
		}
		i++
	}
	return len(s)
}

func isIdentByte(b byte) bool {
	return b == '_' ||
		(b >= 'a' && b <= 'z') ||
		(b >= 'A' && b <= 'Z') ||
		(b >= '0' && b <= '9')
}

// skipString returns the index just past the closing quote of a string literal
// that begins at s[i] == '"'.
func skipString(s string, i int) int {
	i++ // opening quote
	for i < len(s) {
		if s[i] == '\\' {
			i += 2
			continue
		}
		if s[i] == '"' {
			return i + 1
		}
		i++
	}
	return i
}

// eqNorm rewrites a single = to ==, leaving ==, <=, >=, != untouched, skipping
// string literals.
func eqNorm(s string) string {
	var b strings.Builder
	for i := 0; i < len(s); {
		c := s[i]
		if c == '"' {
			end := skipString(s, i)
			b.WriteString(s[i:end])
			i = end
			continue
		}
		if c == '=' {
			var prev, next byte
			if i > 0 {
				prev = s[i-1]
			}
			if i+1 < len(s) {
				next = s[i+1]
			}
			switch {
			case prev == '<' || prev == '>' || prev == '!' || prev == '=':
				b.WriteByte('=')
			case next == '=':
				b.WriteString("==")
				i++
			default:
				b.WriteString("==")
			}
			i++
			continue
		}
		b.WriteByte(c)
		i++
	}
	return b.String()
}

// matchKeyword reports whether the word `kw` occurs at s[i] as a standalone
// identifier (delimited by non-identifier bytes on both sides).
func matchKeyword(s string, i int, kw string) bool {
	if i+len(kw) > len(s) || s[i:i+len(kw)] != kw {
		return false
	}
	if i > 0 && isIdentByte(s[i-1]) {
		return false
	}
	if i+len(kw) < len(s) && isIdentByte(s[i+len(kw)]) {
		return false
	}
	return true
}

// indexTopLevelKeyword returns the byte index of the first standalone keyword at
// bracket depth 0 at or after `from`, or -1.
func indexTopLevelKeyword(s, kw string, from int) int {
	depth := 0
	for i := from; i < len(s); {
		c := s[i]
		switch c {
		case '"':
			i = skipString(s, i)
			continue
		case '(', '[', '{':
			depth++
		case ')', ']', '}':
			depth--
		}
		if depth == 0 && matchKeyword(s, i, kw) {
			return i
		}
		i++
	}
	return -1
}

// mapTopLevelGroups replaces the inner text of every top-level bracket group
// with fn(inner), leaving everything else (and the brackets) intact.
func mapTopLevelGroups(s string, fn func(string) string) string {
	var b strings.Builder
	for i := 0; i < len(s); {
		c := s[i]
		if c == '"' {
			end := skipString(s, i)
			b.WriteString(s[i:end])
			i = end
			continue
		}
		if c == '(' || c == '[' || c == '{' {
			open := c
			close := matchingClose(open)
			depth := 1
			j := i + 1
			for j < len(s) && depth > 0 {
				switch s[j] {
				case '"':
					j = skipString(s, j)
					continue
				case open:
					depth++
				case close:
					depth--
					if depth == 0 {
						goto done
					}
				}
				j++
			}
		done:
			b.WriteByte(open)
			if j <= len(s) && i+1 <= j {
				inner := s[i+1 : min(j, len(s))]
				b.WriteString(fn(inner))
			}
			if j < len(s) {
				b.WriteByte(close)
			}
			i = j + 1
			continue
		}
		b.WriteByte(c)
		i++
	}
	return b.String()
}

func matchingClose(open byte) byte {
	switch open {
	case '(':
		return ')'
	case '[':
		return ']'
	case '{':
		return '}'
	}
	return 0
}

// convertConditionals rewrites FEEL `if C then A else B` into expr's
// parenthesised block form `(if C { A } else { B })`, recursing into nested
// conditionals in the condition, branches, and bracket groups.
func convertConditionals(s string) string {
	s = mapTopLevelGroups(s, convertConditionals)
	ifIdx := indexTopLevelKeyword(s, "if", 0)
	if ifIdx < 0 {
		return s
	}
	thenIdx := indexTopLevelKeyword(s, "then", ifIdx+2)
	if thenIdx < 0 {
		return s // not a FEEL conditional we recognise
	}
	elseIdx := indexTopLevelKeyword(s, "else", thenIdx+4)
	if elseIdx < 0 {
		return s
	}
	pre := s[:ifIdx]
	cond := strings.TrimSpace(s[ifIdx+2 : thenIdx])
	thenB := strings.TrimSpace(s[thenIdx+4 : elseIdx])
	elseB := strings.TrimSpace(s[elseIdx+4:])

	condT := convertConditionals(cond)
	thenT := convertConditionals(thenB)
	elseT := convertConditionals(elseB)
	return pre + "(if " + condT + " { " + thenT + " } else { " + elseT + " })"
}

// captureDecimals rewrites every fractional/exponent numeric literal to its
// exact number("…") constructor form, so the full decimal precision survives
// expr's lossy float lexing. Integer literals are left untouched.
func captureDecimals(s string) string {
	var b strings.Builder
	for i := 0; i < len(s); {
		c := s[i]
		if c == '"' {
			end := skipString(s, i)
			b.WriteString(s[i:end])
			i = end
			continue
		}
		if c >= '0' && c <= '9' && numberStartsHere(s, i) {
			tok, next, fractional := scanNumber(s, i)
			if fractional {
				b.WriteString(`number("`)
				b.WriteString(tok)
				b.WriteString(`")`)
			} else {
				b.WriteString(tok)
			}
			i = next
			continue
		}
		b.WriteByte(c)
		i++
	}
	return b.String()
}

// numberStartsHere reports whether s[i] begins a fresh numeric literal (not a
// continuation of an identifier or a member-access digit).
func numberStartsHere(s string, i int) bool {
	if i == 0 {
		return true
	}
	prev := s[i-1]
	if prev == '_' || prev == '.' ||
		(prev >= 'a' && prev <= 'z') ||
		(prev >= 'A' && prev <= 'Z') ||
		(prev >= '0' && prev <= '9') {
		return false
	}
	return true
}

// scanNumber consumes a numeric literal starting at s[i], returning its text,
// the index just past it, and whether it is fractional/exponent (and so needs
// exact capture).
func scanNumber(s string, i int) (tok string, next int, fractional bool) {
	start := i
	for i < len(s) && s[i] >= '0' && s[i] <= '9' {
		i++
	}
	// fraction: `.` followed by a digit (not the `..` range operator)
	if i+1 < len(s) && s[i] == '.' && s[i+1] >= '0' && s[i+1] <= '9' {
		fractional = true
		i++
		for i < len(s) && s[i] >= '0' && s[i] <= '9' {
			i++
		}
	}
	// exponent
	if i < len(s) && (s[i] == 'e' || s[i] == 'E') {
		j := i + 1
		if j < len(s) && (s[j] == '+' || s[j] == '-') {
			j++
		}
		if j < len(s) && s[j] >= '0' && s[j] <= '9' {
			fractional = true
			i = j
			for i < len(s) && s[i] >= '0' && s[i] <= '9' {
				i++
			}
		}
	}
	return s[start:i], i, fractional
}
