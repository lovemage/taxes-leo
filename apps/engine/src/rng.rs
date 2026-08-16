//! 可重現的亂數來源。
//!
//! 核心規格 3.4：
//! - RNG 演算法與版本固定並寫入 manifest；升級 RNG 視為格式版本變更。
//! - 每手使用由 master seed、hand index 與用途 domain 派生的獨立 stream；
//!   發牌、策略混頻與 Monte Carlo Equity **不得共用同一 stream**。
//! - 相同 manifest 重跑須產生逐事件一致結果。
//!
//! 這裡刻意自行實作而不引入 rand crate：可重現性要求演算法在版本升級後
//! 仍逐位元一致，外部 crate 的內部演算法可能隨版本更動。xoshiro256** 與
//! splitmix64 都有公開的參考實作，行為可長期釘死。

/// 寫入 `RunManifest` 的演算法識別。任何演算法或派生方式的變更都必須
/// 改動這個字串，並視為格式版本變更。
pub const RNG_VERSION: &str = "xoshiro256starstar+splitmix64-derive/v1";

/// 用途 domain。不同 domain 取得互相獨立的 stream，避免發牌消耗量
/// 影響策略取樣結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u64)]
pub enum RngDomain {
    /// 發牌與洗牌
    Deal = 0x1000,
    /// 策略混頻取樣
    StrategyMix = 0x2000,
    /// Monte Carlo Equity
    Equity = 0x3000,
}

/// splitmix64：用於由 (master seed, hand index, domain) 派生 stream 種子。
///
/// 參考實作為公有領域（Steele et al., 2014）。
#[derive(Debug, Clone, Copy)]
struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// xoshiro256**：實際的 stream 產生器。
#[derive(Debug, Clone)]
pub struct Rng {
    state: [u64; 4],
}

impl Rng {
    /// 由 master seed、手序與用途 domain 派生獨立 stream。
    ///
    /// 三個輸入先經 splitmix64 混合，確保相鄰的 seed 或手序不會產生
    /// 相關的 stream。
    #[must_use]
    pub fn derive(master_seed: u64, hand_index: u64, domain: RngDomain) -> Self {
        let mut mixer = SplitMix64(
            master_seed
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .wrapping_add(hand_index.wrapping_mul(0xD1B5_4A32_D192_ED03))
                .wrapping_add(domain as u64),
        );
        let mut state = [0u64; 4];
        for slot in &mut state {
            *slot = mixer.next();
        }
        // xoshiro 的狀態不得全為 0
        if state.iter().all(|&s| s == 0) {
            state[0] = 0x9E37_79B9_7F4A_7C15;
        }
        Self { state }
    }

    pub fn next_u64(&mut self) -> u64 {
        let result = self.state[1]
            .wrapping_mul(5)
            .rotate_left(7)
            .wrapping_mul(9);
        let t = self.state[1] << 17;

        self.state[2] ^= self.state[0];
        self.state[3] ^= self.state[1];
        self.state[1] ^= self.state[2];
        self.state[0] ^= self.state[3];
        self.state[2] ^= t;
        self.state[3] = self.state[3].rotate_left(45);

        result
    }

    /// 產生 `[0, bound)` 的均勻整數。
    ///
    /// 用拒絕取樣消除模偏差。洗牌的均勻性直接影響模擬結果的正確性，
    /// 不能為了省一次迴圈而接受偏差。
    ///
    /// # Panics
    /// `bound` 為 0 時 panic。
    pub fn below(&mut self, bound: u64) -> u64 {
        assert!(bound > 0, "上界必須為正");
        // 落在 [0, threshold) 的值會造成偏差，直接重抽
        let threshold = u64::MAX % bound + 1;
        let threshold = if threshold == bound { 0 } else { threshold };
        loop {
            let value = self.next_u64();
            if value >= threshold {
                return value % bound;
            }
        }
    }

    /// Fisher–Yates 洗牌（自尾端往前）。
    ///
    /// 相同 stream 對相同長度的序列恆產生相同排列，這是重播與
    /// 「同 seed 逐事件一致」的基礎。
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        if items.len() < 2 {
            return;
        }
        for i in (1..items.len()).rev() {
            let bound = u64::try_from(i + 1).expect("序列長度必在 u64 範圍");
            let j = usize::try_from(self.below(bound)).expect("索引必在 usize 範圍");
            items.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 相同輸入產生相同序列() {
        let mut a = Rng::derive(42, 7, RngDomain::Deal);
        let mut b = Rng::derive(42, 7, RngDomain::Deal);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn 不同_domain_產生不同序列() {
        let mut deal = Rng::derive(42, 7, RngDomain::Deal);
        let mut mix = Rng::derive(42, 7, RngDomain::StrategyMix);
        let mut equity = Rng::derive(42, 7, RngDomain::Equity);
        let (a, b, c) = (deal.next_u64(), mix.next_u64(), equity.next_u64());
        assert_ne!(a, b, "發牌與策略取樣不得共用 stream");
        assert_ne!(b, c, "策略取樣與 Equity 不得共用 stream");
        assert_ne!(a, c, "發牌與 Equity 不得共用 stream");
    }

    #[test]
    fn 相鄰手序產生不相關的序列() {
        let mut h0 = Rng::derive(42, 0, RngDomain::Deal);
        let mut h1 = Rng::derive(42, 1, RngDomain::Deal);
        assert_ne!(h0.next_u64(), h1.next_u64());
    }

    #[test]
    fn 洗牌可重現且為排列() {
        let original: Vec<u32> = (0..52).collect();

        let mut a = original.clone();
        Rng::derive(1, 1, RngDomain::Deal).shuffle(&mut a);
        let mut b = original.clone();
        Rng::derive(1, 1, RngDomain::Deal).shuffle(&mut b);
        assert_eq!(a, b, "相同 stream 必須產生相同排列");

        let mut sorted = a.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, original, "洗牌結果必須是原序列的排列");

        let mut c = original.clone();
        Rng::derive(2, 1, RngDomain::Deal).shuffle(&mut c);
        assert_ne!(a, c, "不同 seed 應產生不同排列");
    }

    #[test]
    fn below_的值落在範圍內且涵蓋全部餘數() {
        let mut rng = Rng::derive(9, 9, RngDomain::Deal);
        let mut seen = [false; 7];
        for _ in 0..1000 {
            let v = rng.below(7);
            assert!(v < 7);
            seen[usize::try_from(v).expect("小於 7")] = true;
        }
        assert!(seen.iter().all(|&s| s), "應能取到 0..7 的每個值");
    }
}
