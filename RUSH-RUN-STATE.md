# Rush Linux hardware run — state log (append-only)
# step | status | result | artifact
S0.1 clone+toolchain        | DONE | Branch claude/sleepy-goldberg-ga3z6h checked out, rustc 1.96.0 |
S0.2 surface detect         | DONE | virt=none, 24 cpus, EPP=y, platform_profile=n, RAPL=y, BAT1=y, PSI=y, PM_QoS=y |
S1.1 build+test             | DONE | workspace built release, 39 tests passed, optid+optctl verified |
S1.2 dry-run bench          | DONE | ran v2 bench dry-run, baseline RESP p95=0.071ms p99=0.093ms, POWER=30.13W, restore VERIFIED |
S1.3 applied bench (v2)     | DONE | perf/batt modes run, baseline p95=0.070ms/30.18W, perf p95=0.059ms/30.14W, batt p95=0.075ms/30.12W, restore VERIFIED | benchmarks/host-v2-fedora-20260619T220644Z.log |
S1.4 matrix + rushbench     | TODO |
S2.1 build disk.raw         | TODO |
S2.2 qemu uefi validate     | TODO |
S2.3 rollback + signing     | TODO |
S3.x bare-metal (GATED)     | TODO |
COMMIT results + draft PR   | TODO |
