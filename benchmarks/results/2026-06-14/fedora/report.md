# Rush Linux Contract Validation Report
Generated at: 2026-06-14T19:51:20Z

## Summary Table
| Class | Workload | Metric | N | Median | P95 | IQR | Avg Power (W) | Status / Violations |
|---|---|---|---|---|---|---|---|---|
| idle | cyclictest | cyclictest-max-us | 5 | N/A | N/A | N/A | N/A | class_mismatch |
| idle | foreground-launch | foreground-launch-ms | 5 | N/A | N/A | N/A | N/A | unsupported_here |
| idle | psi-cpu | psi-cpu-avg10 | 5 | 0 | 0 | 0 | 28.80 W | OK |
| idle | psi-io | psi-io-avg10 | 5 | 0 | 0 | 0 | 31.20 W | OK |
| interactive | cyclictest | cyclictest-max-us | 5 | N/A | N/A | N/A | N/A | class_mismatch |
| interactive | foreground-launch | foreground-launch-ms | 5 | N/A | N/A | N/A | N/A | unsupported_here |
| interactive | psi-cpu | psi-cpu-avg10 | 5 | 0 | 0 | 0 | 34.80 W | OK |
| interactive | psi-io | psi-io-avg10 | 5 | 0 | 0 | 0 | 33.60 W | OK |
| latency-critical | cyclictest | cyclictest-max-us | 5 | N/A | N/A | N/A | N/A | class_mismatch |
| latency-critical | foreground-launch | foreground-launch-ms | 5 | N/A | N/A | N/A | N/A | unsupported_here |
| latency-critical | psi-cpu | psi-cpu-avg10 | 5 | 0 | 0 | 0 | 38.40 W | OK |
| latency-critical | psi-io | psi-io-avg10 | 5 | 0 | 0 | 0 | 32.40 W | OK |
| light | cyclictest | cyclictest-max-us | 5 | N/A | N/A | N/A | N/A | class_mismatch |
| light | foreground-launch | foreground-launch-ms | 5 | N/A | N/A | N/A | N/A | unsupported_here |
| light | psi-cpu | psi-cpu-avg10 | 5 | 0 | 0 | 0 | 32.40 W | OK |
| light | psi-io | psi-io-avg10 | 5 | 0 | 0 | 0 | 38.40 W | OK |
| throughput | cyclictest | cyclictest-max-us | 5 | N/A | N/A | N/A | N/A | class_mismatch |
| throughput | foreground-launch | foreground-launch-ms | 5 | N/A | N/A | N/A | N/A | unsupported_here |
| throughput | psi-cpu | psi-cpu-avg10 | 5 | 0 | 0 | 0 | 32.40 W | OK |
| throughput | psi-io | psi-io-avg10 | 5 | 0 | 0 | 0 | 38.40 W | OK |

## Energy Analysis
- Note: insufficient battery run data to compare idle vs interactive power draw.

## Contract Verification
- Note: no latency-critical cyclictest results found in the dataset.
