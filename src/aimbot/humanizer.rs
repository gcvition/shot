//! 拟人化自瞄状态机——按 Rain `docs/design/aimbot.md` §12 推荐组合实现。
//!
//! 流水线（文档终态；hitscan 下弹道层退化为恒等，部位选择退化为单点）：
//!   反应延迟 N(μ,σ) → Fitts 定时 → 主运动（minimum-jerk 速度包络，
//!   X/Y 逐轴同 duration 不同速）→ 反馈停顿 → P 修正段 → 保持/跟枪态
//!   （OU tremor + 轻量 P）。输出经节拍抖动 + RemainMouse 亚像素取整。
//!
//! 与上游 v0 `rain-features/src/aim.rs` 的关系：接口同形
//! （`step`/`should_click`/`click_hold_secs`/`retarget`/`locked`），
//! 内部由弹簧-速度模型换成文档的双子运动骨架；§14.5 的签名差异
//! 是 `step` 追加了逐目标容差 `w`（文档 pick_target 三元组里的 W）。
//!
//! 与上游一致的刻意差异：
//! - 时间用调用方传入的 `now`（模拟秒）而非 `Instant`——headless 可复现；
//! - 位移单位是鼠标 count（= apply_look 的 dx/dy 单位），不是屏幕像素；
//! - `click_hold_secs` 返回秒，调用方负责松键。
//!
//! 常量口径：§13 要求"参数带物理含义注释，禁止参考实现魔数"——
//! 每项都标了含义；数值按 §3/§10/§12 的建议区间自定，不与
//! AFG/NEXO 常量重合（指纹差异化）。

/// 拉枪阶段（§12 流水线状态机）。
#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    /// 未锁定/未激活。
    Idle,
    /// 反应延迟：新目标出现到开始移动（视觉-运动反射弧）。
    React { until: f64 },
    /// 主运动：minimum-jerk 速度包络覆盖大部分距离（含欠/过冲偏置）。
    Flick(Flick),
    /// 反馈停顿：主运动结束到修正开始（视觉确认时间）。
    Pause { until: f64 },
    /// 修正段：P 控制小幅收敛到逐轴容差内（1–3 次视觉修正）。
    Correct { since: f64 },
    /// 保持/跟枪：容差内环绕目标点，扳机条件在这里判。
    Hold,
}

/// 一次主运动的逐拉枪参数（§11 参数随机化：每次拉枪采样，
/// 让轨迹集合的统计分布覆盖人类分布而非收敛成一个点）。
#[derive(Clone, Copy, Debug, PartialEq)]
struct Flick {
    /// 起始时刻（模拟秒）。
    start: f64,
    /// Fitts 算出的计划耗时（秒）。
    dur: f64,
    /// 每轴计划覆盖量（counts，带符号；|cover|>|err0| 即过冲）。
    cover_x: f64,
    cover_y: f64,
    /// 已输出覆盖量。
    done_x: f64,
    done_y: f64,
}

/// 拟人化参数表（单位：鼠标 count / 秒；时间秒）。
/// 每项注释写物理含义——§13「常量审计」禁止来源不明的魔数。
#[derive(Clone, Copy, Debug)]
pub struct AimTune {
    /// 索敌半径（准星偏差 counts，超出不选为目标）。
    pub fov: f64,
    /// Fitts 截距 a：与难度无关的基础耗时。
    pub fitts_a: f64,
    /// Fitts 斜率 b：每 bit 难度（log2(D/W+1)）的耗时。
    pub fitts_b: f64,
    /// b 的逐拉枪采样幅度（±比例；§11 参数随机化）。
    pub fitts_b_spread: f64,
    /// 水平速度上限 counts/s（横向行程长、手腕发力容易，天然更快）。
    pub max_spd_x: f64,
    /// 垂直速度上限 counts/s。
    pub max_spd_y: f64,
    /// 主运动覆盖比例下限（欠冲带：人偏保守）。
    pub cover_lo: f64,
    /// 主运动覆盖比例上限。
    pub cover_hi: f64,
    /// 失误率：每次拉枪注入过冲的概率（§12 用 15%）。
    pub miss_p: f64,
    /// 过冲倍率下限（失误时的覆盖倍率）。
    pub over_lo: f64,
    /// 过冲倍率上限。
    pub over_hi: f64,
    /// 主运动后视觉反馈停顿下限（秒）。
    pub pause_lo: f64,
    /// 停顿上限。
    pub pause_hi: f64,
    /// 修正段 P 增益 /s（误差逐 tick 指数收敛）。
    pub corr_k: f64,
    /// 修正段最长耗时（超时也进保持态——人会接受"差不多"）。
    pub corr_timeout: f64,
    /// 保持态跟枪 P 增益 /s（轻量追踪，不抢 tremor 的主导地位）。
    pub track_k: f64,
    /// 保持态脱靶再拉枪阈值 = w × 本值（目标跑远重新甩）。
    pub reflick_ratio: f64,
    /// OU tremor 均值回复率 /s（对应 ~8–12Hz 生理震颤频段）。
    pub ou_theta: f64,
    /// OU tremor 扩散强度：位置偏移的稳态 std = σ/√(2θ) counts。
    /// 是"瞄准点徘徊范围"不是速度——有界，不会随帧率漂移放大。
    pub ou_sigma: f64,
    /// 反应延迟均值（秒，§10 的 N(190,40)ms）。
    pub react_mean: f64,
    /// 反应延迟标准差。
    pub react_sd: f64,
    /// 反应延迟下限（生理极限 ~120ms）。
    pub react_min: f64,
    /// 主运动中微停顿概率/帧（人手分段发力）。
    pub stall_p: f64,
    /// 输出节拍目标间隔均值（秒，§13 的 250–500Hz 区间）。
    pub emit_mean: f64,
    /// 节拍抖动：到点帧仍按该概率挂起一帧（帧粒度下的间隔噪声近似）。
    pub emit_holdover_p: f64,
    /// 两次点击最小间隔（秒）。
    pub click_cd: f64,
    /// 用户可调水平速度倍率（乘在 X 轴限速/增益上，Fitts 时长按均值缩放）。
    pub spd_mul_x: f64,
    /// 用户可调垂直速度倍率。
    pub spd_mul_y: f64,
}

impl Default for AimTune {
    fn default() -> Self {
        Self {
            fov: 600.0,
            fitts_a: 0.100,
            fitts_b: 0.090,
            fitts_b_spread: 0.15,
            max_spd_x: 3000.0,
            max_spd_y: 2000.0,
            // 欠冲带收窄 0.92–0.98：少进 Correct（匀速系观感更丝滑的
            // 原因之一就是没有修正段）；失误率压到 6%、过冲限 1.1–1.3×
            // ——15%×1.6 的过冲在训练场里就是"准星拉过头"（实测反馈），
            // 反检测需求 Rain 侧另调。
            cover_lo: 0.92,
            cover_hi: 0.98,
            miss_p: 0.06,
            over_lo: 1.10,
            over_hi: 1.30,
            // 停顿收窄 20–50ms：34fps 下 30–80ms = 1–3 帧完全不动，
            // 是"一顿一顿"观感的主因之一。
            pause_lo: 0.020,
            pause_hi: 0.050,
            corr_k: 10.0,
            corr_timeout: 0.60,
            track_k: 6.0,
            reflick_ratio: 4.0,
            // θ=8：相关时间 ~125ms（>34fps 帧时间 29ms）——逐帧增量在
            // 数帧内保持同向，呈"缓慢手漂"而非逐帧白噪声；θ=60 时相关
            // 16ms<帧时间，退化成每帧反号 = 肉眼可见的抖。
            ou_theta: 8.0,
            // 稳态 std = σ/√(2θ) = 10/4 = 2.5 counts 的手漂幅度。
            ou_sigma: 10.0,
            react_mean: 0.190,
            react_sd: 0.040,
            react_min: 0.120,
            stall_p: 0.015,
            emit_mean: 0.003,
            emit_holdover_p: 0.10,
            click_cd: 0.160,
            spd_mul_x: 1.0,
            spd_mul_y: 1.0,
        }
    }
}

/// 拟人化移动器：双子运动状态机 + 噪声层。
pub struct Humanizer {
    phase: Phase,
    /// 亚像素余量（RemainMouse：浮点步进只发整数，余量累计）。
    rx: f64,
    ry: f64,
    /// 待发送的浮点移动（节拍挂起时暂存）。
    pending_x: f64,
    pending_y: f64,
    /// 距下次发送的剩余时间。
    emit_in: f64,
    /// OU tremor 状态（x/y 各一）：瞄准点位置偏移，有界徘徊。
    ou_x: f64,
    ou_y: f64,
    /// 上帧已输出的 OU 偏移（位置模型发增量而非速度积分）。
    ou_px: f64,
    ou_py: f64,
    /// 锁定目标 ID（由调用方填写）。
    pub locked: Option<u64>,
    /// xorshift 随机状态（每会话一个种子）。
    seed: u64,
    /// 上次点击时刻（模拟秒；冷却判定用）。
    last_click: f64,
    /// 最近一次 step 的 now（lead 估计用）。
    last_now: f64,
    /// 诊断：上帧输出分解 = (阶段步长x, y, tremor增量x, y, 暂存x, y)。
    pub last_dbg: (f64, f64, f64, f64, f64, f64),
}

impl Default for Humanizer {
    fn default() -> Self {
        Self::new()
    }
}

impl Humanizer {
    pub fn new() -> Self {
        Self {
            phase: Phase::Idle,
            rx: 0.0,
            ry: 0.0,
            pending_x: 0.0,
            pending_y: 0.0,
            emit_in: 0.0,
            ou_x: 0.0,
            ou_y: 0.0,
            ou_px: 0.0,
            ou_py: 0.0,
            locked: None,
            seed: 0x9E37_79B9_7F4A_7C15,
            last_click: f64::NEG_INFINITY,
            last_now: 0.0,
            last_dbg: (0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
        }
    }

    /// xorshift64 伪随机 [0,1)。
    fn rand(&mut self) -> f64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed >> 11) as f64 / (1u64 << 53) as f64
    }

    /// 标准正态（Box-Muller）。
    fn gauss(&mut self) -> f64 {
        let u1 = (1.0 - self.rand()).max(1e-12);
        let u2 = self.rand();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }

    /// 换目标：全部内部状态清零（§13 StopAiming），进反应延迟。
    pub fn retarget(&mut self, now: f64, cfg: &AimTune) {
        self.phase = Phase::React {
            until: now + self.react_delay(cfg),
        };
        self.rx = 0.0;
        self.ry = 0.0;
        self.pending_x = 0.0;
        self.pending_y = 0.0;
        self.emit_in = 0.0;
        self.ou_x = 0.0;
        self.ou_y = 0.0;
        self.ou_px = 0.0;
        self.ou_py = 0.0;
    }

    /// 松键/停用：清空一切内部状态（§13 StopAiming）。
    pub fn stop(&mut self) {
        self.phase = Phase::Idle;
        self.locked = None;
        self.rx = 0.0;
        self.ry = 0.0;
        self.pending_x = 0.0;
        self.pending_y = 0.0;
        self.emit_in = 0.0;
        self.ou_x = 0.0;
        self.ou_y = 0.0;
        self.ou_px = 0.0;
        self.ou_py = 0.0;
    }

    fn react_delay(&mut self, cfg: &AimTune) -> f64 {
        (cfg.react_mean + cfg.react_sd * self.gauss()).max(cfg.react_min)
    }

    /// 当前阶段名（日志/调试）。
    pub fn phase_name(&self) -> &'static str {
        match self.phase {
            Phase::Idle => "Idle",
            Phase::React { .. } => "React",
            Phase::Flick(_) => "Flick",
            Phase::Pause { .. } => "Pause",
            Phase::Correct { .. } => "Correct",
            Phase::Hold => "Hold",
        }
    }

    /// intercept 提前量（秒）：匀速外推时间 = 剩余拉枪时间的估计。
    /// hitscan 无飞行时间，lead 只需补拉枪剩余时间内的目标位移。
    pub fn est_lead(&self, cfg: &AimTune) -> f64 {
        match self.phase {
            Phase::Flick(f) => (f.start + f.dur - self.last_now).max(0.0),
            Phase::Correct { .. } => (1.0 / cfg.corr_k).min(0.3),
            Phase::Hold => 0.05,
            _ => 0.0,
        }
    }

    /// 本帧追踪步进。`(dx,dy)` = 当前误差 counts（含提前量修正后）；
    /// `w` = 该目标逐轴命中容差 counts；`now`/`dt` 模拟秒。
    /// 返回本 tick 应下发的整数移动量。
    pub fn step(
        &mut self,
        dx: f64,
        dy: f64,
        dt: f64,
        w: f64,
        cfg: &AimTune,
        now: f64,
    ) -> (i32, i32) {
        if !(dt.is_finite() && dt > 0.0) || self.phase == Phase::Idle {
            return (0, 0);
        }
        self.last_now = now;
        let dt = dt.min(0.1); // 卡顿帧不让包络跳步
        let (mut mx, mut my) = (0.0, 0.0);
        let w = w.max(1.0); // 容差下限：无可命中区域时也能收敛

        match self.phase {
            Phase::Idle => {}
            Phase::React { until } => {
                // 反应延迟期间保持不动（只有 tremor 由下方噪声层叠加）。
                if now >= until {
                    self.plan_flick(dx, dy, w, cfg, now);
                }
            }
            Phase::Flick(f) => {
                // 微停顿：低概率整帧不动（人手分段发力），计划时间照扣。
                if self.rand() < cfg.stall_p {
                    self.phase = Phase::Flick(Flick {
                        start: f.start + dt,
                        ..f
                    });
                } else {
                    let tau = ((now - f.start) / f.dur).clamp(0.0, 1.0);
                    // minimum-jerk 速度包络 prof' = 30τ²(1−τ)²,峰值 1.875。
                    let shape = 30.0 * tau * tau * (1.0 - tau) * (1.0 - tau);
                    let sx = flick_axis(
                        f.cover_x,
                        f.done_x,
                        shape,
                        f.dur,
                        cfg.max_spd_x * cfg.spd_mul_x,
                        dt,
                    );
                    let sy = flick_axis(
                        f.cover_y,
                        f.done_y,
                        shape,
                        f.dur,
                        cfg.max_spd_y * cfg.spd_mul_y,
                        dt,
                    );
                    mx += sx;
                    my += sy;
                    let mut nf = f;
                    nf.done_x += sx;
                    nf.done_y += sy;
                    // 退出：包络走完 / 计划覆盖完成 / 欠冲模式下误差已进容差。
                    let plan_done = (nf.cover_x - nf.done_x).abs() <= w
                        && (nf.cover_y - nf.done_y).abs() <= w;
                    let in_tol = dx.abs() <= w && dy.abs() <= w;
                    if in_tol {
                        // 直接落点：跳过低效 Pause+Correct 直接进保持态
                        // （匀速系"丝滑"的关键就是没有中间停顿相位）。
                        self.phase = Phase::Hold;
                    } else if tau >= 1.0 || plan_done {
                        self.phase = Phase::Pause {
                            until: now
                                + cfg.pause_lo
                                + self.rand() * (cfg.pause_hi - cfg.pause_lo),
                        };
                    } else {
                        self.phase = Phase::Flick(nf);
                    }
                }
            }
            Phase::Pause { until } => {
                if now >= until {
                    self.phase = Phase::Correct { since: now };
                }
            }
            Phase::Correct { since } => {
                // P 修正：逐轴指数收敛，限低速——修正段是小碎步逼近，
                // 限到 0.2× 上限让 34fps 下单帧步长 ≤~12 counts 不跳变。
                // 再按剩余误差封顶 0.6×：corr_k·spd·dt 在卡顿帧可 >1
                // （增益>1=每帧反向超调=实测 Correct 31% 翻转），
                // 步长≤0.6|err| 则任何 dt 下闭环都收敛且单帧不瞬移。
                mx += (dx * cfg.corr_k * cfg.spd_mul_x * dt)
                    .clamp(
                        -cfg.max_spd_x * cfg.spd_mul_x * 0.3 * dt,
                        cfg.max_spd_x * cfg.spd_mul_x * 0.3 * dt,
                    )
                    .clamp(-dx.abs() * 0.6, dx.abs() * 0.6);
                my += (dy * cfg.corr_k * cfg.spd_mul_y * dt)
                    .clamp(
                        -cfg.max_spd_y * cfg.spd_mul_y * 0.3 * dt,
                        cfg.max_spd_y * cfg.spd_mul_y * 0.3 * dt,
                    )
                    .clamp(-dy.abs() * 0.6, dy.abs() * 0.6);
                let in_tol = dx.abs() <= w && dy.abs() <= w;
                if in_tol || now - since > cfg.corr_timeout {
                    self.phase = Phase::Hold;
                }
            }
            Phase::Hold => {
                // 逐轴死区：误差已在命中容差内就不修（人不会为亚容差
                // 偏差动腕；同时也避免把自身 tremor 当误差拉回，自激）。
                if dx.abs() > w {
                    mx += (dx * cfg.track_k * cfg.spd_mul_x * dt)
                        .clamp(
                            -cfg.max_spd_x * cfg.spd_mul_x * 0.2 * dt,
                            cfg.max_spd_x * cfg.spd_mul_x * 0.2 * dt,
                        )
                        .clamp(-dx.abs() * 0.6, dx.abs() * 0.6);
                }
                if dy.abs() > w {
                    my += (dy * cfg.track_k * cfg.spd_mul_y * dt)
                        .clamp(
                            -cfg.max_spd_y * cfg.spd_mul_y * 0.2 * dt,
                            cfg.max_spd_y * cfg.spd_mul_y * 0.2 * dt,
                        )
                        .clamp(-dy.abs() * 0.6, dy.abs() * 0.6);
                }
                // 脱靶超阈值 → 重新甩枪（已注意目标，不加反应延迟）。
                if dx.hypot(dy) > w * cfg.reflick_ratio {
                    self.plan_flick(dx, dy, w, cfg, now);
                }
            }
        }

        // OU tremor = 瞄准点的有界位置偏移：输出 ou 增量而非速度积分。
        // 速度模型在粗帧率下每帧踢 ±dt×60 counts 且会漂移——34fps 实测
        // 单帧 ±17；位置模型把徘徊总幅度钉在 OU 稳态范围内（≈±3σ~9
        // counts≈0.2°），手抖质感保留、漂移消失（§3/§10 的 tremor 需求）。
        let tremor_scale = match self.phase {
            Phase::Hold => 1.0,
            Phase::Pause { .. } => 0.5,
            Phase::Correct { .. } => 0.35,
            Phase::React { .. } => 0.3,
            Phase::Flick(_) => 0.2,
            Phase::Idle => 0.0,
        };
        // 徘徊幅度再封顶 w×0.25（稳态 std 口径）：远目标 w 只有几 counts
        // 时 tremor 不会把开火误差顶出 w×0.8 门限、也不会推出命中圈。
        let ou_std = cfg.ou_sigma / (2.0 * cfg.ou_theta).sqrt();
        let tremor_scale = tremor_scale * (w * 0.25 / ou_std.max(0.1)).min(1.0);
        let sq = dt.sqrt();
        self.ou_x += -cfg.ou_theta * self.ou_x * dt + cfg.ou_sigma * sq * self.gauss();
        self.ou_y += -cfg.ou_theta * self.ou_y * dt + cfg.ou_sigma * sq * self.gauss();
        // ou_px 记"上帧已输出的偏移"：缩放因子变化时增量自动把
        // 累计偏移对齐到新的期望值，不产生阶跃。
        let off_x = self.ou_x * tremor_scale;
        let off_y = self.ou_y * tremor_scale;
        let trm = (off_x - self.ou_px, off_y - self.ou_py);
        mx += trm.0;
        my += trm.1;
        self.ou_px = off_x;
        self.ou_py = off_y;
        // 诊断：分解本帧输出构成（阶段步长 / tremor 增量 / 暂存量）。
        self.last_dbg = (mx - trm.0, my - trm.1, trm.0, trm.1, self.pending_x, self.pending_y);

        // 节拍抖动：emit_in 到点才发，且按概率再挂起一帧
        // （帧粒度的宿主里近似 §10/§13 的 2–4ms+噪声输出间隔）。
        // 挂起概率按 emit_mean/dt 衰减：帧比目标节拍还粗时（34fps 实测）
        // 挂起一帧=下帧双倍输出，变成 0/2× 交替抖动而非噪声。
        self.pending_x += mx;
        self.pending_y += my;
        self.emit_in -= dt;
        let ho_p = cfg.emit_holdover_p * (cfg.emit_mean / dt.max(cfg.emit_mean)).min(1.0);
        if self.emit_in <= 0.0 && self.rand() >= ho_p {
            self.rx += self.pending_x;
            self.ry += self.pending_y;
            self.pending_x = 0.0;
            self.pending_y = 0.0;
            self.emit_in = cfg.emit_mean * (0.7 + 0.6 * self.rand());
            let ix = self.rx.trunc() as i32;
            let iy = self.ry.trunc() as i32;
            self.rx -= ix as f64;
            self.ry -= iy as f64;
            (ix, iy)
        } else {
            if self.emit_in <= 0.0 {
                // 挂起路径同样给间隔噪声（节拍抖动对两条路径一致）。
                self.emit_in = cfg.emit_mean * (0.7 + 0.6 * self.rand());
            }
            (0, 0)
        }
    }

    /// 扳机门控：保持态 + 逐轴残余 ≤ 容差 + 过冷却
    /// （§12：扳机条件 = 残余误差 ≤ 该部位容差）。
    pub fn should_click(&self, dx: f64, dy: f64, w: f64, cfg: &AimTune, now: f64) -> bool {
        self.phase == Phase::Hold
            && dx.abs() <= w
            && dy.abs() <= w
            && now - self.last_click > cfg.click_cd
    }

    /// 记录一次开火并返回按住时长（20~45ms 随机，像人手）。
    pub fn click_hold_secs(&mut self, now: f64) -> f64 {
        self.last_click = now;
        0.020 + self.rand() * 0.025
    }

    /// 规划一次主运动：Fitts 定时 + 逐拉枪采样欠/过冲覆盖。
    fn plan_flick(&mut self, dx: f64, dy: f64, w: f64, cfg: &AimTune, now: f64) {
        let dist = dx.hypot(dy).max(w);
        let b = cfg.fitts_b * (1.0 + (self.rand() * 2.0 - 1.0) * cfg.fitts_b_spread);
        // 速度倍率：两轴同 duration（保持自然弧），按均值反比缩时。
        let mul = ((cfg.spd_mul_x + cfg.spd_mul_y) * 0.5).max(0.2);
        let dur = (cfg.fitts_a + b * (dist / w + 1.0).log2().max(0.0)) / mul;
        // 失误率：过冲 1.2–1.6×；否则欠冲带 0.88–0.95（人偏保守）。
        let frac = if self.rand() < cfg.miss_p {
            cfg.over_lo + self.rand() * (cfg.over_hi - cfg.over_lo)
        } else {
            cfg.cover_lo + self.rand() * (cfg.cover_hi - cfg.cover_lo)
        };
        self.phase = Phase::Flick(Flick {
            start: now,
            dur: dur.max(0.05),
            cover_x: dx * frac,
            cover_y: dy * frac,
            done_x: 0.0,
            done_y: 0.0,
        });
    }
}

/// 单轴主运动步进：minimum-jerk 速度包络 × 计划覆盖的增益表形式。
/// 欠冲计划 rem_cover<err（绝不越过零点）；过冲计划 rem_cover>err
/// （自然推过零点）——按计划剩余封顶两种情况都成立；误差翻号后
/// 由 step 的 in_tol 退出检查和修正段接手。
fn flick_axis(cover: f64, done: f64, shape: f64, dur: f64, max_spd: f64, dt: f64) -> f64 {
    let rem_cover = cover - done; // 带符号的计划剩余
    if rem_cover.abs() < 1e-9 {
        return 0.0;
    }
    let v_cmd = (shape * cover.abs() / dur).min(max_spd) * dt;
    rem_cover.signum() * v_cmd.min(rem_cover.abs())
}

/// 目标筛选：锁定目标在 `fov*1.5` 滞回带内保持，超出（含目标转到
/// 屏幕外/背后）丢锁重选；否则取 FOV 内距准星最近者。
/// `cands` = (id, dx, dy)；返回 (id, dx, dy)。
pub fn pick_target(
    cands: &[(u64, f64, f64)],
    locked: Option<u64>,
    fov: f64,
) -> Option<(u64, f64, f64)> {
    if let Some(lk) = locked
        && let Some(&c) = cands.iter().find(|&&(o, ..)| o == lk)
        && c.1.hypot(c.2) < fov * 1.5
    {
        return Some(c);
    }
    cands
        .iter()
        .copied()
        .filter(|&(_, dx, dy)| dx.hypot(dy) < fov)
        .min_by(|a, b| a.1.hypot(a.2).partial_cmp(&b.1.hypot(b.2)).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f64 = 1.0 / 125.0;

    fn drive(h: &mut Humanizer, cfg: &AimTune, dx: f64, dy: f64, frames: u32) -> (f64, f64) {
        let (mut dx, mut dy) = (dx, dy);
        let mut now = 0.0;
        for _ in 0..frames {
            let (ix, iy) = h.step(dx, dy, DT, 10.0, cfg, now);
            dx -= ix as f64;
            dy -= iy as f64;
            now += DT;
        }
        (dx, dy)
    }

    /// 固定目标：一段时窗后误差应收敛到容差附近（欠冲由修正段补）。
    #[test]
    fn step_should_converge_on_static_target() {
        let mut h = Humanizer::new();
        let cfg = AimTune::default();
        h.retarget(0.0, &cfg);
        let (dx, dy) = drive(&mut h, &cfg, 300.0, -200.0, 600);
        assert!(dx.hypot(dy) < 30.0, "residual={dx},{dy}");
    }

    /// 反应延迟：retarget 后 ~120ms 内不应产生大位移。
    #[test]
    fn retarget_should_enforce_reaction_delay() {
        let mut h = Humanizer::new();
        let cfg = AimTune::default();
        h.retarget(0.0, &cfg);
        let mut moved = 0.0;
        for i in 0..15 {
            // 前 15 帧（120ms）内累计位移应远小于一次拉枪幅度。
            let (ix, iy) = h.step(300.0, 0.0, DT, 10.0, &cfg, i as f64 * DT);
            moved += (ix as f64).hypot(iy as f64);
        }
        assert!(moved < 80.0, "reaction delay window moved {moved}");
    }

    /// Fitts 定时：距离越大耗时越长；容差越大耗时越短。
    #[test]
    fn fitts_duration_should_scale_with_distance_and_tolerance() {
        let mut h = Humanizer::new();
        let cfg = AimTune::default();
        h.retarget(0.0, &cfg);
        h.step(400.0, 0.0, DT, 10.0, &cfg, 1.0); // 推过 React
        let Phase::Flick(far) = h.phase else {
            panic!("expected Flick, got {:?}", h.phase)
        };
        h.retarget(0.0, &cfg);
        h.step(100.0, 0.0, DT, 10.0, &cfg, 1.0);
        let Phase::Flick(near) = h.phase else {
            panic!("expected Flick")
        };
        assert!(far.dur > near.dur, "far={} near={}", far.dur, near.dur);
        h.retarget(0.0, &cfg);
        h.step(400.0, 0.0, DT, 50.0, &cfg, 1.0);
        let Phase::Flick(wide) = h.phase else {
            panic!("expected Flick")
        };
        assert!(far.dur > wide.dur, "far={} wide={}", far.dur, wide.dur);
    }

    /// 亚像素余量：微小恒速输入也应累积成整数步进，不丢步。
    #[test]
    fn step_should_accumulate_subpixel() {
        let mut h = Humanizer::new();
        let cfg = AimTune {
            ou_sigma: 0.0,
            emit_holdover_p: 0.0,
            ..AimTune::default()
        };
        h.phase = Phase::Hold;
        let mut total = 0i32;
        let mut now = 0.0;
        for _ in 0..200 {
            // dx=11 超出逐轴死区（w=10）才有追踪步进；每帧 ~0.5 count
            // 亚像素，靠 RemainMouse 累积成整数。
            let (ix, _) = h.step(11.0, 0.0, DT, 10.0, &cfg, now);
            total += ix;
            now += DT;
        }
        assert!(total > 0, "subpixel steps must accumulate");
    }

    /// 扳机门控：只在保持态+容差内+冷却过后开火。
    #[test]
    fn should_click_gates() {
        let mut h = Humanizer::new();
        let cfg = AimTune::default();
        assert!(!h.should_click(1.0, 1.0, 10.0, &cfg, 0.0)); // Idle 不开
        h.phase = Phase::Hold;
        assert!(h.should_click(2.0, 1.0, 10.0, &cfg, 1.0));
        assert!(!h.should_click(11.0, 0.0, 10.0, &cfg, 1.0)); // 超容差
        h.click_hold_secs(1.0);
        assert!(!h.should_click(2.0, 1.0, 10.0, &cfg, 1.05)); // 冷却中
        assert!(h.should_click(2.0, 1.0, 10.0, &cfg, 1.3)); // 冷却过
        h.phase = Phase::Correct { since: 0.0 };
        assert!(!h.should_click(2.0, 1.0, 10.0, &cfg, 2.0)); // 非保持态
    }

    /// 失误率：大样本下过冲拉枪占比应接近 miss_p（容忍统计波动）。
    #[test]
    fn overshoot_rate_should_match_miss_p() {
        let cfg = AimTune::default();
        let mut over = 0u32;
        const N: u32 = 400;
        for i in 0..N {
            let mut h = Humanizer::new();
            h.seed = h.seed.wrapping_add(u64::from(i) * 0x9E37_79B9);
            h.plan_flick(300.0, 0.0, 10.0, &cfg, 0.0);
            if let Phase::Flick(f) = h.phase
                && f.cover_x > 300.0
            {
                over += 1;
            }
        }
        let rate = f64::from(over) / f64::from(N);
        assert!(
            (rate - cfg.miss_p).abs() < 0.06,
            "overshoot rate {rate} vs {}",
            cfg.miss_p
        );
    }

    /// 粗帧率回归：34fps 下 React/Pause/Hold 的单帧 tremor 输出
    /// 应有界（旧速度积分模型实测 ±17→"屏幕一直抖"）。
    /// 位置偏移模型把徘徊钉在 ~±3σ≈9 counts 总幅度内。
    #[test]
    fn tremor_should_stay_bounded_at_low_fps() {
        let cfg = AimTune::default();
        let mut h = Humanizer::new();
        h.retarget(0.0, &cfg);
        let dt = 1.0 / 34.0;
        let mut max_out = 0i32;
        let mut now = 0.0;
        for _ in 0..68 {
            // 2 秒都在反应延迟/早期阶段（远目标，React ~0.2s 后 Flick 的
            // 首帧还没到大输出段也算在内：只统计 ph=React 的输出）。
            if h.phase_name() == "React" {
                let (ix, iy) = h.step(500.0, 300.0, dt, 10.0, &cfg, now);
                max_out = max_out.max(ix.abs()).max(iy.abs());
            } else {
                h.step(500.0, 300.0, dt, 10.0, &cfg, now);
            }
            now += dt;
        }
        assert!(
            max_out <= 12,
            "34fps React 单帧输出应有界, max={max_out}"
        );
    }

    /// 复现：倒计时期 sim_time 冻结（now 恒 0）+ spd=2.0，
    /// React 卡死期间输出应仍≈0（诊断真实日志 ±60 的来源）。
    #[test]
    fn repro_frozen_now() {
        let cfg = AimTune { spd_mul_x: 2.0, spd_mul_y: 2.0, ..Default::default() };
        let mut h = Humanizer::new();
        h.retarget(0.0, &cfg);
        let (mut dx, mut dy) = (-110.0f64, 24.0f64);
        let mut max_out = 0i32;
        for _ in 0..40 {
            let (ix, iy) = h.step(dx, dy, 0.029, 13.3, &cfg, 0.0);
            dx -= ix as f64;
            dy -= iy as f64;
            max_out = max_out.max(ix.abs()).max(iy.abs());
        }
        assert!(max_out <= 12, "frozen-now React out={max_out}");
    }

    /// 确定性：同种子同输入 → 同输出序列。
    #[test]
    fn step_should_be_deterministic_per_seed() {
        let cfg = AimTune::default();
        let mut a = Humanizer::new();
        let mut b = Humanizer::new();
        a.retarget(0.0, &cfg);
        b.retarget(0.0, &cfg);
        for i in 0..200 {
            let now = i as f64 * DT;
            let sa = a.step(200.0, 100.0, DT, 10.0, &cfg, now);
            let sb = b.step(200.0, 100.0, DT, 10.0, &cfg, now);
            assert_eq!(sa, sb);
        }
    }

    /// 目标筛选：锁定目标在滞回带内优先，出带丢锁；FOV 内选最近。
    #[test]
    fn pick_target_prefers_locked_then_nearest() {
        let cands = [(1u64, 10.0, 0.0), (2, 700.0, 0.0), (3, 50.0, 0.0)];
        assert_eq!(pick_target(&cands, Some(2), 600.0).unwrap().0, 2);
        assert_eq!(pick_target(&cands, None, 600.0).unwrap().0, 1);
        assert!(pick_target(&cands, Some(9), 5.0).is_none()); // 锁死+FOV 内无人 → None
        // 锁定目标飞出滞回带（>fov*1.5）→ 丢锁重选 FOV 内最近者。
        assert_eq!(pick_target(&cands, Some(2), 400.0).unwrap().0, 1);
        // 出带后 FOV 内也无人 → None（不会去追屏幕外的目标）。
        assert!(pick_target(&cands, Some(2), 8.0).is_none());
    }
}
