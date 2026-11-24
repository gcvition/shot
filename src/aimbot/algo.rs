//! 匀速定步长系自瞄算法：AFG / NEXO / Nep 三家同源骨架的复刻。
//!
//! 三家都出自 PUBG 系实现的同一结构——**每帧把误差向量归一化成方向，
//! 按固定步长（速度×dt）朝目标走，逐轴钳制不超过剩余误差，亚像素
//! 余量取整**。差异只在"步长的时间包络"：
//!
//! | 算法 | 参考实现 | 包络 |
//! |---|---|---|
//! | `Afg` | `AFG/AimBot/AimBot.cpp::AimBotAPI3` | 无——立刻全速匀速 |
//! | `Nexo` | `NEXO/.../Hack/AimBot.cpp::AimBotAPI` | 三次贝塞尔速度包络，800ms 内 0→100%（默认曲线 `{0.25,0.1,0.25,1.0}`，`PowerfulCurve` 关即钳 t） |
//! | `Nep` | `Neptune` 反编译 `FUN_140045c00` 段 | 随机速度游走：每 ~150ms 抽 ±20% 新乘数，新旧间线性插值（"Rand Aimbot Curve"） |
//!
//! 语义对齐说明：
//! - 原实现按"每 tick 固定 counts"输出（AimThread 毫秒级自旋），
//!   这里在帧粒度下等价成 `speed(counts/s) × dt`；
//! - 原实现的 `Move(x,y)` 即发整数 count，亚像素余量 `RemainMouse`
//!   语义完全一致；
//! - 锁定/换目标/丢锁由 `system.rs` 外层统一管（三家原实现也是
//!   外层线程管目标，AimBotAPI 只管位移）。

use crate::settings::AimAlgo;

use super::humanizer::AimTune;

/// 基础指令速度 = max_spd × 本系数（counts/s）。
/// 匀速系没有加减速段，取 Rain 峰值一半：手感上"比 Rain 更直更稳"。
const BASE_SPD_RATIO: f64 = 0.45;

/// NEXO 贝塞尔包络时长（秒，`NewCurveTime` 默认 800ms）。
const NEXO_CURVE_SECS: f64 = 0.8;

/// Nep 随机游走窗口（秒）：每窗抽一个新乘数并插值过去
/// （反编译 `0x96` 帧阈值 ≈ 150ms 量级）。
const NEP_WALK_SECS: f64 = 0.15;

/// Nep 乘数抖动幅度（"random -20% to +20%"）。
const NEP_WALK_AMP: f64 = 0.2;

/// 匀速系步进器：三模式共用骨架，包络按 `mode` 分发。
pub(crate) struct ConstAim {
    /// 亚像素余量（RemainMouseX/Y 同语义）。
    rx: f64,
    ry: f64,
    /// 本次锁定累计时间（包络/游走时钟；retarget/松键归零）。
    t: f64,
    /// Nep：当前游走段起点乘数（x/y 各一）。
    walk_from_x: f64,
    walk_from_y: f64,
    /// Nep：当前游走段目标乘数。
    walk_to_x: f64,
    walk_to_y: f64,
    /// Nep：当前游走段起始时刻。
    walk_t0: f64,
    /// xorshift 随机状态。
    seed: u64,
    /// 上次点击时刻（扳机冷却）。
    last_click: f64,
}

impl Default for ConstAim {
    fn default() -> Self {
        Self {
            rx: 0.0,
            ry: 0.0,
            t: 0.0,
            walk_from_x: 1.0,
            walk_from_y: 1.0,
            walk_to_x: 1.0,
            walk_to_y: 1.0,
            walk_t0: 0.0,
            seed: 0x9E37_79B9_7F4A_7C15,
            last_click: f64::NEG_INFINITY,
        }
    }
}

impl ConstAim {
    /// 换目标/松键：清余量、归零包络时钟（对齐 StopAiming 的
    /// `RemainMouseX/Y=0; BezierCurrentTime=0` 语义）。
    pub fn reset(&mut self) {
        self.rx = 0.0;
        self.ry = 0.0;
        self.t = 0.0;
        self.walk_from_x = 1.0;
        self.walk_from_y = 1.0;
        self.walk_to_x = 1.0;
        self.walk_to_y = 1.0;
        self.walk_t0 = 0.0;
    }

    fn rand(&mut self) -> f64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed >> 11) as f64 / (1u64 << 53) as f64
    }

    /// 包络乘数（x, y）：对基础速度的逐轴缩放。
    /// `now` 是绝对 sim 秒（仅 Nep 游走窗口边界判定需要 `self.t`）。
    fn env_mul(&mut self, mode: AimAlgo) -> (f64, f64) {
        match mode {
            // AFG：无包络，恒定全速。
            AimAlgo::Afg | AimAlgo::Rain => (1.0, 1.0),
            // NEXO：三次贝塞尔 ramp——CurveMultiplier = Bezier(0,c1,c2,1,t/T)，
            // PowerfulCurve 关 → t 钳到 1（原实现 else 分支同语义）。
            AimAlgo::Nexo => {
                let t = (self.t / NEXO_CURVE_SECS).clamp(0.0, 1.0);
                // Curves = {0.25, 0.1, 0.25, 1.0}：X 用 [0],[2]，Y 用 [1],[3]。
                (bezier01(0.25, 0.25, t), bezier01(0.1, 1.0, t))
            }
            // Nep：随机速度游走——窗口到期抽新目标乘数，
            // 窗口内新旧线性插值（"非平滑曲线"的 RNG 调制）。
            AimAlgo::Nep => {
                if self.t - self.walk_t0 >= NEP_WALK_SECS {
                    self.walk_from_x = self.walk_to_x;
                    self.walk_from_y = self.walk_to_y;
                    self.walk_to_x = 1.0 + (self.rand() * 2.0 - 1.0) * NEP_WALK_AMP;
                    self.walk_to_y = 1.0 + (self.rand() * 2.0 - 1.0) * NEP_WALK_AMP;
                    self.walk_t0 = self.t;
                }
                let u = ((self.t - self.walk_t0) / NEP_WALK_SECS).clamp(0.0, 1.0);
                (
                    self.walk_from_x + (self.walk_to_x - self.walk_from_x) * u,
                    self.walk_from_y + (self.walk_to_y - self.walk_from_y) * u,
                )
            }
        }
    }

    /// 本帧步进：`(dx,dy)` = 误差 counts（含提前量），返回整数输出。
    /// 骨架同 `AimBotAPI3`/`AimBotAPI`：方向归一化 → 定步长 → 逐轴钳
    /// 制 → 亚像素取整。
    pub fn step(&mut self, dx: f64, dy: f64, dt: f64, cfg: &AimTune, mode: AimAlgo) -> (i32, i32) {
        let len = dx.hypot(dy);
        if !(dt.is_finite() && dt > 0.0) || len < 1e-9 {
            return (0, 0);
        }
        self.t += dt;
        let (ux, uy) = (dx / len, dy / len);
        let (ex, ey) = self.env_mul(mode);
        // 逐轴指令速度 counts/s：方向分量 × 轴速度 × 包络。
        let vx = cfg.max_spd_x * BASE_SPD_RATIO * cfg.spd_mul_x * ex;
        let vy = cfg.max_spd_y * BASE_SPD_RATIO * cfg.spd_mul_y * ey;
        // 定步长，逐轴钳制到剩余误差（不冲过目标）。
        let mx = (vx * ux * dt).clamp(-dx.abs(), dx.abs());
        let my = (vy * uy * dt).clamp(-dy.abs(), dy.abs());
        self.rx += mx;
        self.ry += my;
        let ix = self.rx.trunc() as i32;
        let iy = self.ry.trunc() as i32;
        self.rx -= ix as f64;
        self.ry -= iy as f64;
        (ix, iy)
    }

    /// 当前指令速度 counts/s（intercept 提前量估计用：
    /// 到达时间 ≈ 剩余距离 / 当前速度）。
    pub fn speed_now(&self, cfg: &AimTune, mode: AimAlgo) -> f64 {
        let (ex, ey) = match mode {
            AimAlgo::Nexo => {
                let t = (self.t / NEXO_CURVE_SECS).clamp(0.0, 1.0);
                (bezier01(0.25, 0.25, t), bezier01(0.1, 1.0, t))
            }
            AimAlgo::Nep => {
                let u = ((self.t - self.walk_t0) / NEP_WALK_SECS).clamp(0.0, 1.0);
                (
                    self.walk_from_x + (self.walk_to_x - self.walk_from_x) * u,
                    self.walk_from_y + (self.walk_to_y - self.walk_from_y) * u,
                )
            }
            _ => (1.0, 1.0),
        };
        let vx = cfg.max_spd_x * BASE_SPD_RATIO * cfg.spd_mul_x * ex;
        let vy = cfg.max_spd_y * BASE_SPD_RATIO * cfg.spd_mul_y * ey;
        // 方向未知时按两轴均值的量级给（lead 只需要数量级）。
        (vx + vy) * 0.5
    }

    /// 扳机：误差进逐轴容差即开（匀速系收敛到点上，无保持态概念），
    /// 带与 Rain 相同的点击冷却语义。
    pub fn should_click(&mut self, dx: f64, dy: f64, w: f64, cfg: &AimTune, now: f64) -> bool {
        dx.abs() <= w && dy.abs() <= w && now - self.last_click > cfg.click_cd
    }

    /// 记录开火并返回按住时长（20~45ms，与 Rain 同一分布）。
    pub fn click_hold_secs(&mut self, now: f64) -> f64 {
        self.last_click = now;
        0.020 + self.rand() * 0.025
    }

    /// 日志/调试阶段名。
    pub fn phase_name(mode: AimAlgo) -> &'static str {
        match mode {
            AimAlgo::Afg => "Afg",
            AimAlgo::Nexo => "Nexo",
            AimAlgo::Nep => "Nep",
            AimAlgo::Rain => "Rain",
        }
    }
}

/// 三次贝塞尔 `Bezier(0, c1, c2, 1, t)` 的值（NEXO `BezierValue`
/// 定点版：p0=0、p3=1）。
fn bezier01(c1: f64, c2: f64, t: f64) -> f64 {
    let u = 1.0 - t;
    3.0 * u * u * t * c1 + 3.0 * u * t * t * c2 + t * t * t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::AimAlgo;

    const DT: f64 = 1.0 / 125.0;

    fn drive(a: &mut ConstAim, cfg: &AimTune, mode: AimAlgo, dx: f64, dy: f64, n: u32) -> (f64, f64) {
        let (mut dx, mut dy) = (dx, dy);
        for _ in 0..n {
            let (ix, iy) = a.step(dx, dy, DT, cfg, mode);
            dx -= ix as f64;
            dy -= iy as f64;
        }
        (dx, dy)
    }

    /// 三模式都应收敛到目标（钳制保证不越过零点）。
    #[test]
    fn all_modes_should_converge() {
        for mode in [AimAlgo::Afg, AimAlgo::Nexo, AimAlgo::Nep] {
            let cfg = AimTune::default();
            let mut a = ConstAim::default();
            let (dx, dy) = drive(&mut a, &cfg, mode, 300.0, -200.0, 400);
            assert!(
                dx.hypot(dy) < 5.0,
                "{mode:?} residual={dx:.1},{dy:.1}"
            );
        }
    }

    /// 匀速骨架特征：AFG 在误差足够大时每步速度恒定（无非线性衰减）。
    #[test]
    fn afg_should_move_constant_speed() {
        let cfg = AimTune::default();
        let mut a = ConstAim::default();
        let mut speeds = Vec::new();
        for _ in 0..20 {
            let (ix, _) = a.step(1000.0, 0.0, DT, &cfg, AimAlgo::Afg);
            speeds.push(ix.abs());
        }
        // 前 20 帧误差远大于步长 → 每帧输出应基本一致（±亚像素抖动 1）。
        let max = *speeds.iter().max().unwrap();
        let min = *speeds.iter().min().unwrap();
        assert!(max - min <= 1, "constant speed violated: {speeds:?}");
        assert!(max > 0);
    }

    /// NEXO 包络：起点速度应低于末段（0→100% ramp）。
    #[test]
    fn nexo_should_ramp_speed() {
        let cfg = AimTune::default();
        let mut a = ConstAim::default();
        // 头 3 帧（≈24ms，包络刚起）的平均速度应显著小于
        // 600ms 后的平均速度。
        let mut early = 0.0f64;
        for _ in 0..3 {
            let (ix, _) = a.step(5000.0, 0.0, DT, &cfg, AimAlgo::Nexo);
            early += ix.abs() as f64;
        }
        // 快进时钟到包络后段：直接推 t。
        a.t = NEXO_CURVE_SECS * 0.9;
        let mut late = 0.0f64;
        for _ in 0..10 {
            let (ix, _) = a.step(5000.0, 0.0, DT, &cfg, AimAlgo::Nexo);
            late += ix.abs() as f64;
        }
        assert!(
            late / 10.0 > early / 3.0,
            "early={early:.1}/3 late={late:.1}/10"
        );
    }

    /// Nep 游走：长样本下乘数应变化且停在 [0.8,1.2] 内。
    #[test]
    fn nep_walk_should_stay_bounded() {
        let mut a = ConstAim::default();
        let mut mins = 2.0f64;
        let mut maxs = 0.0f64;
        for i in 0..200 {
            a.t = i as f64 * DT;
            let (ex, _) = a.env_mul(AimAlgo::Nep);
            mins = mins.min(ex);
            maxs = maxs.max(ex);
        }
        assert!(mins >= 0.79 && maxs <= 1.21, "walk bounds {mins}..{maxs}");
        assert!(maxs - mins > 0.05, "walk should wander");
    }

    /// 亚像素余量：匀速小步长下不丢步（RemainMouse 语义）。
    #[test]
    fn subpixel_should_accumulate() {
        let cfg = AimTune {
            max_spd_x: 50.0, // 50*0.45=22.5 counts/s → 0.18 counts/帧
            ..AimTune::default()
        };
        let mut a = ConstAim::default();
        let mut total = 0i32;
        for _ in 0..100 {
            let (ix, _) = a.step(1000.0, 0.0, DT, &cfg, AimAlgo::Afg);
            total += ix;
        }
        assert!(total > 0, "subpixel steps must accumulate, got {total}");
    }

    /// 逐轴钳制：误差小于一步时不应越过零点（与原 clamp 语义一致）。
    #[test]
    fn should_not_overshoot_past_target() {
        let cfg = AimTune::default();
        let mut a = ConstAim::default();
        let (ix, _) = a.step(0.5, 0.0, DT, &cfg, AimAlgo::Afg);
        // err=0.5 < 步长 → 输出至多是 err 取整 0 或 1，绝不能 > 1。
        assert!(ix <= 1, "clamped step out={ix}");
        // 反向：误差 -0.5 输出 ∈ [-1,0]。
        let (ix, _) = a.step(-0.5, 0.0, DT, &cfg, AimAlgo::Afg);
        assert!((-1..=1).contains(&ix));
    }
}
