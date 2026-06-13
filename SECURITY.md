# Security Policy

## Reporting a vulnerability

Please **do not** open a public issue for security vulnerabilities.

Instead, report them privately via GitHub's [private vulnerability reporting](https://docs.github.com/en/code-security/security-advisories/guidance-on-reporting-and-writing-information-about-vulnerabilities/privately-reporting-a-security-vulnerability)
("Report a vulnerability" under the repository's **Security** tab).

Please include:

- a description of the issue and its impact,
- steps to reproduce (a proof of concept if possible),
- affected version or commit.

You can expect an initial response within a few days. Once a fix is available,
we'll coordinate disclosure.

## Supported versions

Lanes is pre-1.0; only the latest release receives security fixes.

## Hardening notes for self-hosters

- Run behind TLS and keep `COOKIE_SECURE=true` (the default) in production.
- Keep the `/data` volume (SQLite DB + attachments) backed up and access-restricted.
- Don't expose the instance to the public internet without authentication in front of it unless you intend it to be multi-tenant.