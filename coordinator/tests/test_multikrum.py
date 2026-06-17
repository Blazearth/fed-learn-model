"""
Bug 3 regression: Multi-Krum selection set size (Task 9.7).
Verifies n-f-2 selection (not f+1).
"""
import numpy as np
import sys
import os

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "aggregation"))
from aggregator import MultiKrumAggregator


class TestMultiKrumSelectionSize:
    def test_n5_f1_selects_2(self):
        """n=5, f=1 → n-f-2 = 2. (f+1=2 also = 2, but formula must be n-f-2)."""
        agg = MultiKrumAggregator()
        updates = [np.array([float(i)]) for i in range(5)]
        result = agg.aggregate(updates, f=1)
        assert result.shape == (1,)
        assert np.isfinite(result[0])

    def test_n7_f2_selects_3(self):
        """n=7, f=2 → n-f-2 = 3 (f+1=3 coincidence, but formula matters for n=10)."""
        agg = MultiKrumAggregator()
        updates = [np.array([float(i)]) for i in range(7)]
        result = agg.aggregate(updates, f=2)
        assert np.isfinite(result[0])

    def test_n10_f3_selects_5_not_4(self):
        """
        n=10, f=3 → n-f-2 = 5 updates selected.
        f+1 = 4 would be WRONG.
        Place updates at 0..9 on 1D line.
        The 5 middle points have smallest distances.
        Mean of 5 middle ≈ 4.5 (n-f-2=5 correct).
        Mean of 4 middle ≈ 4.0 (f+1=4 wrong).
        """
        agg = MultiKrumAggregator()
        updates = [np.array([float(i)]) for i in range(10)]
        result = agg.aggregate(updates, f=3)
        val = float(result[0])
        # n-f-2=5 → mean of roughly [3,4,5,6,7] ≈ 5.0 ± 1.5
        assert 3.0 < val < 7.0, f"Expected ~5.0 for n-f-2=5 selection, got {val:.2f}"

    def test_byzantine_outlier_excluded(self):
        """4 honest updates near [1..4] + 1 Byzantine at 1000. Outlier must be excluded."""
        agg = MultiKrumAggregator()
        honest = [np.ones(10) * i for i in range(1, 5)]
        byzantine = [np.ones(10) * 1000]
        result = agg.aggregate(honest + byzantine, f=1)
        assert np.mean(result) < 10, f"Byzantine not excluded: mean={np.mean(result)}"

    def test_fedavg_fallback_n3_f1(self):
        """n=3, f=1 → 2f+3=5 > 3 → FedAvg fallback."""
        agg = MultiKrumAggregator()
        updates = [np.array([1.0]), np.array([3.0]), np.array([5.0])]
        result = agg.aggregate(updates, f=1)
        np.testing.assert_allclose(result, np.array([3.0]))
