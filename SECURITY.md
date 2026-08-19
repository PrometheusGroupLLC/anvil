<!--
  MAINTAINER NOTE — reporting channels.

  This policy deliberately lists GitHub's private vulnerability reporting as the
  ONLY channel. An email address was considered and intentionally left out:
  info@goprometheusgroup.com is intended as a SECONDARY channel and should be
  added here once — and only once — delivery from OUTSIDE the organization has
  been confirmed end to end (send a test message from an unrelated external
  account and verify it arrives). Google Workspace groups default to a Private
  domain-sharing setting, and a group created through the Cloud Identity API
  inherits it, so external mail may be silently rejected. A disclosure address
  that bounces without telling the reporter is worse than publishing no address
  at all.

  ALSO REQUIRED, AND NOT A FILE: private vulnerability reporting must be turned
  ON in the repository settings after the repository is created
  (Settings -> Code security and analysis -> Private vulnerability reporting).
  Until that switch is enabled, the Security tab below has no "Report a
  vulnerability" button and this document points at something that does not
  exist.
-->

# Security Policy

## Reporting a vulnerability

**Please report security vulnerabilities privately through GitHub.**

Go to the **[Security tab](../../security)** of this repository and click
**"Report a vulnerability"**. That opens a private advisory visible only to you
and the maintainers.

This route needs no email, no account beyond the GitHub one you already have,
and no trust in our mail routing. The report lands in the same place we look.

**Do not open a public issue, pull request, or discussion for a security
vulnerability.** Those are visible to everyone the moment they are filed —
including before a fix exists. If you have already filed one publicly, please
delete it if you can and open a private report instead.

If you are unsure whether something counts as a security issue, treat it as one
and report it privately. We would rather triage a non-issue in private than
have a real one disclosed in public.

If private reporting is unavailable to you — for example the button is missing,
which would be a misconfiguration on our side — please open a public issue
containing **no technical detail whatsoever**, saying only that you have a
security report and asking us to open a private channel. Do not describe the
vulnerability in that issue.

### What to include

The more of this you can provide, the faster we can act:

- What the issue is and what an attacker could achieve with it.
- The affected component (`anvil-core`, `anvil-engine`, `anvil-mcp`, or another
  crate) and the commit you observed it on.
- Reproduction steps, a proof-of-concept, or a failing `.feature` scenario.
- Any suggested remediation, if you have one.

### What to expect, honestly

Anvil is maintained by a very small team. These are commitments we can
actually keep, not aspirational ones:

| Stage | Target |
| --- | --- |
| Acknowledgement that your report was received and read | within **5 business days** |
| Initial assessment — whether we consider it a vulnerability, and its severity | within **15 business days** |
| Fix, mitigation, or a written explanation of why we will not act | communicated as soon as we have one; **we do not commit to a fixed remediation deadline** |

If you have not heard from us within 5 business days, please comment on your
private advisory — it means the report was missed, not ignored.

We do not currently operate a paid bug-bounty programme and cannot offer
financial rewards. We are glad to credit you by name in the advisory and the
release notes for the fix if you would like that; tell us how you wish to be
credited, or tell us you would prefer to remain anonymous.

## Disclosure

We ask for **90 days** from your report before public disclosure, or until a
fix ships — whichever comes first. If we are unresponsive, or you believe users
are at active risk, you are not obliged to wait; we would appreciate a heads-up
that you intend to disclose.

## Supported versions

Anvil is pre-1.0 and has no long-term-support branches. Security fixes are
applied to the current `main` of this repository and appear in the next export
(see [CONTRIBUTING.md](CONTRIBUTING.md) for how the mirror works). There is no
backporting to older tags.

## Scope

In scope: the Rust crates in this repository — the engine, the MCP shim, the
core library, and the hearth reader — including anything that lets untrusted
input reach the filesystem, escape a permitted root, or execute a command.

Out of scope: vulnerabilities in third-party dependencies (report those
upstream, though we appreciate being told), issues that require an attacker to
already have full local access to the machine running the engine, and the
absence of hardening measures that are not themselves exploitable.
