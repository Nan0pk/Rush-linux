use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PairOrder {
    Ab,
    Ba,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PairPlan {
    pub schema_version: u32,
    pub seed: u64,
    pub pairs: usize,
    pub order: Vec<PairOrder>,
}

/// Build a deterministic, balanced sequence of AB and BA matched pairs.
///
/// The seed is retained in the plan so the exact ordering can be reproduced.
/// For an odd number of pairs, one order necessarily occurs once more than the
/// other; the seed decides which order gets that extra pair so repeated odd
/// campaigns do not systematically privilege AB.
pub fn build_pair_plan(pairs: usize, seed: u64) -> Result<PairPlan, String> {
    if pairs == 0 {
        return Err("pair count must be at least 1".to_string());
    }

    let upper = pairs.div_ceil(2);
    let lower = pairs / 2;
    let mut rng = XorShift64::new(seed);
    let (ab_count, ba_count) = if pairs % 2 == 0 || rng.next() & 1 == 0 {
        (upper, lower)
    } else {
        (lower, upper)
    };

    let mut order = Vec::with_capacity(pairs);
    order.extend(std::iter::repeat(PairOrder::Ab).take(ab_count));
    order.extend(std::iter::repeat(PairOrder::Ba).take(ba_count));

    for index in (1..order.len()).rev() {
        let swap_with = (rng.next() % ((index + 1) as u64)) as usize;
        order.swap(index, swap_with);
    }

    Ok(PairPlan {
        schema_version: 1,
        seed,
        pairs,
        order,
    })
}

/// Small deterministic generator used only to randomize experiment ordering.
/// It is not used for security, identifiers, or hardware authorization.
struct XorShift64(u64);

impl XorShift64 {
    fn new(seed: u64) -> Self {
        // XorShift has an all-zero absorbing state. Map seed 0 to a fixed
        // non-zero state while still recording the user's original seed.
        Self(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }

    fn next(&mut self) -> u64 {
        let mut value = self.0;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.0 = value;
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(plan: &PairPlan) -> (usize, usize) {
        let ab = plan
            .order
            .iter()
            .filter(|order| **order == PairOrder::Ab)
            .count();
        let ba = plan.order.len() - ab;
        (ab, ba)
    }

    #[test]
    fn pair_plan_is_balanced_for_even_and_odd_counts() {
        for pairs in 1..=20 {
            let plan = build_pair_plan(pairs, 1234).unwrap();
            let (ab, ba) = counts(&plan);
            assert_eq!(plan.order.len(), pairs);
            assert!(ab.abs_diff(ba) <= 1);
        }
    }

    #[test]
    fn odd_pair_extra_is_not_systematically_assigned_to_ab() {
        let mut saw_ab_extra = false;
        let mut saw_ba_extra = false;
        for seed in 0..64 {
            let plan = build_pair_plan(5, seed).unwrap();
            let (ab, ba) = counts(&plan);
            saw_ab_extra |= ab > ba;
            saw_ba_extra |= ba > ab;
        }
        assert!(saw_ab_extra && saw_ba_extra);
    }

    #[test]
    fn same_seed_reproduces_exact_order() {
        assert_eq!(
            build_pair_plan(20, 42).unwrap(),
            build_pair_plan(20, 42).unwrap()
        );
    }

    #[test]
    fn zero_seed_is_valid_and_reproducible() {
        assert_eq!(
            build_pair_plan(9, 0).unwrap(),
            build_pair_plan(9, 0).unwrap()
        );
    }

    #[test]
    fn zero_pairs_is_rejected() {
        assert_eq!(
            build_pair_plan(0, 1).unwrap_err(),
            "pair count must be at least 1"
        );
    }

    #[test]
    fn serialized_plan_retains_seed_and_order() {
        let json = serde_json::to_string(&build_pair_plan(5, 77).unwrap()).unwrap();
        assert!(json.contains("\"seed\":77"));
        assert!(json.contains("\"pairs\":5"));
        assert!(json.contains("\"order\""));
        assert!(json.contains("\"AB\"") || json.contains("\"BA\""));
    }
}
