# CLAUDE.md — Claude Agent Notes

Use `AGENTS.md` as the primary project onboarding document.

When changing Rust code, follow the Microsoft Pragmatic Rust
[Universal Guidelines](https://microsoft.github.io/rust-guidelines/guidelines/universal/index.html)
from the start of the task. In particular: keep lint exceptions narrow and
justified, avoid leaking secrets through `Debug` or logs, document meaningful
production constants, prefer structured `tracing` fields, and preserve small,
purposeful module interfaces.
