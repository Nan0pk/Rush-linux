# Rush Linux Contract Validation Report
Generated at: 2026-06-14T15:49:40Z

## Summary Table
| Class | Workload | Metric | N | Median | P95 | IQR | Avg Power (W) | Status / Violations |
|---|---|---|---|---|---|---|---|---|
| idle | cyclictest | cyclictest-max-us | 5 | 185 | 3601.6 | 3059 | 0.00 W | OK |
| idle | foreground-launch | foreground-launch-ms | 5 | N/A | N/A | N/A | N/A | unsupported_here |
| idle | psi-cpu | psi-cpu-avg10 | 5 | 0 | 0 | 0 | 0.00 W | OK |
| idle | psi-io | psi-io-avg10 | 5 | 20 | 20 | 0 | 0.00 W | OK |
| interactive | cyclictest | cyclictest-max-us | 5 | 2875 | 3449.8 | 751 | 0.00 W | OK |
| interactive | foreground-launch | foreground-launch-ms | 5 | N/A | N/A | N/A | N/A | unsupported_here |
| interactive | psi-cpu | psi-cpu-avg10 | 5 | 0 | 0 | 0 | 0.00 W | OK |
| interactive | psi-io | psi-io-avg10 | 5 | 0 | 0 | 0 | 0.00 W | OK |
| latency-critical | cyclictest | cyclictest-max-us | 5 | 2483 | 3486.6 | 1300 | 0.00 W | budget_violation |
| latency-critical | foreground-launch | foreground-launch-ms | 5 | N/A | N/A | N/A | N/A | unsupported_here |
| latency-critical | psi-cpu | psi-cpu-avg10 | 5 | 0 | 0 | 0 | 0.00 W | OK |
| latency-critical | psi-io | psi-io-avg10 | 5 | 0 | 0 | 0 | 0.00 W | OK |
| light | cyclictest | cyclictest-max-us | 5 | 412 | 3477.8 | 3135 | 0.00 W | OK |
| light | foreground-launch | foreground-launch-ms | 5 | N/A | N/A | N/A | N/A | unsupported_here |
| light | psi-cpu | psi-cpu-avg10 | 5 | 0 | 0 | 0 | 0.00 W | OK |
| light | psi-io | psi-io-avg10 | 5 | 0 | 0 | 0 | 0.00 W | OK |
| throughput | cyclictest | cyclictest-max-us | 5 | 75 | 2566.2 | 2358 | 0.00 W | OK |
| throughput | foreground-launch | foreground-launch-ms | 5 | N/A | N/A | N/A | N/A | unsupported_here |
| throughput | psi-cpu | psi-cpu-avg10 | 5 | 0 | 0 | 0 | 0.00 W | OK |
| throughput | psi-io | psi-io-avg10 | 5 | 0 | 0 | 0 | 0.00 W | OK |

## Energy Analysis
- Idle average power draw: 0.00 W
- Interactive average power draw: 0.00 W
- **Warning:** idle power draw (0.00 W) is NOT less than interactive power draw (0.00 W)!

## Contract Verification
- Latency-critical cyclictest median: 2483 us (Contract Floor: 10 us)
  - **BUDGET VIOLATION DETECTED**: Observed latency (2483 us) exceeds contract limit (10 us)!
