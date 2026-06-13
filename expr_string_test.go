package blkit

import "testing"

func TestStringConstruction(t *testing.T) {
	if s, err := String("hello"); err != nil || s.Native() != "hello" {
		t.Errorf("String(hello) = %v, %v", s, err)
	}
	if s, err := String([]byte{0x68, 0x65, 0x6C, 0x6C, 0x6F}); err != nil || s.Native() != "hello" {
		t.Errorf("String([]byte) = %v, %v", s, err)
	}
	if _, err := String([]byte{0x68, 0xFF, 0xFE}); err == nil {
		t.Errorf("expected invalid-UTF-8 error")
	}
}

func TestStringLiteralsAndLength(t *testing.T) {
	assertEval(t, map[string]string{
		`stringLength("hello")`: "5",
		`stringLength("a\nb")`:  "3",
		`stringLength("🎉")`:     "1",
		`stringLength("🎉a")`:    "2",
		`stringLength("é")`:     "1",
	})
}

func TestStringOperators(t *testing.T) {
	assertEval(t, map[string]string{
		`"foo" + "bar"`:          "foobar",
		`"order-" + string(123)`: "order-123",
		`"a" = "A"`:              "false",
	})
}

func TestStringFunctions(t *testing.T) {
	assertEval(t, map[string]string{
		`substring("foobar", 3, 2)`:   "ob",
		`substring("foobar", 3)`:      "obar",
		`substring("foobar", -2)`:     "ar",
		`substringBefore("a-b", "-")`: "a",
		`substringAfter("a-b", "-")`:  "b",
		`upperCase("aBc")`:            "ABC",
		`lowerCase("aBc")`:            "abc",
		`trim("  hi  ")`:              "hi",
		`trimLeading("  hi  ")`:       "hi  ",
		`trimTrailing("  hi  ")`:      "  hi",
		`contains("foobar", "oo")`:    "true",
		`startsWith("foo", "fo")`:     "true",
		`endsWith("foo", "oo")`:       "true",
		`isBlank("  ")`:               "true",
		`isEmpty("")`:                 "true",
		`indexOf("hello", "l")`:       "3",
		`indexOf("hello", "z")`:       "null",
		`charAt("hello", 1)`:          "h",
		`charAt("hello", 99)`:         "", // out of range → ""
		`substring("hi", 5)`:          "", // start past end → ""
		`reverse("abc")`:              "cba",
		`padLeading("7", 4, "0")`:     "0007",
		`padTrailing("hi", 5, ".")`:   "hi...",
		`repeat("ab", 3)`:             "ababab",
	})
}

func TestStringRegex(t *testing.T) {
	assertEval(t, map[string]string{
		`matches("hello", "h.+o")`:                                        "true",
		`matches("ABC", "[a-z]+", "i")`:                                   "true",
		`matches("page 42", "\\d+")`:                                      "false",
		`matches("42", "\\d+")`:                                           "true",
		`replace("a-b-c", "-", "/")`:                                      "a/b/c",
		`replace("2025-03-28", "(\\d{4})-(\\d{2})-(\\d{2})", "$3/$2/$1")`: "28/03/2025",
	})
}

func TestStringSplitExtract(t *testing.T) {
	assertEval(t, map[string]string{
		`split("a,b,c", ",")`:          `["a", "b", "c"]`,
		`extract("id 12, 34", "\\d+")`: `["12", "34"]`,
		`extract("x", "\\d+")`:         `[]`,
	})
}

func TestStringPattern(t *testing.T) {
	assertEval(t, map[string]string{
		`matches("alice@example.com", pattern("[\\w.+-]+@[\\w-]+\\.[\\w.-]+"))`: "true",
	})
	re, err := Pattern(`[0-9]+`)
	if err != nil || re.Source() != `[0-9]+` {
		t.Errorf("Pattern() = %v, %v", re, err)
	}
	if _, err := Pattern(`[0-9`); err == nil {
		t.Errorf("expected RegexError for malformed pattern")
	}
}

func TestNamedArguments(t *testing.T) {
	assertEval(t, map[string]string{
		`substring(string: "foobar", startPosition: 3, length: 2)`: "ob",
		`substring(string: "foobar", startPosition: 3)`:            "obar",
		`contains(string: "foobar", match: "oob")`:                 "true",
		`round(n: 2.345, scale: 2)`:                                "2.35",
		`substring("foobar", 3, 2)`:                                "ob", // positional still works
	})
}
