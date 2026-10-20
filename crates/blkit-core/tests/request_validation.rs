use blkit_core::compiled_graph::{RequestSchema, RequestType};
use serde_json::json;

fn schema() -> RequestSchema {
    use RequestType::{Dictionary, DictionaryEntry, List, Named};
    RequestSchema {
        ports: [
            ("order".into(), Named("Order".into())),
            ("decision".into(), Named("Decision".into())),
            (
                "entries".into(),
                List(Box::new(DictionaryEntry(Box::new(Named("Number".into()))))),
            ),
        ]
        .into(),
        records: [
            (
                "Order".into(),
                [
                    ("details".into(), Named("Detail".into())),
                    ("children".into(), List(Box::new(Named("Order".into())))),
                ]
                .into(),
            ),
            (
                "Detail".into(),
                [
                    ("amount".into(), Named("Number".into())),
                    ("due".into(), Named("DateTime".into())),
                    (
                        "tags".into(),
                        Dictionary(Box::new(List(Box::new(Named("Bool".into()))))),
                    ),
                ]
                .into(),
            ),
        ]
        .into(),
        enums: [(
            "Decision".into(),
            vec!["approved".into(), "rejected".into()],
        )]
        .into(),
    }
}

#[test]
fn persisted_request_schema_validates_recursive_records_and_rejects_extra_or_missing_fields() {
    let persisted: RequestSchema =
        serde_json::from_value(serde_json::to_value(schema()).unwrap()).unwrap();
    let valid = json!({"order":{"details":{"amount":"12.50","due":"2025-01-01T12:00:00Z","tags":{"ok":[true,false]}},"children":[{"details":{"amount":"1","due":"2025-01-02T12:00:00Z","tags":{}},"children":[]}]},"decision":"approved","entries":[{"key":"fee","value":"2"}]});
    assert!(persisted.validate_response(&valid).is_ok());
    for (where_, invalid) in [
        (
            "top-level extra",
            json!({"order":valid["order"],"decision":"approved","entries":[],"extra":0}),
        ),
        (
            "top-level missing",
            json!({"order":valid["order"],"decision":"approved"}),
        ),
        (
            "nested extra",
            json!({"order":{"details":{"amount":"12.50","due":"2025-01-01T12:00:00Z","tags":{},"extra":0},"children":[]},"decision":"approved","entries":[]}),
        ),
        (
            "nested missing",
            json!({"order":{"details":{"amount":"12.50","tags":{}},"children":[]},"decision":"approved","entries":[]}),
        ),
        (
            "recursive child",
            json!({"order":{"details":valid["order"]["details"],"children":[{"details":valid["order"]["details"],"children":[],"extra":0}]},"decision":"approved","entries":[]}),
        ),
        (
            "list element",
            json!({"order":{"details":{"amount":"1","due":"2025-01-01T12:00:00Z","tags":{"bad":[1]}},"children":[]},"decision":"approved","entries":[]}),
        ),
        (
            "entry extra",
            json!({"order":valid["order"],"decision":"approved","entries":[{"key":"fee","value":"2","extra":0}]}),
        ),
        (
            "entry missing",
            json!({"order":valid["order"],"decision":"approved","entries":[{"key":"fee"}]}),
        ),
        (
            "enum variant",
            json!({"order":valid["order"],"decision":"unknown","entries":[]}),
        ),
        (
            "decimal",
            json!({"order":{"details":{"amount":"bad","due":"2025-01-01T12:00:00Z","tags":{}},"children":[]},"decision":"approved","entries":[]}),
        ),
        (
            "datetime",
            json!({"order":{"details":{"amount":"1","due":"2025-99-01T12:00:00Z","tags":{}},"children":[]},"decision":"approved","entries":[]}),
        ),
        ("not object", json!([])),
    ] {
        assert!(
            persisted.validate_response(&invalid).is_err(),
            "accepted {where_}: {invalid}"
        );
    }
    assert!(persisted.validate_response(&valid).is_ok());
}

#[test]
fn request_schema_accepts_supported_builtin_json_encodings() {
    for (name, value) in [
        ("Bool", json!(true)),
        ("String", json!("hello")),
        ("Number", json!("1.25")),
        ("Date", json!("2025-01-01")),
        ("Time", json!("12:00:00Z")),
        ("DateTime", json!("2025-01-01T12:00:00Z")),
        ("DTDuration", json!("PT1H")),
        ("YMDuration", json!("P1Y")),
        ("Value", json!({"any": [null, 2]})),
        ("Dictionary", json!({"any": [null, 2]})),
        (
            "Calendar",
            json!({"validFrom":"2025-01-01","validTo":"2025-12-31","entries":[]}),
        ),
        (
            "CalendarEntry",
            json!({"value":"2025-01-01","name":"holiday"}),
        ),
    ] {
        let schema = RequestSchema {
            ports: [("field".into(), RequestType::Named(name.into()))].into(),
            records: Default::default(),
            enums: Default::default(),
        };
        assert!(
            schema.validate_response(&json!({"field":value})).is_ok(),
            "rejected {name}: {value}"
        );
    }
}

#[test]
fn persisted_schema_rejects_builtin_shadows_and_record_enum_name_collisions() {
    for (case, descriptor, response) in [
        (
            "Bool record shadow",
            json!({"ports":{"flag":{"Named":"Bool"}},"records":{"Bool":{}},"enums":{}}),
            json!({"flag":{}}),
        ),
        (
            "Bool enum shadow",
            json!({"ports":{"flag":{"Named":"Bool"}},"records":{},"enums":{"Bool":["approved"]}}),
            json!({"flag":"approved"}),
        ),
        (
            "unused builtin shadow",
            json!({"ports":{"flag":{"Named":"Bool"}},"records":{"Number":{}},"enums":{}}),
            json!({"flag":true}),
        ),
        (
            "record enum collision",
            json!({"ports":{"field":{"Named":"Ticket"}},"records":{"Ticket":{}},"enums":{"Ticket":["approved"]}}),
            json!({"field":{}}),
        ),
    ] {
        let persisted: RequestSchema = serde_json::from_value(descriptor).unwrap();
        assert!(
            persisted.validate_response(&response).is_err(),
            "accepted {case}: {response}"
        );
    }
}

#[test]
fn bare_dictionary_requires_object_and_unresolved_type_is_rejected() {
    let free_dictionary = RequestSchema {
        ports: [("field".into(), RequestType::Named("Dictionary".into()))].into(),
        records: Default::default(),
        enums: Default::default(),
    };
    assert!(
        free_dictionary
            .validate_response(&json!({"field": [1]}))
            .is_err()
    );
    let schema = RequestSchema {
        ports: [("field".into(), RequestType::Named("Unknown".into()))].into(),
        records: Default::default(),
        enums: Default::default(),
    };
    assert!(schema.validate_response(&json!({"field":null})).is_err());
}
