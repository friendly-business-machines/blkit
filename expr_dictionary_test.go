package blkit

import "testing"

func TestDictionary(t *testing.T) {
	assertEval(t, map[string]string{
		// literals + path access
		`{a: 1, b: 2}`:                  "{a: 1, b: 2}",
		`{name: "Alice", age: 30}.name`: "Alice",
		`{a: {b: 3}}.a.b`:               "3",
		`{a: 1}.missing`:                "null",
		// functions
		`getValue({foo: 123}, "foo")`:             "123",
		`getValue({x: 1, y: {z: 0}}, ["y", "z"])`: "0",
		`has({a: 1}, "a")`:                        "true",
		`has({a: 1}, "z")`:                        "false",
		`size({a: 1, b: 2})`:                      "2",
		`keys({b: 1, a: 2})`:                      `["a", "b"]`,
		`dictionaryRemove({a: 1, b: 2}, "a")`:     "{b: 2}",
		// equality is order-independent
		`{a: 1, b: 2} = {b: 2, a: 1}`: "true",
	})
}

func TestDictionaryForwardRefs(t *testing.T) {
	assertEval(t, map[string]string{
		`{a: 2, b: a * 2}`:             "{a: 2, b: 4}",
		`{a: 2, b: a * 2}.b`:           "4",
		`{x: 5, y: x + 1, z: y * 2}.z`: "12",
		`{a: 1, b: 2}`:                 "{a: 1, b: 2}", // no forward-ref still works
	})
}
