## What does this PR do?

<!-- One-paragraph summary: what and why. Link issues with "Closes #NNN" or "Follow up on #NNN". -->

Closes #

## Type of change

- [ ] Bug fix
- [ ] New feature (language builtin, codegen, runtime)
- [ ] Refactor (no behavior change)
- [ ] Documentation
- [ ] Test coverage

## Checklist

<!-- Required before review. The CI workflow is the arbiter — do not claim green CI without a link to the run. -->

- [ ] Branch targets `development` (new features prototyped on `experimental/<feature>` first)
- [ ] `cargo fmt` and `cargo clippy` clean
- [ ] Added/updated tests in `tests/` — **behavioral (JIT) tests preferred over compile-only for runtime changes**
- [ ] All CI checks green on the latest commit
- [ ] `CHANGELOG.md` updated under the next version heading
- [ ] `PRD.md` status table updated (if a roadmap feature changed state)
- [ ] Docs site updated (`aha-lang-docs`) for user-facing syntax/builtin changes
- [ ] No `print(...)` on String values in examples/tests — use `print_str` (`print` only accepts i64)

## For language/runtime changes only

- [ ] Feature implemented as an **AHA builtin via codegen**, not a Rust-level library
- [ ] String-returning runtime functions allocate `len + 1` with explicit `\0` (codegen measures with `strlen` — see the v1.7.1 null-termination fix)
- [ ] Comparison/logical operators return `Int` (0/1), never `Bool` — keeps `&&`/`||` composable with arithmetic
- [ ] If/while conditions and merge phis stay type-consistent for struct-typed values (String/Struct/Enum)

## Notes for reviewers

<!-- Anything tricky, perf impact, alternative designs rejected, etc. -->
