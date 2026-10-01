# Public API questions

Use the [Rust API Guidelines checklist][checklist] for **changed public
interfaces**, selecting only applicable items:

- [C-GOOD-ERR][good-err], [C-VALIDATE][validate], [C-FAILURE][failure]:
  Can callers handle invalid inputs and errors without an unexpected panic?
  Are relevant failure modes documented?
- [C-CONV][conv] and [C-CONV-TRAITS][conv-traits]: Are conversions and method
  names predictable?
- [C-NEWTYPE][newtype] and [C-CUSTOM-TYPE][custom-type]: Would a type prevent
  an actual caller mistake? Do not request a wrapper without a concrete
  invariant.
- [C-DEBUG][debug]: Can public types be diagnosed without exposing secrets?

The checklist is guidance, not a mandate to implement every trait, builder,
or generic signature. Check the linked rationale and the surrounding API
before filing a finding.

[checklist]: https://rust-lang.github.io/api-guidelines/checklist.html
[good-err]: https://rust-lang.github.io/api-guidelines/interoperability.html#c-good-err
[validate]: https://rust-lang.github.io/api-guidelines/dependability.html#c-validate
[failure]: https://rust-lang.github.io/api-guidelines/documentation.html#c-failure
[conv]: https://rust-lang.github.io/api-guidelines/naming.html#c-conv
[conv-traits]: https://rust-lang.github.io/api-guidelines/interoperability.html#c-conv-traits
[newtype]: https://rust-lang.github.io/api-guidelines/type-safety.html#c-newtype
[custom-type]: https://rust-lang.github.io/api-guidelines/type-safety.html#c-custom-type
[debug]: https://rust-lang.github.io/api-guidelines/debuggability.html#c-debug
