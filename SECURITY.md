# Security Policy

Adaptive Linux is an early-stage operating system project. Do not deploy it on
production systems yet.

Report security issues privately through the GitHub repository owner until a
dedicated advisory process is published.

Security-sensitive areas:

- `optid` privileged sysfs and systemd actions.
- Kernel configuration fragments.
- Boot, UKI, signing, and rollback policy.
- Package metadata, signatures, and update descriptors.
- eBPF observability once implemented.

