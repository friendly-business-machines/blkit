# Spec Delta

## ADDED Requirements

### Requirement: Source-authored errors identify the offending construct
For a `.bl` source with a lexical or syntax error, the compiler SHALL report the error with a source location (including the end-of-file position for a missing closing token) and a description of the expected or invalid construct. For source-bound validation errors whose offending declaration or expression is known, the compiler SHALL report that construct's source location; at minimum, unknown named types and invalid process bindings SHALL be located. Locations SHALL refer to the original file rather than a normalized or merged intermediate representation. For validation failures that cannot reliably be tied to a single construct, the compiler SHALL retain the relevant file context and SHALL NOT invent a location. No lexical, syntax, or validation error SHALL result in emitted Rust.

#### Scenario: Malformed expression or declaration
- **WHEN** a `.bl` file contains `expression amount + ;`, an unrecognized character, or an unclosed declaration
- **THEN** compilation fails with an explanation of the error and a location at the unexpected token or end of source in the original file, without emitting Rust

#### Scenario: Source-bound validation failure
- **WHEN** a declared port refers to an unknown type or a `bind` statement names a nonexistent port
- **THEN** compilation fails with a diagnostic that identifies the unknown type or invalid binding and locates its source construct

#### Scenario: Project sources remain distinct
- **WHEN** one `.bl` project file declares a type and a second file in the same scope has a malformed expression or invalid binding
- **THEN** the reported source path and location identify the second file rather than a merged declaration or the first file

#### Scenario: Independent errors and safe recovery
- **WHEN** a file has two independent malformed statements separated by valid statement boundaries
- **THEN** compilation reports both errors without treating a recovered partial parse as valid or emitting Rust; recovery SHALL NOT manufacture a cascade of duplicate messages for the same error
