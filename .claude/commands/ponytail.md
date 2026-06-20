Apply the minimal-code decision ladder to the current task. Before writing any code, stop at the first rung that holds:

1. Does this need to be built at all? (YAGNI — skip it)
2. Does the Rust stdlib / std already do this? Use it.
3. Does a native platform feature cover it? Use it.
4. Does an already-imported crate in Cargo.toml solve it? Use it.
5. Can this be one line or a trivial combinator chain? Make it so.
6. Only then: write the minimum code that works.

Never cut:
- Input validation at trust boundaries
- Error handling that prevents data loss or undefined behaviour
- Safety invariants optid's contract layer depends on
- Anything explicitly requested

No unrequested abstractions, traits, or generics. No new dependency if avoidable. No boilerplate nobody asked for. Deletion over addition. Boring over clever.

Non-trivial logic leaves ONE runnable test — the smallest thing that fails if the logic breaks.

After applying the ladder, continue with the normal Rush session lifecycle (start-work → work → finish-work).
