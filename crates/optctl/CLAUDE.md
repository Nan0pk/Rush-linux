# optctl crate

CLI for tracing and overriding optid policy. Loaded automatically when you read files here.

## Responsibilities
- Human-readable interface to optid's D-Bus API
- Trace policy decisions in real time
- Override actuator targets for debugging

## Conventions
- All output goes to stdout; errors to stderr with a non-zero exit
- Use `clap` for argument parsing; keep subcommand structure flat
- No business logic here — delegate to optid via D-Bus

## Test scope
`cargo test -p optctl`
