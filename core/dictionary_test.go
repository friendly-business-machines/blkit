package core

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
		// values + entries (key-sorted ordering)
		`values({a: 1, b: 2})`:     "[1, 2]",
		`getEntries({a: 1, b: 2})`: `[{key: "a", value: 1}, {key: "b", value: 2}]`,
		// dictionaryPut adds/overwrites a key, returning a new dictionary
		`dictionaryPut({a: 1}, "b", 2)`: "{a: 1, b: 2}",
		// dictionaryMerge folds a list of dictionaries left→right (last wins)
		`dictionaryMerge([{a: 1}, {b: 2}])`:       "{a: 1, b: 2}",
		`dictionaryMerge([{a: 1}, {a: 9, b: 2}])`: "{a: 9, b: 2}",
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
