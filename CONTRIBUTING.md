# Contributing

Spaceman is an experimental public preview. Coordinate substantial work with the repository owner
before investing in it; contribution expectations may change as the preview matures.

## Before Changing Code

Read [AGENTS.md](AGENTS.md), [Development](docs/development.md),
[Security and privacy](docs/security-and-privacy.md), and [Recovery](docs/recovery.md). Treat
[PLAN.md](PLAN.md) and [RESEARCH.md](docs/RESEARCH.md) as history, not current specifications.

Keep changes small and preserve these priorities:

- never lose parked-window recovery metadata;
- never let incomplete observations prove that a logical window disappeared;
- keep native handles out of persisted/logical state and on their owning thread;
- preserve title privacy in serialized and debug output;
- fail conservatively when physical-window replacement is ambiguous;
- disable other window managers and use disposable windows for live tests.

## Quality

Run the commands in [Development](docs/development.md). Add focused tests for behavior changes. Update
README and linked docs whenever controls, configuration, compatibility, paths, privacy, packaging, or
recovery behavior changes.

Do not weaken safety behavior to make a live test pass. Do not add telemetry, network access,
automatic installation, new private APIs, or persisted sensitive
data without explicit design and security review.

## Changes And Review

Explain user-visible behavior, safety impact, tests run, and any live testing performed. Never attach
unsanitized state or output containing private user data. Do not claim compatibility or release
readiness without clean-machine evidence.
