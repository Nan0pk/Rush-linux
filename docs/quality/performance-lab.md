# 🧪 The Evidence Lab

The Evidence Lab is where Rush Linux moves from theoretical architecture to proven performance. We believe in **evidence-based optimization**, meaning every policy change in `optid` must be backed by measured data.

## 🛠️ Current PoC: Host-Bench (HP Victus)

Our first major proof-of-concept (PoC) was conducted on an HP Victus laptop to validate the `optid` engine's ability to shift between extreme throughput and power-efficient responsiveness.

### 📊 The Results: Throughput vs. Efficiency

We compared the `optid` engine against standard kernel levers (`baseline`, `epp`, `weight`).

#### 🔌 AC Power (High Performance Mode)
When `optid-performance` is engaged, the system sees a massive jump in raw work capacity.

| Lever | Work Units (Median) | p95 Responsiveness | Power (Watts) |
| :--- | :--- | :--- | :--- |
| **Baseline** | 797M | 0.065ms | 16.47W |
| **`optid-performance`** | **1.51B** | **0.061ms** | 30.12W |

**Conclusion**: `optid-performance` nearly **doubles the throughput** (Work Units) while actually *improving* p95 latency. This proves that the engine can push the hardware to its limit without sacrificing foreground responsiveness.

#### 🔋 Battery Power (Efficiency Mode)
The goal here is to maximize work-per-joule without letting the system feel "sluggish."

| Lever | Work Units (Median) | p95 Responsiveness | Power (Watts) |
| :--- | :--- | :--- | :--- |
| **Baseline** | 815M | 0.070ms | 16.26W |
| **`optid-performance`** | **1.19B** | 0.069ms | 25.08W |
| **`optid-battery`** | 763M | 0.085ms | 16.42W |

**Conclusion**: Even on battery, `optid-performance` can provide a ~46% boost in work capacity. Meanwhile, `optid-battery` maintains a power profile similar to the baseline, ensuring no regression in battery life.

---

## 🔬 Methodology

To ensure these results are reproducible, we provide the full testing suite in the repository.

### The Toolchain
- **`tools/bench-optid-matrix.sh`**: The primary orchestrator that runs the matrix of levers vs. scenarios.
- **`tools/bench-work-load.py`**: A controlled synthetic workload designed to stress CPU and memory.
- **`release/evidence/`**: Raw CSV results and terminal transcripts from every test run.

### How to Reproduce
If you have a modern Linux machine and the `optid` MVP installed:
```bash
# Run the matrix benchmark on your own hardware
./tools/bench-optid-matrix.sh
```

---

## 📈 What's Next for the Lab?

- [ ] **Multi-Hardware Validation**: Testing on AMD vs. Intel vs. ARM (Apple Silicon/Raspberry Pi).
- [ ] **Real-World App Benchmarks**: Moving from synthetic "Work Units" to actual build times (Rust/C++) and video render speeds.
- [ ] **Thermal Throttling Analysis**: Measuring how `optid` handles long-term thermal saturation.
