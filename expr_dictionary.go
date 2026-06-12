package blkit

import "strings"

// BlDictionary is an ordered map of named entries to BlValues.
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
	parts := make([]string, len(d.keys))
	for i, k := range d.keys {
		parts[i] = k + ": " + literalString(d.m[k])
	}
	return "{" + strings.Join(parts, ", ") + "}"
}

func (BlDictionary) IsNull() bool { return false }

func (BlDictionary) isBlValue() {}

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

// Dictionary constructs a BlDictionary from a map of BlValues. Key order is
// sorted for determinism since Go map iteration is unordered.
func Dictionary(entries map[string]BlValue) (BlDictionary, error) {
	d := newDictionary()
	keys := make([]string, 0, len(entries))
	for k := range entries {
		keys = append(keys, k)
	}
	// stable, deterministic ordering
	sortStrings(keys)
	for _, k := range keys {
		d.set(k, entries[k])
	}
	return d, nil
}

func sortStrings(xs []string) {
	for i := 1; i < len(xs); i++ {
		for j := i; j > 0 && xs[j-1] > xs[j]; j-- {
			xs[j-1], xs[j] = xs[j], xs[j-1]
		}
	}
}
