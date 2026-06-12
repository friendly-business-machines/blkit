package blkit

import "testing"

func TestSchemaConstruction(t *testing.T) {
	_, err := Schema(
		Field{Name: "name", Type: TypeString},
		Field{Name: "age", Type: TypeNumber},
	)
	if err != nil {
		t.Fatalf("valid schema rejected: %v", err)
	}
}

func TestSchemaWellFormednessErrors(t *testing.T) {
	cases := []struct {
		name   string
		fields []Field
	}{
		{"duplicate name", []Field{{Name: "a", Type: TypeNumber}, {Name: "a", Type: TypeString}}},
		{"empty name", []Field{{Name: "", Type: TypeNumber}}},
		{"null type", []Field{{Name: "a", Type: TypeNull}}},
		{"list without element", []Field{{Name: "a", Type: TypeList}}},
		{"list duplicate element", []Field{{Name: "a", Type: TypeList, Element: []Type{TypeNumber, TypeNumber}}}},
		{"scalar with nesting", []Field{{Name: "a", Type: TypeNumber, Element: []Type{TypeNumber}}}},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if _, err := Schema(c.fields...); err == nil {
				t.Errorf("expected error for %s", c.name)
			}
		})
	}
}

func TestSchemaValidateInputClosed(t *testing.T) {
	schema, _ := Schema(
		Field{Name: "name", Type: TypeString},
		Field{Name: "age", Type: TypeNumber, Optional: true},
	)
	name, _ := String("Alice")
	good, _ := Dictionary(map[string]BlValue{"name": name})
	if err := schema.ValidateInput(good); err != nil {
		t.Errorf("valid input rejected: %v", err)
	}
	// undeclared key rejected under closed semantics
	extra, _ := Dictionary(map[string]BlValue{"name": name, "extra": name})
	if err := schema.ValidateInput(extra); err == nil {
		t.Errorf("expected undeclared-key rejection")
	}
	// permissive output ignores undeclared key
	if err := schema.ValidateOutput(extra); err != nil {
		t.Errorf("output validation rejected undeclared key: %v", err)
	}
}

func TestSchemaValidateMissingRequired(t *testing.T) {
	schema, _ := Schema(Field{Name: "name", Type: TypeString})
	empty, _ := Dictionary(map[string]BlValue{})
	if err := schema.ValidateInput(empty); err == nil {
		t.Errorf("expected missing-required error")
	}
}

func TestSchemaTypeMismatch(t *testing.T) {
	schema, _ := Schema(Field{Name: "age", Type: TypeNumber})
	name, _ := String("Alice")
	bad, _ := Dictionary(map[string]BlValue{"age": name})
	err := schema.ValidateInput(bad)
	if err == nil {
		t.Fatalf("expected type-mismatch error")
	}
	if _, ok := err.(*SchemaError); !ok {
		t.Errorf("expected *SchemaError, got %T", err)
	}
}

func TestSchemaNestedDictionary(t *testing.T) {
	addr, _ := Schema(Field{Name: "city", Type: TypeString})
	schema, _ := Schema(
		Field{Name: "name", Type: TypeString},
		Field{Name: "address", Type: TypeDictionary, Fields: addr},
	)
	city, _ := String("Bristol")
	addrVal, _ := Dictionary(map[string]BlValue{"city": city})
	name, _ := String("Alice")
	good, _ := Dictionary(map[string]BlValue{"name": name, "address": addrVal})
	if err := schema.ValidateInput(good); err != nil {
		t.Errorf("nested validation failed: %v", err)
	}
}
