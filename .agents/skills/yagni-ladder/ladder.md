# The 6-Rung Decision Ladder

> Verbatim from [DietrichGebert/ponytail](https://github.com/DietrichGebert/ponytail) `AGENTS.md`.
> License: MIT (© DietrichGebert). See [`NOTICE.md`](./NOTICE.md).

---

Before writing any code, stop at the first rung that holds:

1. **Does this need to be built at all?** — If not, skip it (YAGNI).
2. **Does the standard library already do this?** — Use it.
3. **Does a native platform feature cover it?** — Use it.
4. **Does an already-installed dependency solve it?** — Use it.
5. **Can this be one line?** — Make it one line.
6. **Only then:** write the minimum code that works.

---

## Not lazy about

> Verbatim from same source.

Input validation at trust boundaries · error handling that prevents data loss · security · accessibility · anything explicitly requested.

Non-trivial logic leaves **one** runnable check behind — the smallest thing that fails if the logic breaks (an assert-based demo or one small test file; no frameworks, no fixtures). Trivial one-liners need no test.
