# Security Policy

Rush Linux is an early-stage operating system project. Do not deploy it on
production systems yet.

Report security issues privately. The current contact of record is the project
maintainer, **GitHub [@Nan0pk](https://github.com/Nan0pk)**, until a dedicated
security team is formed.

## Supported Versions

No production release is supported yet. The repository is a development
scaffold.

## Reporting

Preferred channel: use GitHub's private vulnerability reporting on this
repository — **Security → Advisories → "Report a vulnerability"**
(`https://github.com/Nan0pk/Rush-linux/security/advisories/new`). This opens a
private advisory visible only to you and the maintainer; no public issue is
created.

If private advisories are not yet enabled, contact the maintainer of record
([@Nan0pk](https://github.com/Nan0pk)) directly and ask them to enable it rather
than disclosing details in a public issue.

Do not open public issues for vulnerabilities that allow privilege escalation,
unsafe sysfs writes, update compromise, or signature bypass.

You can expect an initial acknowledgement within a best-effort window while the
project is pre-1.0; response-time commitments will be formalised alongside the
governance plan in `docs/project-sustainability.md` (item C1).

Security-sensitive areas:

- `optid` privileged sysfs and systemd actions.
- Kernel configuration fragments.
- Boot, UKI, signing, and rollback policy.
- Package metadata, signatures, and update descriptors.
- eBPF observability once implemented.

## Security Requirements

- Privileged actions must be allowlisted and explainable.
- `optid` must preserve a dry-run mode.
- The default packaged service must not pass `--apply`; mutating mode must stay
  explicit until the safety model is proven.
- Update and package metadata must be signed before any installable release.
- Bad kernels and failed updates must be rollbackable.
- eBPF probes must have explicit overhead and safety limits.
- Documentation must be updated when security-sensitive behavior changes.
