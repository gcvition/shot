# Shot

Windows 桌面瞄准训练器。菜单用 [GPUI Kit](https://gpui-kit.com)，训练和回放用 [Bevy 0.19](https://bevyengine.org)。手感按厘米/360° 计算，场景来自 KovaaK 风格的 `.sce` 文件。

当前版本 **0.1.0**（写在 `Cargo.toml` 的 `[package].version`）。改这个数字再编译，窗口标题和菜单顶栏会一起变成新版本，不必再改别处。

## 它做什么

- 选场景打 **60 秒**（`.sce` 里的 `Timelimit` 可以覆盖）
- **Sixshot Ultimate**：墙上同时 6 个小黑点，打掉立刻补一个
- **Gridshot Ultimate**：3×3 格子里同时活 3 个金属球
- 厘米/360° + DPI 决定每一鼠标 count 转多少度，**不乘帧时间**
- 自定义渲染分辨率，可无边框拉伸（对标 CS 拉伸 4:3）
- 主题、准星、命中/空枪音效可换
- 开局倒计时 0–30 秒
- 打完写入 SQLite 成绩和 `.shot` 轨迹，可回放准星扫墙轨迹
- **Esc 中途退出不计入成绩**；只有打满时长才记一局

没有移动靶、弹道、后坐力、在线排行榜，也没有对局录像编码。

## 环境

| 需要 | 说明 |
|------|------|
| Windows | 主平台。显示器分辨率列表走 Win32；其它系统会回落到一组常见分辨率 |
| Rust | `edition = "2024"`，请用较新的稳定版 rustc（建议 1.85+） |
| GPU | Bevy 默认渲染后端 |

开发时在仓库根目录运行即可，程序会找到旁边的 `res/`。

## 构建与运行

```bash
cargo test --lib
cargo run --release
```

第一次编译 Bevy 会比较久。菜单窗口固定 **640×480**，标题是 `Shot v0.1.0`。

同一个 exe 还能当训练进程用（菜单点「开始训练」时会自己再拉起一份）：

```bash
# 菜单
cargo run --release

# 直接开一局（场景 id = 文件名，不含 .sce）
cargo run --release -- play "Sixshot Ultimate"

# 回放 cache/sessions/<id>.shot
cargo run --release -- replay <session-id>
```

发布时把 `shot.exe` 和整个 `res/` 放在同一目录。程序按这个顺序找资源：

1. exe 旁边的 `res/`
2. 源码树的 `res/`（`cargo run` 时）
3. 当前工作目录

运行时数据写在 `cache/`（已 gitignore），不要往 `res/` 里写设置或成绩。

```
cache/settings.json
cache/stats.db
cache/sessions/<id>.shot
cache/last_session.json
```

## 怎么玩

### 菜单

三个 Tab：**训练** / **设置** / **记录**。顶栏右侧是版本号。

1. 在「训练」里点场景按钮，开始一局
2. 菜单窗口会卡住，直到训练窗口关掉
3. 打满 60 秒后跳到「记录」，可以看到得分、命中、准确率、命中/秒和趋势图
4. 按 **Esc** 提前退出：窗口关掉，本局 **不写入** 成绩，菜单提示「已退出，本局不计入记录」

历史列表可以按场景筛选，也可以回放或删除。删除会同时去掉 SQLite 行和 `.shot` 文件。

### 训练窗口（Shot Play）

| 操作 | 作用 |
|------|------|
| 鼠标移动 | 转视角 |
| 左键 | 开火（hitscan，打最近的球） |
| Esc | 立刻退出；没打满时长就不记分 |

从菜单点进来时左键可能还按着，所以必须窗口聚焦 **并且** 松开过左键之后才会开始计开火，避免误触。

倒计时没走完时可以转视角，但不能开枪，时间也不往「60 秒」里加。

### 回放

| 操作 | 作用 |
|------|------|
| Space | 暂停 / 继续 |
| 1 / 2 / 3 | 0.25× / 1× / 2× |
| ← / → | 后退 / 前进 1 秒 |
| Esc | 退出回放 |

橙色折线是最近约 0.55 秒准星扫过墙面的轨迹，绿点是命中。

## 设置

改完点 **保存**。已经开始的一局 **不会** 热更新，下一局才生效。

| 项 | 含义 |
|----|------|
| 厘米/360° | 鼠标垫上转一圈要滑多少厘米，默认 `19.05`（800 DPI 时正好 6000 count 一圈） |
| DPI | 鼠标 DPI，200–32000 |
| FOV | 60–120 |
| FOV 类型 | 分辨率水平 / 4:3 水平 / 垂直。水平 FOV 按 **渲染分辨率** 的宽高比算，不是窗口拉伸后的比例 |
| 开始倒计时 | 0 = 立刻能开枪，最大 30 |
| 渲染分辨率 | 来自系统枚举，也可自定义；必须是偶数，范围 640×480–7680×4320 |
| 显示 | 无边框（拉满主屏，会拉伸）或窗口 |
| 主题 / 准星 / 音效 | 扫描 `res/themes`、`res/crosshairs`、`res/sounds`（含子文件夹） |

数字框打字时只有落在合法范围才生效；失焦或回车才钳制，避免把 `8` 立刻变成 DPI 200。

## 场景与资源

菜单上出现哪些场景，完全由 `res/scenarios/*.sce` 决定。文件名（不含扩展名）就是场景 id，最多列出 10 个。

自带两个：

| 文件 | 玩法 |
|------|------|
| `res/scenarios/Sixshot Ultimate.sce` | 墙面随机 6 个小点 |
| `res/scenarios/Gridshot Ultimate.sce` | 3×3 格子，同时 3 个活球 |

`.sce` 是 KovaaK 风格 INI，解析不必 100% 兼容，规则大致是：

- `Name`：显示名
- `Timelimit`：秒，至少 5，最多 180，缺省 60
- `AddedBots`：分号分隔，个数 = 同时存在的目标数
- Bot 的 `SpawnOffset` 跨度很大，或地图名带 `gridshot` → 格子模式；否则随机墙

其它资源目录：

```
res/themes/        KovaaK 主题 JSON（墙/地/天/敌人颜色）
res/crosshairs/    准星图（png / jpg / webp），会裁掉空白只留「有墨」的像素
res/sounds/        命中、未命中（ogg / wav）
```

旧路径 `res/主题.json` 这类会在读设置时自动迁到子目录。历史记录里的 `six-targets` / `grid-shot` 也会迁成现在的文件名。

## 架构（二次开发）

仓库里其实是 **两个程序共用一个 exe**：

```
shot.exe              GPUI 菜单（选场景、改设置、看记录）
    └── spawn 自己
        shot.exe play <场景id> <对局id>     Bevy 训练
        shot.exe replay <对局id>            Bevy 回放
```

GPUI 和 Bevy 不共一个消息循环。菜单里 `Command::new(current_exe).args(["play", …]).status()` 会阻塞到训练窗口关掉。

### 菜单：MVI

```
View (src/shell/view.rs)     只画画，点击只发 Intent
        │
        ▼
Intent (src/shell/intent.rs) 「用户想做什么」
        │
        ▼
Update (src/shell/update.rs) Shell::dispatch —— 唯一改状态的入口
        │
        ▼
Model (src/shell/model.rs)   设置、历史、Input / Select / Slider
        └── cx.notify() → 再 render
```

加一个按钮：

1. `intent.rs` 加变体
2. `update.rs` 的 `dispatch` 里处理
3. `view.rs` 里 `this.dispatch(Intent::…)`

不要在 `render` 里写 `self.settings.xxx =`，也不要在 `render` 里改 Select 的 items（会 observe 死循环）。

### 训练：Bevy ECS

3D 不直接画到窗口。训练分辨率和窗口大小可以不同：

```
Camera3d (WORLD_LAYER)  → 一张 Image 渲染目标（RT）
Camera2d (HUD_LAYER)    → 同一张 RT（准星、倒计时）
第三个 Camera2d + Sprite → 把 RT 拉伸到整个窗口
```

世界物体必须带 `RenderLayers::layer(WORLD_LAYER)`。转头用 `Transform::look_at`，不要给 3D 相机加 `Mesh3d`。

想改开火，搜 `fire_hitscan`；想改刷点，看 `setup` 和 `src/scenario.rs`。Play 模式一帧顺序大致是：武装开火门 → 鼠标视角 → 相机 → 射线 → 走表 → HUD → 拉伸 RT → 锁鼠标 → Esc。

### 目录

| 路径 | 职责 |
|------|------|
| `src/main.rs` | 按命令行分流：菜单 / play / replay |
| `src/shell/` | GPUI 菜单（MVI） |
| `src/viewport.rs` | Bevy 训练与回放 |
| `src/launch.rs` | 再 spawn 一份自己 |
| `src/sce.rs` | 读 `.sce` |
| `src/scenario.rs` | 墙面随机点 / 格子的生成数学 |
| `src/session.rs` | `.shot` 轨迹（鼠标、开火、出生点） |
| `src/stats.rs` | SQLite 成绩 |
| `src/settings.rs` | `cache/settings.json` |
| `src/sensitivity.rs` | 厘米/360° → 每 count 多少度 |
| `src/hitscan.rs` / `src/project.rs` | 射线打球、世界坐标投到屏幕 |
| `src/appearance.rs` | 房间贴图、准星裁剪、回放轨迹网格 |
| `src/theme.rs` | 主题 JSON |
| `src/sfx.rs` / `src/wav.rs` | 音效线程 |
| `src/display.rs` | Windows 分辨率列表 |
| `src/fov.rs` | FOV 换成 Bevy 要的垂直弧度 |
| `src/paths.rs` | 根目录 / `res/` / `cache/` |
| `res/` | 跟仓库走的资源 |
| `cache/` | 运行时生成，不进 git |

源码里的 `//!` / `///` 是给二次开发看的说明。本地也可以：

```bash
cargo doc --open --no-deps
```

`.agents/skills/` 里还有 GPUI Kit、Bevy ECS、Rust 习惯的 agent skill，改 UI 或 ECS 时可以按那些规则来。

早期蓝图在 [`DESIGN.md`](DESIGN.md)。有些条目已经过时（例如对局录像），以本 README 和源码为准。

## 测试

```bash
cargo test --lib
```

覆盖灵敏度（默认 19.05cm/360 @ 800 DPI = 6000 count）、场景解析、成绩库、准星裁剪、Esc 不误触开火门、版本号与 `Cargo.toml` 一致等。Bevy 窗口本身没有自动化点击测试。

## 许可

仓库暂未附带许可证文件。使用或再分发前请先和作者确认。
