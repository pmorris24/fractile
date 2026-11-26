# Security Policy

## Project Status

Spaceman is an experimental public preview without a guaranteed security maintenance window.

## Reporting A Vulnerability

Do not file a public issue for a suspected vulnerability or include private window/state data in a
report. Contact the repository owner privately; if GitHub private vulnerability reporting is enabled,
use the repository's **Security > Report a vulnerability** flow.

Include the affected revision, macOS version and architecture, impact, minimal reproduction, and any
suggested mitigation. Redact window titles, bundle IDs where sensitive, local paths beyond those
needed to explain the issue, and the contents of `parked-windows.json` or `workspace-state.json`.

The maintainer will acknowledge the report, assess scope, and coordinate disclosure when practical.
No response-time guarantee applies while the project remains experimental.

## Scope Notes

Spaceman has broad Accessibility permission and uses private `_AXUIElementGetWindow`. The local
control socket and lock are mode `0600`. There is no telemetry and no network access. See
[Security and privacy](docs/security-and-privacy.md) for the current data model.
