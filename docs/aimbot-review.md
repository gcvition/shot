# 自瞄模块 10 轮 Review 记录（v2·文档架构版）

对象：`src/aimbot/`（`humanizer.rs` 按 `rain/docs/design/aimbot.md` §12
终态实现、`system.rs` 接线）+ `Settings.aimbot_*` + `shell` 设置 UI。
（v1 为 `aim.rs` v0 直接移植，已被本版取代；本记录覆盖 v2。）

每轮一个视角；「修复」指已落地到代码，「记录」指确认的已知限制。

## R1 状态机正确性

- **[修复] React/Pause 分支不可达**：初版 match 顶部合并分支
  `Idle|React|Pause => {}` 抢先捕获，后面的转移分支永不执行 →
  状态机卡死在 React。改为每分支显式处理。
- **[修复] `est_lead` 占位引用不存在字段**：补 `last_now` 缓存，
  Flick→剩余时长、Correct→`min(1/corr_k,0.3)`、Hold→0.05、其余 0。
- **[确认] 转移时序**：React 到期当帧 `plan_flick`、τ=0 时
  `shape=0` 无丢失步进；Flick→Pause 判定用 post-step `done` 与
  pre-step `err`，语义一致。

## R2 数值/边界

- **[修复] `flick_axis` cap 逻辑收敛**：欠冲计划 `rem_cover<err`
  （绝不越零点）、过冲计划 `rem_cover>err`（自然推过）——统一
  `v_cmd.min(|rem_cover|)`，删掉冗余的 overshot 分支。
- **[修复] `dt` 防护**：`!(dt>0)` 改 `!(dt.is_finite()&&dt>0)`
  （兼防 NaN + clippy `neg_cmp_op_on_partial_ord`）；`dt.min(0.1)`
  防卡顿帧包络跳步。
- **[确认] OU 稳定性**：θ=60、dt≤0.1 → |1−θdt| 恒 <1 不发散；
  稳态 std≈1.0 count（tremor 段乘 dt·60 输出 ~±0.5 帧幅）。
- **[确认] `gauss`** Box-Muller u1∈(0,1]、ln 域正确。

## R3 时序语义

- **[确认] countdown 冻结**：sim_time 冻结时 React/Pause 不计时
  ——反应延迟从比赛时间起算，语义正确。
- **[修复] holdover 节拍无抖动**：挂起路径 `emit_in` 也按
  `0.7+0.6·rand` 采样，与正常发送路径一致。
- **[记录] 帧粒度下的节拍**：`emit_mean=3ms` 低于 8ms 帧间隔，
  必然每帧到点；抖动实际通过 10% holdover 体现——宿主约束，
  真实 2–4ms 节拍需独立调度线程（df 阶段再议）。

## R4 系统层（system.rs）

- **[修复] 借用冲突 ×3**：`ctl.hum.*(…&ctl.tune)` 双借 `ResMut`；
  `AimTune:Copy` 拷局部 `tune`。
- **[修复] 持键激活语义**：`launch.aimbot`(武装) ∧
  `mouse.pressed(aimbot_key)`；松开即 `hum.stop()`+清空历史
  （§13 StopAiming）。左键释放判定置顶，先于 active 门。
- **[修复] ADS `deg_eff` 镜像**：激活键默认右键=ADS 键，
  `apply_mouse_look` 会按 `ads.sens_mult` 改每 count 度数——
  误差换算必须同公式，否则绑右键时系统性瞄偏。
- **[确认] 起算同源**：`body.eye()` + `look+acc_*`（fire 口径）。

## R5 与 aimbot.md 对照

- **[确认] §12 流水线全段**：React N(190,40)min120 → Fitts
  (a+b·log2(D/W+1)，b 逐拉枪 ±15% 采样) → minimum-jerk
  `30τ²(1−τ)²` 主运动（X/Y 分轴上限 1800/1200、同 duration)
  → 停顿 U(30,80)ms → P 修正（超时 0.6s) → Hold(OU tremor+跟枪 P)
  → 扳机=保持态残余≤W。
- **[确认] §11 差异化**：无 AFG/NEXO 魔数；逐拉枪参数采样；
  xorshift 会话种子。
- **[确认] §6.2 lead**：位置历史差分（0.12s 窗）+ est_lead
  外推，感知滞后是特性。
- **[记录] 未做项**：疲劳漂移（需会话时长输入）、目标粘滞
  （Hold 用 P 追踪偏紧，比"大概瞄着"更准——可接受偏差）、
  选点仍"最近"未加容差内随机偏好。

## R6 确定性

- **[确认] 全链路可复现**：所有随机量走定种子 xorshift；
  `now`=sim_time、`dt`=Time；105 测含 `deterministic_per_seed`
  断言；关闭时 golden digest 不变。

## R7 测试质量

- **[修复] e2e 适配持键**：`run_frames` 增 `hold_aim_key` 参数，
  按住 `MouseButton::Right`（默认绑定）跑完全场。
- **[新增]** `aimbot_without_aim_key_should_do_nothing`：武装但
  未持键 → 零输入。
- **[新增单测]** Fitts 随距离/容差单调、反应延迟窗口、失误率
  统计（400 样本 ±0.06）、确定性、扳机门控（含非 Hold 态）。
- **[记录] 缺口**：Neutral-only 场景、headshot_only、Beam/弹丸
  武器未覆盖。

## R8 API/文档一致性

- **[确认]** `Settings.aimbot_enabled`(serde default)+
  `AimbotButton`(Right 默认，Left 不可绑）；菜单开关持久化走
  既有 SaveSettings 流。
- **[确认]** 模块头注释与实现一致（流水线阶段、单位、刻意差异）。
- **[修复]** `est_lead`/`step` 签名补 `w`（§14.5 三元组的 W）。

## R9 与其他系统交互

- **[确认]** ADS：绑右键时 aimbot 与 ads_zoom 共存，deg_eff
  一致换算；非 ADS 场景无影响。
- **[确认]** RecoilNegatable：合成 MouseMotion 仍走 acc 抵消。
- **[确认]** FireGate 无死锁；countdown 只挡开火不挡瞄准（与人同）。
- **[记录]** Auto/Burst 武器 20–45ms 按住可多发；回放模式
  `is_play` 门已挡；课程换局 sim_time 回跳由时间回跳松键覆盖。

## R10 终审通读

- **[确认]** `pending`/`emit_in`/`ou_*`/`hist` 在 stop/retarget
  均清零；`holding_until` 三重保险（到点/回跳/1s 上限）。
- **[记录] 已知限制**：w 第二遍 O(n) 查找（n 小不优化）；Hold 段
  est_lead 与 Correct 段不连续（0.05 vs 0.1，跳变可接受）；
  Beam 需持续按住追踪未适配；headshot_only 瞄中心不瞄头。

## 结论

10 轮共修复 7 处（含 1 处状态机卡死级）、确认 14 处、记录 8 条
已知限制。`cargo test --lib` 105 全绿（5 e2e 持键语义 + 6 状态机
单测），clippy 无新增告警，golden digest 不变。

**E2E 整场**:wall 场景 20s/2499 帧，15/15 命中 = 100% 命中率、
TTK ≈0.8–1.3s/靶（反应延迟+两段运动的人类节奏）。

---

# 实战修复记录（用户实测反馈版）

实测症状：按住激活键屏幕持续抖动、偶发"天旋地转"、整靶节奏
比手动慢。逐条定位并修复：

## F1 `w` 单位错配（根因，`system.rs`）

`asin(r/len)` 返回**弧度**却直接除以 `deg_eff`（度/count），
w 比真实值小 ~57× 恒被钳到 1 count ≈0.06°。连锁后果：

- `should_click` 要求 |err|≤1 count → 几乎不开火 →「比手动慢」；
- `reflick` 阈值 `w×4`≈4 counts → Hold 每帧重甩 →「按住就抖」；
- Fitts `log2(D/W)` 中 W=1 → 拉枪时长膨胀 → 更慢。

**修复**：改 `atan(r/len).to_degrees()/deg_eff`（atan 免 clamp），
抽 `hit_tolerance_counts` 单测钉住（0.3m@10m → ~28.7 counts）。

## F2 锁定无 FOV 边界（"天旋地转"主因）

`pick_target` 锁定保持不查距离——目标转到屏幕外/背后，瞄准仍
满速抡过去。**修复**：滞回语义，锁定目标在 `fov×1.5` 内才保持，
出带丢锁重选 FOV 内最近者；FOV 内无人则停。

## F3 速度估计无钳制

Blink/瞬移目标的 `RenderPos` 差分出离谱速度，`lead` 乘出来把
瞄准点甩飞。**修复**：`cap_vel` 钳到 25 m/s。

## F4 Hold 死区（tremor-控制器自激）

保持态 P 跟枪会把自身 OU tremor 造成的偏移当误差拉回 → 抖动
循环。**修复**：逐轴死区——|err|≤w 不修（人也一样，亚容差偏差
不动腕），同时更符合 Fitts 语义。

## F5 扳机口径

Hold 提前量让准星跑在目标前面，hitscan 却打目标当前位置；
加上容差边缘 + tremor/帧延迟 → 擦边 miss。**修复**：扳机判定
改用无提前量原始误差（`step` 仍用带 lead 误差驱动运动），
容差收紧 `w×0.8` 留余量。E2E 命中率 57%→88%→96%。

## F6 可调用途参数（新增设置项）

- `aimbot_fov_deg`（5–90°，默认 36）：索敌角半径，运行期按
  `deg_eff` 换算 counts；HUD 层画角半径一致的椭圆圈
  （`aimbot_fov_ring` system + `ring_mesh2d` 单位环，ADS 变焦
  同步缩放），`aimbot_fov_visible` 开关。
- `aimbot_speed_x/y`（0.2–3.0，默认 1.0）：乘在 X/Y 轴限速与
  增益上，Fitts 时长按两轴均值反比缩（保持同 duration 自然弧）。
- 默认参数提速：fitts_a 0.15→0.10、fitts_b 0.10→0.09、
  max_spd 1800/1200→3000/2000 counts/s（原 caps 会截断大包络
  峰值，拉枪更肉）。

## 回归

`cargo test --lib` **107 全绿**（新增 `hit_tolerance_counts`/
`cap_vel`/滞回丢锁 3 处断言）；golden digest `8013938259272503606`
不变；clippy 无新增告警。

**E2E 整场**（新默认参数）：24/25 = **96% 命中**（v2 为 15/15=100%
但只有 15 次出手；本版出手更快且保留拟人误差），TTK 明显缩短。
