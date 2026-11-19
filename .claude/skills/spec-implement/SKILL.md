---
name: spec-implement
description: Use when the user asks to implement a spec.
---

All spec files follow RFC language. Read `TESTING.md` before writing tests.

1. Read the spec file diff (`git diff staging -- '*SPEC.md'` if the user doesn't say which).
2. Review the relevant parts of the codebase.
3. Plan the implementation BASED ON the diff of the spec.
4. Write tests that check the code matches the spec, before the code. Each test MUST cite
   the spec ID it proves (in its title in TypeScript, in a comment above it elsewhere). You
   SHOULD use the same language in the tests as the spec. Test through the component's
   public interface, as `TESTING.md` describes.
5. Implement until the tests pass. `just tests-for <SPEC-ID>` prints the command that runs
   only the tests citing that section.
6. Run `just check`, which runs `spec-check` and the CI checks for every area you touched. A
   new spec section with a test must come off `scripts/spec-coverage-allow.txt`; spec-check
   says so if it doesn't.

As you implement, you MAY modify the spec file, but you MUST keep modifications as minimal as possible. Do NOT put extra implementation details in the spec. A new section gets the next unused ID for its file.
