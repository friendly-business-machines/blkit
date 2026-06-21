package core

import "fmt"

// BlSchema is a declarative shape for any Bl* value: an ordered set of named,
// typed fields. Nesting hangs off each field's Type.
type BlSchema []Field

// Field is a named, typed entry in a schema. Which nested fields apply depends
// on Type: scalar leaves carry no nesting; TypeDictionary / TypeTable carry
// Fields; TypeList carries Element.
type Field struct {
	Name     string
	Type     Type
	Optional bool

	Fields  BlSchema // for TypeDictionary / TypeTable
	Element []Type   // for TypeList
}

// Schema builds a BlSchema from the supplied fields and validates
// well-formedness. On error the returned BlSchema is nil.
func Schema(fields ...Field) (BlSchema, error) {
	s := BlSchema(fields)
	if err := s.validateWellFormed(""); err != nil {
		return nil, err
	}
	return s, nil
}

func (s BlSchema) validateWellFormed(path string) error {
	seen := map[string]bool{}
	for _, f := range s {
		fp := joinPath(path, f.Name)
		if f.Name == "" {
			return &SchemaError{Path: path, Err: fmt.Errorf("empty field name")}
		}
		if seen[f.Name] {
			return &SchemaError{Path: path, Err: fmt.Errorf("duplicate field name %q", f.Name)}
		}
		seen[f.Name] = true
		if !knownType(f.Type) {
			return &SchemaError{Path: fp, Err: fmt.Errorf("unknown type %d", int(f.Type))}
		}
		if f.Type == TypeNull {
			return &SchemaError{Path: fp, Err: fmt.Errorf("TypeNull is not a declarable shape")}
		}
		switch f.Type {
		case TypeList:
			if len(f.Element) == 0 {
				return &SchemaError{Path: fp, Err: fmt.Errorf("list field requires Element types")}
			}
			elemSeen := map[Type]bool{}
			for _, et := range f.Element {
				if !knownType(et) || et == TypeNull {
					return &SchemaError{Path: fp, Err: fmt.Errorf("invalid Element type")}
				}
				if elemSeen[et] {
					return &SchemaError{Path: fp, Err: fmt.Errorf("duplicate Element type")}
				}
				elemSeen[et] = true
			}
			if len(f.Fields) != 0 {
				return &SchemaError{Path: fp, Err: fmt.Errorf("list field must not declare Fields")}
			}
		case TypeDictionary, TypeTable:
			if len(f.Element) != 0 {
				return &SchemaError{Path: fp, Err: fmt.Errorf("record field must not declare Element")}
			}
			if err := f.Fields.validateWellFormed(fp); err != nil {
				return err
			}
		default: // scalar
			if len(f.Fields) != 0 || len(f.Element) != 0 {
				return &SchemaError{Path: fp, Err: fmt.Errorf("scalar field must not declare nesting")}
			}
		}
	}
	return nil
}

func knownType(t Type) bool {
	return t >= TypeNull && t <= TypeAny
}

func joinPath(parent, child string) string {
	if parent == "" {
		return child
	}
	return parent + "." + child
}

// ValidateInput rejects values whose shape doesn't match the schema and rejects
// undeclared keys / columns (closed).
func (s BlSchema) ValidateInput(v BlValue) error {
	return s.validate(v, "", true)
}

// ValidateOutput rejects values whose shape doesn't match the schema's declared
// fields but accepts undeclared keys / columns (permissive).
func (s BlSchema) ValidateOutput(v BlValue) error {
	return s.validate(v, "", false)
}

func (s BlSchema) validate(v BlValue, path string, closed bool) error {
	d, ok := v.(BlDictionary)
	if !ok {
		return &SchemaError{Path: path, Err: fmt.Errorf("expected dictionary, got %s", typeName(v.Type()))}
	}
	declared := map[string]bool{}
	for _, f := range s {
		declared[f.Name] = true
		fp := joinPath(path, f.Name)
		fv, present := d.get(f.Name)
		if !present || fv.IsNull() {
			if f.Optional {
				continue
			}
			return &SchemaError{Path: fp, Err: fmt.Errorf("required field missing")}
		}
		if err := validateField(f, fv, fp, closed); err != nil {
			return err
		}
	}
	if closed {
		for _, k := range d.keys {
			if !declared[k] {
				return &SchemaError{Path: joinPath(path, k), Err: fmt.Errorf("undeclared key")}
			}
		}
	}
	return nil
}

func validateField(f Field, v BlValue, path string, closed bool) error {
	switch f.Type {
	case TypeAny:
		if v.IsNull() {
			return &SchemaError{Path: path, Err: fmt.Errorf("required field is null")}
		}
		return nil
	case TypeList:
		lst, ok := v.(BlList)
		if !ok {
			return &SchemaError{Path: path, Err: fmt.Errorf("expected list, got %s", typeName(v.Type()))}
		}
		allowed := map[Type]bool{}
		for _, et := range f.Element {
			allowed[et] = true
		}
		for i, e := range lst.items {
			if !allowed[e.Type()] {
				return &SchemaError{Path: fmt.Sprintf("%s[%d]", path, i+1), Err: fmt.Errorf("element type %s not allowed", typeName(e.Type()))}
			}
		}
		return nil
	case TypeDictionary:
		return f.Fields.validate(v, path, closed)
	case TypeTable:
		tbl, ok := v.(BlTable)
		if !ok {
			return &SchemaError{Path: path, Err: fmt.Errorf("expected table, got %s", typeName(v.Type()))}
		}
		for i, row := range tbl.rows {
			if err := f.Fields.validate(row, fmt.Sprintf("%s[%d]", path, i+1), closed); err != nil {
				return err
			}
		}
		return nil
	default: // scalar
		if v.Type() != f.Type {
			return &SchemaError{Path: path, Err: fmt.Errorf("expected %s, got %s", typeName(f.Type), typeName(v.Type()))}
		}
		return nil
	}
}

// BlSchema is no longer part of the expression compile path — concrete Go env
// structs (see BlExpr) now declare an expression's variables and their types at
// Go-compile time. BlSchema remains a standalone runtime value-validation
// utility (ValidateInput / ValidateOutput) for data contracts at process
// boundaries.
