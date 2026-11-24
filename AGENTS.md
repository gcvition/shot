# Shot — 开发指南

桌面端 FPS 瞄准训练器。一个 exe 两个进程：GPUI Kit 菜单 + Bevy 0.19 训练/回放视口。
扩展计划见 `.devin` 之外的用户计划文件（里程碑 M0–M8）；当前处于 M0 收尾：
只重构，不改现有玩法行为。

## 构建 / 测试（Windows，cargo 在 scoop persist 里）

```bash
cargo.exe test --lib          # 单测，含 golden 确定性回归
cargo.exe clippy --all-targets
cargo.exe build               # shot.exe
```

## 关键约束：golden digest

`src/viewport/tests.rs` 的 `golden::play_mode_should_match_golden_digest` 用固定
种子 + 输入脚本跑 headless App，哈希所有射击/出生/消失事件。当前 digest：
**`8013938259272503606`**（6 中 3 空，9 枪）。

- 重构时 digest 变了 = 行为变了，先查清楚是不是有意。
- 刻意改行为时更新 `GOLDEN_DIGEST` 常量并在提交说明里写原因。

## 结构（M0 之后）

- `src/viewport/` — Bevy 玩法，按职责分文件（mod.rs 文件头有表格）。
  新 system 放进 `PlaySet`（Gate→Look | Camera→Fire→Clock→Replay→Hud→Present）。
  单 system 最多 16 参数，多的用 `#[derive(SystemParam)]` 打包（见 `SpawnCtx`）。
- `src/sce/` — `.sce` 管线：document（分段）→ convert（归一化）→ ScenarioSpec。
  v1 场景走 `spec.legacy`；`[Shot Extension]` 键在 extension.rs。
  `res/scenarios/` 递归扫描，文件名 = 场景 id，重名给 warning。
- `src/session/` — `.shot` 轨迹：v2 结构 + postcard+zstd（`SHOT2` 魔数），
  v1 JSON/旧字段自动升级。
- `src/stats/` — SQLite；`PRAGMA user_version` 迁移在 migrate.rs。
- `src/sfx.rs` — `Sender<SfxEvent>`；只有 Hit/Miss 有音频，其余变体吞掉。

## 容易踩的坑

- `init_state` 需要 `StatesPlugin`（headless 测试里手动加）。
- headless App 需要 `MinimalPlugins` + 手动 `init_resource::<Assets<Mesh>>` 等。
- 命中判定读 `SimPos`/`Hitboxes`，不读 `Transform`（Transform 只是渲染位）。
- `.sce` 里 `{` 开头的行 = 地图 JSON 起点，段解析到那里为止。
- Esc 退出不写成绩；只有 `finish_play`（打满时长）写 `.shot` + SQLite。
