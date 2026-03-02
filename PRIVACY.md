# Privacy Policy

**Last updated:** March 2, 2026

Golem is an open-source CLI tool that connects to third-party AI providers (Anthropic, Google, etc.) on your behalf. This policy explains what data Golem handles and how.

## What Golem collects

**Nothing.** Golem does not collect, transmit, or store any data on external servers operated by the Golem project. There is no telemetry, analytics, or crash reporting.

## Data stored locally

Golem stores the following on your machine in a SQLite database (typically `~/.local/share/golem/golem.db`):

- **OAuth tokens** — access and refresh tokens for providers you log into. These are stored locally and never sent anywhere except back to the issuing provider.
- **Session memory** — task history and conversation context from your REPL sessions.
- **Configuration** — your model preferences and settings.

You can delete all local data by removing the database file.

## Data sent to third-party providers

When you use Golem, your prompts and conversation context are sent directly to the AI provider you selected (e.g., Anthropic, Google). These requests go straight from your machine to the provider's API — Golem does not proxy, intercept, or log them.

Each provider has its own privacy policy:

- [Anthropic Privacy Policy](https://www.anthropic.com/privacy)
- [Google AI Privacy Policy](https://ai.google.dev/terms)

**You are responsible for reviewing and accepting each provider's terms before using them through Golem.**

## OAuth authentication

Golem uses the OAuth 2.0 authorization code flow with PKCE to authenticate with providers. During login:

1. Your browser opens the provider's consent page — Golem never sees your password.
2. You approve access and paste an authorization code back into Golem.
3. Golem exchanges the code for tokens and stores them locally.

No credentials pass through any Golem-operated server.

## Open source

Golem's source code is publicly available at [github.com/assapir/golem](https://github.com/assapir/golem). You can audit exactly what the software does.

## Children's privacy

Golem is not directed at children under 13 and does not knowingly collect data from children.

## Changes to this policy

Updates will be posted to this file in the repository. The "last updated" date at the top will reflect the most recent revision.

## Contact

For questions about this policy, open an issue at [github.com/assapir/golem/issues](https://github.com/assapir/golem/issues) or email [assaf@sapir.io](mailto:assaf@sapir.io).
