# Shot — 开发指南

桌面端 FPS 瞄准训练器。一个 exe 两个进程：GPUI Kit 菜单 + Bevy 0.19 训练/回放视口。
扩展计划见 `.devin` 之外的用户计划文件（里程碑 M0–M8）；当前处于 M0 收尾：
只重构，不改现有玩法行为。

## 构建 / 测试（Windows，cargo 在 scoop persist 里）

```bash
cargo.exe test --lib          # 单测，含 golden 确定性回归
cargo.exe clippy --all-targets
cargo.exe build               # shot.exe（dev：依赖 O3、自身 O1）
cargo.exe build --release     # 实际游玩用这个（thin LTO）
```

**别拿裸 `cargo run` 玩**——`profile.dev` 已把依赖开到 O3，自身 crate O1，
帧率够玩；但 `cargo build` 旧产物是无优化二进制，视口会卡、输入延迟高。
```

## 关键约束：golden digest

`src/viewport/tests.rs` 的 `golden::play_mode_should_match_golden_digest` 用固定
种子 + 输入脚本跑 headless App，哈希所有射击/出生/消失事件。当前 digest：
**`8013938259272503606`**（6 中 3 空，9 枪）。

- 重构时 digest 变了 = 行为变了，先查清楚是不是有意。
- 刻意改行为时更新 `GOLDEN_DIGEST` 常量并在提交说明里写原因。

## 结构（M0 之后）

- `src/viewport/` — Bevy 玩法，按职责分文件（mod.rs 文件头有表格）。
  新 system 放进 `PlaySet`（Gate→Look | Camera→Fire→Spawn→Clock→Replay→Hud→Present）。
  单 system 最多 16 参数，多的用 `#[derive(SystemParam)]` 打包（见 `SpawnCtx`）。
  - `director.rs`（M1）：`FixedUpdate` 240Hz `sim_targets` 推进 `TargetMotion`；
    `Update` 的 `sync_render_pos` 按 `Time<Fixed>::overstep_fraction` 插值出
    `RenderPos`——**命中判定读 `RenderPos`**（所见即所得）。
    `SpawnDirector` 资源管延迟重生 / 寿命 / max_alive / 最小间距 / 准星夹角。
  - 每个目标的运动 RNG 是 `seed ⊕ hash(id)` 派生流（`derive_rng`），
    与场景 RNG（出生点采样）分开——死亡顺序不影响彼此的随机序列。
- `src/sim/` — 无 Bevy 依赖的纯函数模拟层。`motion.rs` 12 个运动模型
  （Static/Linear/Strafe/Sine/Orbit/SmoothRandom/Dodge/Jumper/Blink/Path/
  Distance/Composite），确定性可单测。
- `src/sce/` — `.sce` 管线：document（分段）→ convert（归一化）→ ScenarioSpec。
  v1 场景走 `spec.legacy`；`[Shot Extension]` 键在 extension.rs。
  M1 的 spawn DSL 键（`ShotSpawnRegion[N]`/`ShotMotion[N]`/`ShotSpawnRule[N]`/
  `ShotSpawnCount[N]`/`ShotSpawnRadius[N]`/`ShotSpawnShape[N]`/`ShotNoShoot[N]`）
  在 `dsl.rs`；解析出至少一组时 `spec.legacy` 被清掉 → v2 导演路径。
  `res/scenarios/` 递归扫描，文件名 = 场景 id，重名给 warning。
- `src/session/` — `.shot` 轨迹：v2 结构 + postcard+zstd（`SHOT2` 魔数），
  v1 JSON/旧字段自动升级。
- `src/stats/` — SQLite；`PRAGMA user_version` 迁移在 migrate.rs。
- `src/sfx.rs` — `Sender<SfxEvent>`；只有 Hit/Miss 有音频，其余变体吞掉。

## 已知问题

- **视口卡顿/晕（用户报告，2026-11-24）**：M0 重构后 Sixshot/Gridshot
  感觉比原来卡、晕。release 与 debug 体感相同 → 不是优化等级问题。
  已排查并排除：每帧系统（look/camera/present/cursor/escape/hud/spawn）、
  渲染管线（RT 格式、三相机、灯光/材质/阴影、球面细分）、`.sce` 归一化
  结果、golden digest 全部与重构前一致。待验证方向：外部因素（驱动、
  其他常驻进程、显卡控制面板的 vsync/省电策略）；需要在 HUD 上加
  实时帧率/帧时间读数做实测对比（M1 加跟踪指标时顺手做）。

## 容易踩的坑

- `init_state` 需要 `StatesPlugin`（headless 测试里手动加）。
- headless App 需要 `MinimalPlugins` + 手动 `init_resource::<Assets<Mesh>>` 等。
- 命中判定读 `RenderPos`/`Hitboxes`（M1 起，所见即所得），不读 `Transform`。
- DSL 键值统一小写匹配（`minSep`/`jumpI` 驼峰写法都能命中）。
- 跟枪场景（`ShotScoring=OnTargetPercent`）点击只触发受击反应、不杀目标。
- `.sce` 里 `{` 开头的行 = 地图 JSON 起点，段解析到那里为止。
- Esc 退出不写成绩；只有 `finish_play`（打满时长）写 `.shot` + SQLite。

## M2-M8 结构速查

- src/viewport/combat.rs：WeaponState（射速/弹匣/后座/散布）、fire_hitscan（命中+伤害管线）、track_beam、advance_projectiles（含 aoe）、bot_fire（AI 开火）、PlayerHp。
- src/viewport/course.rs：课程 Intermission 的 round_reset / intermission_tick。
- src/playlist.rs：内置课程 BUILTIN + PlanRun + 自适应 suggest_knobs / apply_knobs。
- src/analysis.rs：局后指标写入 session_metrics；shot reanalyze 补算历史。
- src/sim/movement.rs：WASD/跳/蹲/滑铲 + AABB 碰撞（ShotBlock / ShotMapBounds）。
- DSL 速查：ShotWeapon（mode/rpm/dmg/hsmult/mag/reload/spread/recoil/ads/aoe）、ShotPlayer（preset/hp）、ShotSpawnShape（humanoid）、ShotSpawnHP、ShotHeadshotOnly、ShotBot、ShotMapBounds、ShotBlock[N]、ShotEnd。
- 滑铲：Shift（移动中）；ADS：右键；回放第三人称：V。
- 启动课程：shot playlist <id>（菜单「课程」组）。
