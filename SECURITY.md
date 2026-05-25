# Security Policy

Adaptive Linux is an early-stage operating system project. Do not deploy it on
production systems yet.

Report security issues privately through the GitHub repository owner until a
dedicated advisory process is published.

## Supported Versions

No production release is supported yet. The repository is a development
scaffold.

## Reporting

Until GitHub Security Advisories are enabled for the repository, report issues
privately to the repository owner. Do not open public issues for vulnerabilities
that allow privilege escalation, unsafe sysfs writes, update compromise, or
signature bypass.

Security-sensitive areas:

- `optid` privileged sysfs and systemd actions.
- Kernel configuration fragments.
- Boot, UKI, signing, and rollback policy.
- Package metadata, signatures, and update descriptors.
- eBPF observability once implemented.

## Security Requirements

- Privileged actions must be allowlisted and explainable.
- `optid` must preserve a dry-run mode.
- Update and package metadata must be signed before any installable release.
- Bad kernels and failed updates must be rollbackable.
- eBPF probes must have explicit overhead and safety limits.
- Documentation must be updated when security-sensitive behavior changes.
