package blkit

import "strings"

// normalise performs the source-level rewrites expr's fixed lexer/parser needs,
// before parsing: FEEL single-equals to ==, if/then/else to expr's block form,
// and exact decimal-literal capture.
func normalise(source string) (string, error) {
	s := eqNorm(source)
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

// lowerIsDefined rewrites `isDefined(EXPR)` to __isDefined($env, "root"), where
// root is the leading identifier of EXPR. This keeps an unbound name from
// appearing as a variable reference and reports on the root binding for paths.
func lowerIsDefined(s string) string {
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
		root := leadingIdentifier(arg)
		if root == "" {
			return s
		}
		replacement := `__isDefined($env, "` + root + `")`
		s = s[:idx] + replacement + s[end+1:]
	}
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

// leadingIdentifier returns the leading identifier of an expression string.
func leadingIdentifier(s string) string {
	s = strings.TrimSpace(s)
	i := 0
	for i < len(s) && isIdentByte(s[i]) {
		i++
	}
	return s[:i]
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
		switch {
		case c == '"':
			i = skipString(s, i)
			continue
		case c == '(' || c == '[' || c == '{':
			depth++
		case c == ')' || c == ']' || c == '}':
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
