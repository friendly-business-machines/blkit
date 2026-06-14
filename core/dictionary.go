package core

import (
	"sort"
	"strings"

	"github.com/expr-lang/expr"
)

// BlDictionary is an unordered map of string keys to BlValues. Key-surfacing
// operations emit code-point-sorted order for determinism.
type BlDictionary struct {
	keys []string
	m    map[string]BlValue
}

func (BlDictionary) Type() Type { return TypeDictionary }

func (d BlDictionary) Equal(other BlValue) BlValue {
	o, ok := other.(BlDictionary)
	if !ok || len(d.keys) != len(o.keys) {
		return BlBoolean{false}
	}
	for _, k := range d.keys {
		ov, present := o.get(k)
		if !present {
			return BlBoolean{false}
		}
		if eq, ok := d.m[k].Equal(ov).(BlBoolean); !ok || !eq.b {
			return BlBoolean{false}
		}
	}
	return BlBoolean{true}
}

func (d BlDictionary) String() string {
	ks := d.sortedKeys()
	parts := make([]string, len(ks))
	for i, k := range ks {
		parts[i] = k + ": " + literalString(d.m[k])
	}
	return "{" + strings.Join(parts, ", ") + "}"
}

func (BlDictionary) IsNull() bool { return false }

func (BlDictionary) isBlValue() {}

func (d BlDictionary) sortedKeys() []string {
	ks := append([]string{}, d.keys...)
	sort.Strings(ks)
	return ks
}

func newDictionary() BlDictionary {
	return BlDictionary{m: map[string]BlValue{}}
}

func (d *BlDictionary) set(k string, v BlValue) {
	if d.m == nil {
		d.m = map[string]BlValue{}
	}
	if _, exists := d.m[k]; !exists {
		d.keys = append(d.keys, k)
	}
	d.m[k] = v
}

func (d BlDictionary) get(k string) (BlValue, bool) {
	v, ok := d.m[k]
	return v, ok
}

// Dictionary constructs a BlDictionary from a map of BlValues.
func Dictionary(entries map[string]BlValue) (BlDictionary, error) {
	d := newDictionary()
	for k := range entries {
		if k == "" {
			return BlDictionary{}, &TypeError{Op: "Dictionary", Detail: "empty key"}
		}
	}
	keys := make([]string, 0, len(entries))
	for k := range entries {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	for _, k := range keys {
		d.set(k, entries[k])
	}
	return d, nil
}

// Native returns a defensive copy of the underlying map.
func (d BlDictionary) Native() map[string]BlValue {
	out := make(map[string]BlValue, len(d.keys))
	for _, k := range d.keys {
		out[k] = d.m[k]
	}
	return out
}

// --- functions ------------------------------------------------------------

func getValueFn(args ...any) (any, error) {
	d, ok := args[0].(BlDictionary)
	if !ok {
		return nil, argTypeError(args[0])
	}
	switch key := args[1].(type) {
	case BlString:
		if v, present := d.get(key.s); present {
			return v, nil
		}
		return Null(), nil
	case BlList:
		var cur BlValue = d
		for _, k := range key.items {
			ks, ok := k.(BlString)
			if !ok {
				return Null(), nil
			}
			dd, ok := cur.(BlDictionary)
			if !ok {
				return Null(), nil
			}
			v, present := dd.get(ks.s)
			if !present {
				return Null(), nil
			}
			cur = v
		}
		return cur, nil
	default:
		return nil, argTypeError(args[1])
	}
}

func getEntriesFn(d BlDictionary) BlList {
	ks := d.sortedKeys()
	out := make([]BlValue, len(ks))
	for i, k := range ks {
		entry := newDictionary()
		entry.set("key", str(k))
		entry.set("value", d.m[k])
		out[i] = entry
	}
	return BlList{out}
}

func dictionaryPutFn(args ...any) (any, error) {
	d, ok := args[0].(BlDictionary)
	if !ok {
		return nil, argTypeError(args[0])
	}
	val := asBl(args[2])
	switch key := args[1].(type) {
	case BlString:
		if key.s == "" {
			return nil, &TypeError{Op: "dictionaryPut", Detail: "empty key"}
		}
		return dictWith(d, key.s, val), nil
	case BlList:
		return dictPutPath(d, key.items, val)
	default:
		return nil, argTypeError(args[1])
	}
}

func dictWith(d BlDictionary, key string, val BlValue) BlDictionary {
	out := newDictionary()
	for _, k := range d.keys {
		out.set(k, d.m[k])
	}
	out.set(key, val)
	return out
}

func dictPutPath(d BlDictionary, path []BlValue, val BlValue) (BlValue, error) {
	if len(path) == 0 {
		return d, nil
	}
	ks, ok := path[0].(BlString)
	if !ok {
		return nil, &TypeError{Op: "dictionaryPut", Detail: "path keys must be strings"}
	}
	if len(path) == 1 {
		return dictWith(d, ks.s, val), nil
	}
	inner, _ := d.get(ks.s)
	innerDict, ok := inner.(BlDictionary)
	if !ok {
		innerDict = newDictionary()
	}
	updated, err := dictPutPath(innerDict, path[1:], val)
	if err != nil {
		return nil, err
	}
	return dictWith(d, ks.s, updated), nil
}

func dictionaryMergeFn(l BlList) BlDictionary {
	out := newDictionary()
	for _, e := range l.items {
		d, ok := e.(BlDictionary)
		if !ok {
			continue
		}
		for _, k := range d.keys {
			out.set(k, d.m[k])
		}
	}
	return out
}

func keysFn(d BlDictionary) BlList {
	return listOfStrings(d.sortedKeys())
}

func valuesFn(d BlDictionary) BlList {
	ks := d.sortedKeys()
	out := make([]BlValue, len(ks))
	for i, k := range ks {
		out[i] = d.m[k]
	}
	return BlList{out}
}

func hasFn(d BlDictionary, key BlString) BlBoolean {
	_, present := d.get(key.s)
	return BlBoolean{present}
}

func dictSizeFn(d BlDictionary) BlNumber     { return numFromInt(len(d.keys)) }
func dictIsEmptyFn(d BlDictionary) BlBoolean { return BlBoolean{len(d.keys) == 0} }

func dictionaryRemoveFn(d BlDictionary, key BlString) BlDictionary {
	out := newDictionary()
	for _, k := range d.keys {
		if k != key.s {
			out.set(k, d.m[k])
		}
	}
	return out
}

func dictionaryOptions() []expr.Option {
	return []expr.Option{
		expr.Function("getValue", getValueFn,
			new(func(BlValue, BlValue) BlValue)),
		expr.Function("getEntries", typed1(getEntriesFn), new(func(BlValue) BlList)),
		expr.Function("dictionaryPut", dictionaryPutFn,
			new(func(BlValue, BlValue, BlValue) BlDictionary)),
		expr.Function("dictionaryMerge", typed1(dictionaryMergeFn), new(func(BlValue) BlDictionary)),
		expr.Function("keys", typed1(keysFn), new(func(BlValue) BlList)),
		expr.Function("values", typed1(valuesFn), new(func(BlValue) BlList)),
		expr.Function("has", typed2(hasFn), new(func(BlValue, BlValue) BlBoolean)),
		expr.Function("size", typed1(dictSizeFn), new(func(BlValue) BlNumber)),
		expr.Function("dictionaryRemove", typed2(dictionaryRemoveFn), new(func(BlValue, BlValue) BlDictionary)),
		// isEmpty is a unified cross-type dispatcher in overloads.go.

		// dictionary literal constructor emitted by the MapNode patcher.
		expr.Function("__mkdict", mkDictFn, new(func(...BlValue) BlDictionary)),
	}
}

// mkDictFn builds a BlDictionary from alternating key/value arguments emitted
// by the MapNode patcher.
func mkDictFn(args ...any) (any, error) {
	d := newDictionary()
	for i := 0; i+1 < len(args); i += 2 {
		k, ok := args[i].(BlString)
		if !ok {
			return nil, argTypeError(args[i])
		}
		d.set(k.s, asBl(args[i+1]))
	}
	return d, nil
}
