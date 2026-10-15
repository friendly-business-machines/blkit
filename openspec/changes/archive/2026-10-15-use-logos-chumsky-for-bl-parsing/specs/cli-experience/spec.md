# Spec Delta

## MODIFIED Requirements

### Requirement: Diagnostics are contextual and automation-safe
Operational errors SHALL identify the failing source, project, or underlying operation where available and exit nonzero. CLI-generated failures SHALL go to stderr; completion scripts SHALL go to stdout. For `.bl` lexical and syntax errors, and source-bound validation errors with a known location, direct and project transpilation SHALL display the correct file path, source excerpt, location, and a readable explanation. If no reliable location is known, the CLI SHALL show the path and explanation without a fabricated span. Terminal styling and transient progress SHALL appear only when stderr is an interactive terminal and styling is enabled; non-interactive or color-disabled output SHALL contain no ANSI control sequences or progress animation. The CLI SHALL preserve accessible plain-text diagnostic content without requiring terminal color or graphics.

#### Scenario: Invalid source
- **WHEN** direct transpilation fails parsing or validation
- **THEN** the CLI reports the source path and underlying error on stderr, includes a source excerpt and location when known, returns nonzero, and does not write the output file

#### Scenario: Project build failure
- **WHEN** project transpilation fails during manifest validation, source validation, or Cargo dependency resolution
- **THEN** the CLI reports the operation and original failure details on stderr, returns nonzero, and does not display a success message; a located `.bl` error uses the offending project's source file rather than just its project root

#### Scenario: Redirected output
- **WHEN** stderr is captured by a script or styling is disabled
- **THEN** errors contain no ANSI escape sequences and no transient progress frames; any available file path, location, and explanatory text remain readable

#### Scenario: Error without a source location
- **WHEN** compilation reports a global validation failure without a reliable source span
- **THEN** the CLI reports the error with its source or project context but does not underline an unrelated source token
