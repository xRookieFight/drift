# Security Policy

## Supported versions

Drift is pre 1.0. Only the latest release and the main branch get security fixes.

## Reporting a vulnerability

Please do not open a public issue for security problems.

Report privately through GitHub: open the repository's **Security** tab and click **Report a vulnerability**. Include what you found, how to reproduce it, and what an attacker could do with it.

You can expect an acknowledgement within a week. Once a fix is ready, a release goes out and the advisory is published with credit to you, unless you prefer otherwise.

## Scope

In scope: the Drift shell, its `drift://` pages and the bridge between the UI and the core, session and profile storage, the self updater, and the release artifacts.

Bugs in the Servo engine itself (rendering, JavaScript, networking) should be reported to the Servo project as described in [their security policy](https://github.com/servo/servo/security/policy). If you are unsure where a bug lives, report it here and we will forward it.
