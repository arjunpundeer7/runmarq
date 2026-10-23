# Security policy

## Reporting a vulnerability

Please **do not open a public issue** for security problems.

Report privately through GitHub: **Security → Report a vulnerability** on this repository ([direct link](https://github.com/arjunpundeer7/runmarq/security/advisories/new)). Include the affected component, steps to reproduce and, if possible, the output of `runmarq doctor --json` with anything sensitive removed.

You can expect an acknowledgement within a few days. Fixes are released as soon as practical, and reporters are credited unless they ask not to be.

## Scope

Of particular interest:

- Anything that lets prompt text, code, commands or tool output reach an event, log or the database
- Bypassing the collector's token, `Origin` or `Host` checks
- Hook behaviour that can block, alter or inject into an agent session
- Unsafe changes to agent configuration files
- Path, symlink or file-read issues in the spool, probes or transcript reader

The current controls and known gaps are documented in [docs/privacy/threat-model.md](docs/privacy/threat-model.md).

## Supported versions

runmarq is pre-release. Only the latest `main` receives fixes.
