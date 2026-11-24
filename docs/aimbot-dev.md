# 自瞄模块开发记录（aimbot-dev）

> 目的：记录 shot 自瞄测试床的架构决策、优化历史和排障方法。
> 读者：本项目的后续开发/AI agent。配合 `docs/aimbot-review.md`（10 轮 review 记录）和
> `cache/logs/aimbot-*.log`（运行日志）食用。
>
> 上游设计依据：Rain 项目 `docs/aimbot.md`（拟人化自瞄的六段流水线目标架构）。
> shot 定位：**无 MAKCU 无 DMA 硬件时，验证自瞄算法的沙盒**——算法本身即交付物。

## 1. 架构总览

```
持键(aimbot_key, 默认右键)
  → aimbot_drive (PlaySet::Aimbot)
      ├─ 目标选择 pick_target：FOV 内最近 + 锁定滞回(fov×1.5 内保持)
      ├─ 部位选点 aim_part：Humanoid→最上方 OffsetSphere(头) | Body→胶囊 | Center→RenderPos
      ├─ 速度估计 estimate_vel：位置历史差分, 0.12s 感知滞后, cap_vel=25m/s
      ├─ 提前量 lead：intercept = pos + vel×est_lead（开火判定用无 lead 原始误差）
      ├─ Humanizer::step → (dx,dy) counts
      ├─ MouseMotion 消息 → apply_mouse_look（真实输入路径, 录 .shot 轨迹）
      └─ should_click → ButtonInput<MouseButton>::press(Left)（真实开火路径）
```

Humanizer 内部状态机（aimbot.md §12 终态，双子运动模型）：

```
Idle → React(N(190,40)ms, 下限120ms)
     → Flick(minimum-jerk 包络 30τ²(1-τ)², Fitts 定时 MT=a+b·log2(1+D/W),
             X/Y 分轴限速 3000/2000 counts/s × speed 倍率,
             15% 失误率注入 1.2–1.6× 过冲)
     → Pause(U(30,80)ms) → Correct(P 控制)
     → Hold(OU tremor + 逐轴死区 + 跟枪 P + reflick 阈值 w×4)
     → 扳机：残差 ≤ w×0.8（无 lead 误差）
```

关键文件：

| 文件 | 职责 |
|---|---|
| `src/aimbot/humanizer.rs` | 状态机/拟人化纯逻辑，无 Bevy 依赖，全量单测 |
| `src/aimbot/system.rs` | Bevy 系统层：选目标/部位、误差换算、开火门控、日志接线 |
| `src/aimbot/log.rs` | 诊断日志（见 §4） |
| `src/settings.rs` | `aimbot_*` 字段 + `AimbotPart`/`AimbotButton` 枚举 |
| `src/shell/` | 设置页「自瞄」区 UI（intent/model/update/view/widgets） |
| `src/viewport/gfx.rs` `ring_mesh` + `fov_ring_update` | FOV 圈 HUD 网格 |

## 2. 设置项速查（设置页「自瞄」区）

| 设置 | 字段 | 默认 | 说明 |
|---|---|---|---|
| 启用自瞄 | `aimbot` | off | 总开关；也可 `--aimbot` / `SHOT_AIMBOT=1` 武装 |
| 激活键 | `aimbot_key` | 鼠标右键 | 按住生效；左键不可选（与开火冲突）。ADS 占用右键时建议改中/侧键 |
| 索敌 FOV | `aimbot_fov_deg` | 36° | 只有锥内目标可选；5–90° |
| 显示 FOV 圈 | `aimbot_fov_visible` | on | 准星周围椭圆圈，ADS 变焦自动缩放 |
| 水平速度 | `aimbot_speed_x` | 1.0 | 0.2–3.0×，乘限速/增益/Fitts 时长 |
| 垂直速度 | `aimbot_speed_y` | 1.0 | 同上 |
| 瞄准部位 | `aimbot_part` | Head | Head=最上方 OffsetSphere（人形头）；非人形回退中心。w 按所选部位半径算 |
| 自瞄算法 | `aimbot_algo` | Rain | Rain/AFG/NEXO/Nep 四选一（见 §3 v4） |

## 3. 迭代历史与教训

### v0 — Rain `aim.rs` 移植（已废弃）
弹簧-速度模型 + 简单抖动。问题：非文档终态，参数拍脑袋。

### v1 — aimbot.md 终态重写
双子运动状态机（React→Flick→Pause→Correct→Hold）。**实测灾难：一直抖、天旋地转、比手动慢。**

### v2 — 根因修复（本次实测反馈驱动）
| 症状 | 根因 | 修复 |
|---|---|---|
| 一直抖 | `w` 单位错：`asin(r/len)` 弧度没转度就除以 deg/count，容差恒钳为 1 count≈0.06° → Hold 每帧 reflick、几乎不开火、Fitts 时长膨胀 | `atan().to_degrees()/deg_eff`，单测钉住 |
| 天旋地转 | 锁定目标无 FOV 边界，转到背后也满速追 | 滞回 `fov×1.5` 出带丢锁 |
| 甩飞 | Blink/瞬移目标差分出离谱速度，lead 无上限 | `cap_vel` 25 m/s |
| 高频抖动 | OU tremor 推开的偏移被跟枪 P 当误差拉回，自激振荡 | Hold 逐轴死区（亚容差不修） |
| 边缘 miss | 扳机用带 lead 的误差 + 满 w | 扳机用无 lead 原始误差，`w×0.8` |
| 太慢 | 上面 w bug + 保守参数 | `fitts_a` 0.15→0.10、`max_spd` 3000/2000 |

**教训**：
- **单位换算必须进单测**。`w` 一个 `.to_degrees()` 之差让整个状态机全部参数失效——上层"抖动/慢"的症状其实是底层换算 bug。以后再改角度↔count↔弧度换算，先写断言。
- **提前量只喂运动、不喂扳机**。运动要领先于目标，开火射线打的是当前帧 RenderPos——两套误差要分开算。
- **锁定必须有出带**。无界锁定 = 视角被拽着跑，任何"天旋地转"先查丢锁条件。

### v3 — 部位选点 + 诊断日志 + 设置项扩展
- `aim_part`：人形默认头；`w` 按所选部位半径（头 0.16m → 容差自动更严）
- `AimbotLog`：逐帧 + 事件 + ScoreState 差分
- FOV/速度/显隐进设置页

### v4 — 日志驱动的抖动根因修复 + 四算法可切换（当前）

用户实测两局仍"屏幕很抖"，`aimbot-Sixshot_Ultimate-*.log` 量化后定位：

| 日志证据 | 根因 | 修复 |
|---|---|---|
| `ph=React` 段 `out=` ±5~17 随机方向，Hold 段 `out=-16,15`→`5,-4` 来回 | **OU tremor 是速度噪声积分**(`mx += ou×dt×60`)——34fps 单帧踢 ±17 counts 且随机游走可漂移 | tremor 改**有界位置偏移模型**：ou 状态当瞄准点偏移，逐帧发增量 `off−off_prev`；总徘徊幅度钉在 ±3σ≈9 counts，永不漂移；幅度再封顶 `w×0.25` 防远目标把误差顶出 `w×0.8` 开火门 |
| `out=0` 与大值交替（holdover 挂起帧） | `emit_holdover_p=0.10` 在 dt(29ms)≫emit_mean(3ms) 时变成 0/2× 交替 | 挂起概率按 `emit_mean/dt` 衰减，粗帧自动失效 |
| Correct 段单帧 out 到 43 | 修正限速 0.5×max_spd 在 34fps 单帧 43 counts 步进 | 修正 0.2×、跟枪 0.15× |

**教训**：噪声模型的单位/尺度假设必须随宿主帧粒度验证——
`dt×60` 隐式假设 60fps+，34fps 下每帧踢幅放大 2 倍且无法收敛。
`tremor_should_stay_bounded_at_low_fps` 单测钉住 React 单帧 ≤12。

#### v4.1 — 量化抖感指标：方向反转率

"还抖"复核：`aimbot_wall_at_34fps_should_report_jitter_metrics`
（29ms 手动帧 = 用户实测机帧率）给出决定性指标——**相邻非零帧
方向反转率 49%**（人手移动方向 ~50ms 内连续，反号率应 <15%）。

根因：OU θ=60 的相关时间 16ms **小于 34fps 帧时间 29ms** →
tremor 逐帧退化成白噪声，增量每帧反号 → 最大抖感（有界也抖）。

修复：θ 60→8（相关 ~125ms ≈ 4 帧同向，呈缓慢手漂）、
σ 33→10（稳态 std 3→2.5 counts）、React 期 tremor 0.5→0.3、
Correct 0.3×/Hold 0.2× 限速回调。效果：flips **49%→2.6%**,
命中 18/1 保持，整场 93.1%。

**教训**：相关时间是 tremor 是否"抖"的第一参数——θ 与帧时间
同量级以下退化成白噪声；θ 应显著大于 dt 才有"手漂"质感。

#### v4.2 — 真机实跑复核（SendInput 真实按住激活键 + 日志分解）

**方法**：`cache/testaim.ps1` 启动 `shot.exe play --aimbot`，
`mouse_event(RIGHTDOWN)` 真实按住激活键 15s、周期截屏、松键杀
进程——走完整输入路径（ButtonInput→armed→step→MouseMotion→
apply_mouse_look），不是 headless 替身。

发现与修复：

1. **旧二进制假象**：旧日志 React 期 out=±60 交替——`repro_frozen_now`
   复现证明当前源码发不出；那次日志跑的是未含 v4 修复的旧 exe。
   **教训：分析日志先核 `START` 行参数 + exe mtime。**
2. **Correct/Hold 超调振荡**（真 bug）：`corr_k·spd·dt`/`track_k·spd·dt`
   在卡顿帧（dt>50/83ms@spd=2）增益>1 → 每帧反向超调。
   修复：步长按 `0.6×|err|` 逐轴封顶——任何 dt/spd 下闭环收敛、
   单帧不瞬移（实测 Correct 单帧 98→收敛到 ≤0.6err）。
3. **`apply_mouse_look` 倒计时分支漏 ADS 倍率**（真 bug）：倒计时内
   用裸 `deg.0`，自瞄按 `deg_eff` 计的输出被放大 `1/sens_mult` 倍。
   已镜像正常路径的 `deg_eff` 公式。
4. **倒计时不激活自瞄**：sim_time 冻结期 now 恒 0，状态机卡死
   React——`clock.countdown>0` 现在视为未激活。
5. **F 行分解字段**：`stp=`（阶段步长）`trm=`（tremor 增量）
   `pnd=`（暂存）`dt=`——输出构成一眼可辨，排障不用再猜。

实跑指标（34fps, spd=2.0, Rain）：**28~29 HIT / 0~1 MISS**；
React |out|≤2；Hold flips 仅 tremor 幅度（≤12 counts≈0.4°)。

#### v4.3 — Rain vs 匀速系观感对齐（用户四算法对比反馈）

用户实测四算法后反馈 Rain"最不丝滑 + 准星拉过头"。根因是设计性
特征在训练场语境下读成了缺陷：

| 观感缺陷 | 设计源 | 调整 |
|---|---|---|
| 准星拉过头 | 失误注入：15% 甩枪计划覆盖 1.2–1.6×err | miss_p 0.15→0.06、over 1.2–1.6→1.10–1.30；且 in_tol 提前截断，过冲计划多数不再越过容差边 |
| 一顿一顿 | Flick→Pause(30–80ms 死帧)→Correct 相位链 | in_tol 直接进 Hold 跳两段；Pause 收窄 20–50ms |
| 修正段明显 | 欠冲带 0.88–0.95 → 近半甩枪进 Correct | cover 0.92–0.98，落点率↑ |

效果（spd=3.0, Sixshot）：**35 HIT/0 MISS, TTK p50 0.45s**
（匀速系 ~0.14s——190ms 反应延迟+Fitts 时长是拟人化税，结构差）。

**教训**：aimbot.md 的失误模型/停顿是反检测需求（df/行为分析），
在无检测压力的训练场里每 7 次甩枪一次过冲 = 纯观感缺陷。
同一份拟人化设计在不同语境要分档——Rain 侧部署时可恢复大值。

#### v4.4 — 跟枪场景优化（Smooth Strafe Sphere 实测驱动）

跟枪日志签名：`ph=Hold` 段 `dx` 在 36↔65 间锯齿、`out` 每 ~2 帧
踢 30 其余为 0——**Hold 死区（dx.abs()>w 才修）+ 纯位置 P 跟
移动目标是 0 型系统，稳态必然滞后，死区内准星原地不动 = 阶梯
追踪**。

修复——速度前馈（控制论标准的平滑追踪方案）：

- step() 内部 EMA 估计目标角速度：`tgt_v = dx - last_dx + last_out`
  按 dt 归一，α=0.2（~150ms 时间常数，变向有真实反应滞后）
- Hold 段每帧输出 `tgt_v × dt` 连续跟速 + 原死区 P 修残余；
  静态目标 EMA≈0 → 前馈自动失效，静态行为不变
- 节拍门微帧修复：winit 偶发 dt≈3ms 微帧骑在 `emit_mean` 边界
  随机开关 → 跟速被拆成 0/满额脉冲；`dt >= emit_mean` 时直通
- `log.rs` FLUSH_EVERY 240→30（强杀下最多丢 1s 数据）

实测（34fps, spd=3.0）:Hold |dx| p50 **46 → 1.8 counts**,87 HIT /
0 MISS，追踪全程连续小步（out p50≈1-3)——准星黏住目标；
Sixshot 静态回归 **34 HIT / 1 MISS** 无变化。

#### v4.5 — Rain 参数全面设置化 + 自定义 tune 诊断

- `AimTune` 加 serde（`#[serde(default)]` 半旧配置自动回填）,
  `settings.aimbot_tune` 持久化；`RAIN_PARAMS` 描述表（label/tip/
  min/max/step/decimals/get/set）是设置页、UI 校验、运行时
  `clamp_ranges()` 的唯一事实源——`fov`/`spd_mul` 三项不表列
  （已有独立设置项）。
- 设置页算法选 Rain 时出现「Rain 参数」区：25 项滑块+数字框，
  参数名悬停出说明（原生 `.tooltip` 经 `Field::label_fn` 挂
  Stateful div);「恢复默认」一键回填并同步全部控件。
- `NumField::RainTune(ix)` 复用 EditNumber/SlideAim 路径；
  `system.rs` 每帧从 `settings.aimbot_tune` 构建 tune 并
  `clamp_ranges()`（手改 json 兜底，成对 lo>hi 交换）。
- 日志 START 行加 `tune=default|custom react=.. fitts=.. corr=..`：
  分析日志先确认跑的是不是调过的参数。
- 实测：settings.json 写 `react_mean=0.25` → START 行出现
  `tune=custom react=0.250`,31 HIT/0 MISS——链路通。

#### 算法切换（`aimbot_algo`）

`system.rs` 外层（锁管理/速度估计/提前量/FOV 滞回/开火差分/日志）
四算法共用；只有运动核心分发：

| `AimAlgo` | 实现 | 运动模型 |
|---|---|---|
| `Rain`（默认） | `humanizer.rs` | 双子运动状态机（本仓自研，见 §1） |
| `Afg` | `algo.rs::ConstAim` | 归一化方向 × 定步长 `max_spd×0.45×spd_mul` counts/s，纯匀速无包络 |
| `Nexo` | 同上 | 同骨架 + 三次贝塞尔速度包络 800ms 0→100%，曲线 `{0.25,0.1,0.25,1.0}` |
| `Nep` | 同上 | 同骨架 + 随机速度游走：每 150ms 抽 ±20% 新乘数、新旧插值 |

匀速系扳机 = 误差 ≤ `w×0.8`（收敛到点即开，无保持态）；提前量
= 剩余距离/当前速度；`click_hold_secs`/`click_cd` 与 Rain 同分布。
源实现锚点写在 `algo.rs` 头注释（AFG `AimBotAPI3`、NEXO `AimBotAPI`、
Nep `FUN_140045c00` 段）。

## 4. 日志排障指南

文件：`cache/logs/aimbot-<场景>-<会话8>.log`（仅真实 App 写；headless 测试无 `AppPathsRes` 静默跳过）。

行格式：

```
START t=0.000 scen=wall sess=abc12345 key=Right algo=Rain fov=36.0 spd=1.00x/1.00y part=Head deg=0.060000
LOCK  t=0.412 id=3 dist=142.3               # 锁新目标
F t=0.608 ph=Flick id=3 dx=142.3 dy=-20.1 w=28.7 out=18,-4 yaw=12.34 pit=-2.10
FIRE  t=1.020 id=3 err=-1.2,0.4 w=28.7 hold=33ms
HIT   t=1.041 hits=5 misses=2               # ScoreState 差分（另有 MISS 行）
UNLOCK t=1.050 id=3 why=dead_or_out_of_fov  # 另有 switch/key_released/match_end
END   t=20.000 hits=24 misses=1 frames=1240
```

`F` 行是持键期间逐帧写：`ph`=阶段（Rain 出状态机名
Idle/React/Flick/Pause/Correct/Hold；匀速系出 Afg/Nexo/Nep）、
`dx/dy`=带提前量的误差 counts、`w`=命中容差 counts、`out`=本帧输出 count、
`yaw/pit`=当前视角。

症状 → 日志特征 → 怀疑对象：

| 症状 | 日志特征 | 查什么 |
|---|---|---|
| 屏幕抖（徘徊型） | 静止段 `out=` 恒非零、方向随机、量级 >5 | tremor 模型：位置偏移，单帧 \|out\| ≤12@34fps；超了查 `ou_sigma`/`w×0.25` 封顶 |
| 屏幕抖（反号型） | 相邻 F 行 `out` 符号频繁反转（>15%） | OU θ 是否 ≫1/dt：θ×dt≪1 时逐帧白噪声=v4.1 的坑 |
| 屏幕抖（交替型） | `out=0` 与大值交替出现 | holdover 是否没按 emit_mean/dt 衰减 |
| 屏幕抖（甩枪型） | `ph=Flick`/`Correct` 高频交替，`out=` 方向来回 | `w` 是否过小（F 行 w≈1 就是单位 bug）；reflick 阈值 |
| 天旋地转 | `yaw=` 大幅摆动、LOCK/UNLOCK 交替、`dist=` 巨大 | 丢锁滞回；`cap_vel`；lead 是否爆炸（`dx/dy` 远大于 FOV） |
| 不开火 | 大量 `ph=Hold`（或匀速系长 hold）但无 FIRE | `err` 是否恒大于 `w`（容差太严或 tremor 太大） |
| 只瞄身体 | `part=Center` 出现在 START 行 | 设置页部位选项 |
| 命中低 | FIRE 后无 HIT、`misses` 涨 | `err` 是否贴 `w` 边缘开火（容差再收紧）；lead 过度 |
| 换算法没效果 | START 行 `algo=` 与预期不符 | 设置持久化；RadioGroup index 映射 |

## 5. 测试设施

- **单测** `cargo test --lib aimbot`：状态机收敛/确定性/Fitts/失误率/容差换算/速度钳/滞回丢锁。
- **e2e** `src/viewport/tests.rs::aimbot_e2e`：headless 全场模拟，走真 `MouseMotion`+`ButtonInput` 路径：
  - `wall` 静态靶 96%+ 命中率、`grid`、`sine` 移动靶、
  - `humanoid`（`headshot_only`）：证明默认瞄头 + Center 对比显著更少命中。
  - `aimbot_full_match` 整场跑满时长打印命中率（`--nocapture`）。
- **golden digest** `8013938259272503606`：自瞄路径不影响无自瞄回放。

## 6. 已知限制

- `Blink` 瞬移目标靠 cap_vel 兜底，锁定瞬移后会有 React 延迟——拟人特性，非 bug。
- FOV 圈是 HUD 层椭圆，不随目标遮挡变化（纯指示作用）。
- 无弹道下坠补偿——shot 是 hitscan；接 Rain/df 时弹道层另算（aimbot.md §14）。
- 反应延迟/失误率是"人类节奏"的有意开销，打分时 TTK 比纯最优解慢——这是设计目标不是缺陷。
