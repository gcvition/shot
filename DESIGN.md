# Shot 瞄准训练器 — 设计文档与技术选型

本文是本仓库的实现蓝图。仓库当前只有空的 `shot` crate 和 `res/主题.json`（KovaaK 主题导出）。准星图、命中/未命中音效按约定放在 `res/`，实现时按路径加载；若文件尚未放入，启动时回退到程序生成的十字准星和静音，并在设置页提示。

---

## 1. 目标与非目标

### 目标（v1）

- 第一人称、hitscan 点击训练，手感接近 KovaaK / Aim Lab。
- 两种场景：**六目标**（1w6ts 同类）和 **网格射击**（Gridshot / Tile Frenzy 同类）。
- 每局固定 **60 秒**。
- 灵敏度用 **厘米/360°**，默认 **19.05**；FOV 可调。
- **自定义渲染分辨率**，对局时 **拉伸铺满全屏**（允许非等比，对标 CS 拉伸 4:3）。
- 主题、准星、命中音、未命中音可换，默认资源来自 `res/`。
- 记录每局得分，结束后展示该场景的得分趋势。
- 每局可回放，回放中绘制鼠标/准星轨迹。
- 视频默认写入 `{应用根}/cache/video/`，不弹文件选择框。
- 会话数据从第一天就按「可训练鼠标运动模型」的格式落盘，而不是事后从视频里抠。

### 非目标（v1 不做）

- 移动靶、跟踪、弹道、后坐力。
- 在线排行榜、账号。
- 运行时训练神经网络（只预留数据集；训练是后续独立工具）。
- 对标 KovaaK 的全部场景编辑器。

### 质量第一约束

瞄准器的产品指标不是画面，而是 **输入到准星转动的延迟与确定性**。任何会增加点击延迟的功能（实时编码、对局中跑完整桌面 UI、把视角放进 `FixedUpdate`）都视为缺陷。

---

## 2. 产品规格

### 2.1 应用流程

```
gpui-kit 壳（主菜单 / 设置 / 结算 / 历史）
    → 开始训练或回放
    → Bevy 无边框全屏窗口（自定义分辨率渲染，拉伸铺满）
    → 60s 训练（或回放结束 / ESC）
    → 回到 gpui-kit 结算（本局数据 + 历史趋势）
```

逻辑状态：`Menu | Playing | Results | Replay`。  
**壳层窗口**（菜单、设置、结算、折线）由 gpui-kit 持有；**对局/回放的 3D 视口**由 Bevy 持有。两套框架都要主循环，不要嵌进同一窗口。

### 2.2 设置项

| 项 | 默认 | 说明 |
|---|---|---|
| `cm_per_360` | `19.05` | 鼠标垫上走一圈 360° 的物理距离 |
| `dpi` | `800` | **必须由用户填写**。厘米/360 无法单独从鼠标计数换算成角度 |
| `fov_deg` | `103` | 用户侧水平 FOV |
| `fov_kind` | `HorizontalRes` | 另可选 `Horizontal4x3`、`Vertical` |
| `render_width` | 显示器宽 | 3D+HUD 的内部分辨率 |
| `render_height` | 显示器高 | 同上 |
| `display_mode` | `BorderlessStretch` | 对局窗口铺满当前显示器，画面拉伸填满 |
| `theme_path` | `res/主题.json` | KovaaK 风格 JSON |
| `crosshair_path` | `res/准星.png` | 带 Alpha 的屏幕空间贴图 |
| `hit_sound` | `res/命中.wav` | 点击命中 |
| `miss_sound` | `res/未命中.wav` | 点击未命中 |
| `master_volume` | `1.0` | |
| `video_enabled` | `true` | 局后是否自动导出 MP4 |

设置存 `{应用根}/cache/settings.json`。灵敏度 **不随 FOV 缩放**（物理 360 距离恒定）。这与部分 KovaaK UE4 模式不同，也是正确的 cm/360 语义。

FOV 的宽高比用 **`render_width / render_height`**，不用显示器宽高比。1280×960 拉伸到 1920×1080 时，透视必须按 4:3 算，否则「拉伸」只变形、不改变手感。

### 2.3 六目标（SixTargets）

对标 KovaaK `1wall6targets small`：

- 玩家站在墙正前方，**不能平移**，只能 yaw/pitch。
- 墙上始终有 **6** 个静止球靶（或胶囊体）。
- 点击命中：立即销毁该靶，在墙上另选一个与现有靶不重叠、且不太贴边的位置生成新靶。
- 未命中：播未命中音，计入射击次数，靶不变。
- 计分：`score = hits`（加准确率展示，不把准确率乘进分数，避免玩家刷分方式扭曲）。
- 时长 60s，到点硬停，最后一发若在截止帧已按下则仍结算。

生成约束（建议常量，可之后做成设置）：

- 墙面本地坐标均匀采样。
- 靶半径默认 `0.25 m`，靶心最小间距 `3 * radius`。
- 新靶不得与剩余 5 个重叠。

### 2.4 网格射击（GridShot）

对标 Aim Lab Gridshot / KovaaK Tile Frenzy：

- 墙上 **3×3** 立方块格子。
- 任意时刻 **3** 个格子点亮为靶。
- 命中点亮格：该格熄灭，在空闲 6 格中随机点亮一格。
- 计分与时长同六目标。

### 2.5 结算与趋势

每局写入索引后，回到 gpui-kit 结算页：

- 本局：得分、命中、未命中、准确率、击毁速率（hits/s）。
- 趋势：该场景最近 N 局（默认 30）得分折线，本局高亮。
- 操作：回放本局、再来、返回菜单。
- 历史列表：任意旧局可进回放（读 `cache/sessions/`，视频若已生成则可「打开所在文件夹」，回放本身不依赖 MP4）。

### 2.6 回放

- 用 **会话轨迹** 重建相机与靶，而不是播 MP4。
- 叠加 **准星轨迹**：屏幕空间折线（时间衰减颜色）+ 可选「墙面命中点」折线。
- 控件：播放/暂停、0.25× / 1× / 2×、进度条。回放控制条可以放在 Bevy HUD，或 ESC 后回壳层再打开；v1 用 Bevy 内极薄 overlay，避免回放时再起一套完整桌面 UI。
- MP4 是轨迹的衍生物；应用内回放永远走轨迹。
- 回放窗口与对局相同：同一套渲染分辨率 + 拉伸全屏，保证「看到的」和当时一致。

### 2.7 分辨率与拉伸全屏

对标 CS / Valorant 的自定义分辨率，而不是「窗口开多大渲多大」。

**显示模式（v1）**

| 模式 | 行为 |
|---|---|
| `BorderlessStretch`（默认） | 无边框窗口 = 当前显示器桌面分辨率；3D 渲在 `render_*` 的离屏纹理上，再 **XY 独立缩放** 铺满窗口（可以变形） |
| `Windowed` | 调试用：普通窗口，内容同样按 `render_*` 拉伸到客户区 |

不做 GPU 控制面板那套「独占全屏改显示器模式」：不同显卡驱动的缩放（保持比例 / 拉伸 / 整数倍）不可控，软件自己拉伸才能保证每个用户都是铺满。

**分辨率 UI**

- 下拉：当前桌面原生、`1920×1080`、`1280×960`、`1280×720`、`1024×768`、`1440×1080`，以及自定义宽高。
- 校验：宽高 ≥ 640×480、≤ 显示器物理像素的 2 倍；必须是偶数（H.264 编码友好）。
- 改分辨率只影响下一局；对局中不热切换。

**渲染路径**

```
Camera3d → RenderTarget::Image(render_width × render_height)
                │
                │  准星 / 倒计时 / 分数画在同一张纹理上
                ▼
     全屏四边形 / 2D 相机  →  窗口客户区（非等比 scale）
```

准星必须画在离屏纹理上再一起拉伸，不能画在未拉伸的窗口坐标里（否则 4:3 拉伸时准星几何会和准心错位）。

hitscan 用相机 forward，与窗口像素无关。Raw 鼠标也不走窗口坐标。

---

## 3. 技术选型

### 3.1 引擎：Bevy 0.19 做 3D 视口（选定）

| 方案 | 优点 | 缺点 | 结论 |
|---|---|---|---|
| **Bevy 0.19** | 3D 相机/FOV、wgpu、原始鼠标、音频、ECS 很适合靶子、离屏渲染成熟 | 比自制渲染器重；不要用它做壳层 UI | **对局/回放视口** |
| wgpu + winit 自研 3D | 延迟上限可控 | 场景、资源、回放相机全手写 | 否 |
| Fyrox / Godot / 纯 2D | — | 生态或透视不对 | 否 |

壳层 UI 见 3.6，不走 `bevy_egui`。

仓库已是 `edition = "2024"` 的空 crate，直接：

```toml
[package]
name = "shot"
version = "0.1.0"
edition = "2024"

[dependencies]
bevy = "0.19"
gpui-kit = "0.6"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"
thiserror = "2"
rand = "0.9"
rand_chacha = "0.9"
postcard = { version = "1", features = ["alloc"] }
rusqlite = { version = "0.32", features = ["bundled"] }
chrono = { version = "0.4", features = ["serde", "clock"] }
which = "7"                    # 探测系统 ffmpeg
openh264 = "0.9"               # 无 ffmpeg 时的纯 Rust 回退；落地时钉兼容版本
```

`video-rs` **不要**放进默认依赖：它链接系统 libav，没装开发库的 Windows 会编不过。作为可选 feature：

```toml
[features]
default = []
video-rs-encoder = ["dep:video-rs"]

[dependencies]
video-rs = { version = "0.10", optional = true }
```

开发期 Bevy 可用 `dynamic_linking`；发布关。

**对局里 HUD 只用 Bevy 极薄 overlay（画在离屏纹理上）。** Playing / Replay 不跑 gpui-kit 布局。

### 3.2 输入：winit 相对鼠标 + 自写 cm/360

- 开局：`CursorGrabMode::Locked`、隐藏系统光标、窗口聚焦。
- 只使用 **相对位移**（Bevy `MouseMotion` / winit `DeviceEvent::MouseMotion`），不要用光标像素位置。
- Windows 上相对位移走 Raw Input，绕过指针加速。
- **视角在 `PreUpdate` 按事件累计**，不要放 `FixedUpdate`。
- 同一帧内：先应用全部 `dx/dy`，再处理点击 hitscan。用 `.chain()` 固定顺序。

DPI 不能从系统可靠读取（显示器 DPI ≠ 鼠标 DPI），设置里必填。

### 3.3 音频：Bevy Audio（rodio）

命中/未命中是 2D 音效，不需要空间化。设置里换文件即热加载失败时回退静音。

### 3.4 持久化：SQLite + 二进制轨迹

| 数据 | 格式 | 路径 |
|---|---|---|
| 设置 | JSON | `cache/settings.json` |
| 局索引 / 得分趋势 | SQLite | `cache/stats.db` |
| 会话轨迹（权威） | postcard 二进制 | `cache/sessions/{uuid}.shot` |
| 回放视频（衍生） | H.264 MP4 | `cache/video/{uuid}.mp4` |

不用「只存视频」：MP4 有损、鼠标采样被压到帧率、未来训练模型还得再解码。轨迹文件 60 秒、1 kHz 量级大约 1–2 MB，可接受。

### 3.5 视频编码：系统 ffmpeg → 纯 Rust 回退

对局中实时录屏 **禁止**。局后用轨迹重放，把 **离屏渲染纹理**（`render_width × render_height`，60 fps）交给编码器。编码分辨率用游戏内部分辨率，不把拉伸后的桌面像素再压一遍。

运行时探测顺序（第一次导出时做，结果缓存到进程内）：

```
1. 环境变量 SHOT_FFMPEG（显式路径）
2. PATH 上的 ffmpeg / ffmpeg.exe（which）
3. Windows 常见位置：scoop / chocolatey / `C:\ffmpeg\bin\ffmpeg.exe`
4. 若编译了 feature `video-rs-encoder` 且能 `video_rs::init()`
5. 纯 Rust openh264 + MP4 mux
```

| 后端 | 何时用 | 怎么喂 |
|---|---|---|
| **系统 ffmpeg CLI（优先）** | `ffmpeg -version` 成功 | stdin 管道：`rawvideo` + `rgba` 或 `bgr0`，输出 `libx264` `yuv420p` `-movflags +faststart` |
| **video-rs** | 无 CLI，但本机有 ffmpeg **库** 且启用了 feature | `Encoder` + `preset_h264_yuv420p`。它不是纯 Rust，只是没 CLI 时的 libav 绑定 |
| **openh264** | 上面都没有 | Cisco openh264 出 Annex-B，再用 `mp4`/`minimp4` 复用。无系统依赖，画质和编码器选项弱于 libx264 |

ffmpeg 命令草案（实现时按实际像素格式微调）：

```text
ffmpeg -y -f rawvideo -pix_fmt rgba -s {w}x{h} -r 60 -i -
       -an -c:v libx264 -preset fast -crf 18 -pix_fmt yuv420p
       -movflags +faststart
       cache/video/{session_id}.mp4
```

实现成 `trait VideoSink { fn push_rgba(&mut self, frame: &[u8]); fn finish(self) -> Result<PathBuf>; }`，启动导出时选一个 impl。失败不丢局：轨迹仍在，结算页可「重试导出」。

编码在独立线程/进程。gpui-kit 壳上显示「正在导出回放」。不要附带一份 ffmpeg 进仓库。

落地顺序：先应用内轨迹回放；再接 `VideoSink`；先实现 ffmpeg CLI，再 openh264，video-rs 最后（可选 feature）。

### 3.6 壳层 UI：gpui-kit（选定）

菜单、设置、结算、历史、得分折线用 **[gpui-kit](https://gpui-kit.com)**（`gpui-kit = "0.6"`，一层依赖带上 GPUI / gpui-base / gpui-component）。

| 方案 | 结论 |
|---|---|
| **gpui-kit** | **选定。** 原生控件、主题、表格、**内置 Chart**，中文支持好 |
| bevy_egui + egui_plot | 不再作为壳层。对局叠加也会抢帧 |
| Bevy UI 做菜单 | 表单和折线成本高 |

职责划分：

| 表面 | 框架 |
|---|---|
| 主菜单、设置、结算、趋势图、历史列表、导出进度 | gpui-kit |
| 3D 场景、准星、对局 HUD、回放轨迹线 | Bevy |

**集成（Windows 优先）**

gpui-kit 与 Bevy 都要窗口和事件循环，**禁止**把 GPUI 控件画进 Bevy 交换链。

选定：**同一进程、两扇窗口、时序交接**。

1. 进程入口 `gpui_kit::application().run(...)`，打开壳窗口。
2. 用户点「开始」/「回放」：壳窗口 `window.set_window_hidden(true)`（或最小化），通过 channel 在 **专用线程** 起 Bevy `App`（Windows 允许非主线程跑 winit；若 Bevy 版本要求主线程，则改为「Bevy 占主线程、GPUI 在开局前 `quit` 再在局后重新 `run`」——第二种闪一下，作为后备）。
3. 60s 结束或 ESC：Bevy 窗口关闭，`SessionOutcome` 送回，壳窗口显示结算。
4. 设置、SQLite、路径模块两边共用，不放进任一框架的 Plugin 私有状态。

趋势图：优先 `gpui_kit::component` 的 Chart。数据：

```sql
SELECT score, started_at FROM sessions
WHERE scenario = ? ORDER BY started_at DESC LIMIT 30
```

对局线程 **零** gpui-kit 帧。

---

## 4. 架构

核心原则：**对局只写轨迹；回放和视频和以后的模型都读轨迹。**

```
           gpui-kit 壳（菜单 / 设置 / 结算 / 趋势）
                           │ start session
                           ▼
                    ┌─────────────┐
                    │  Settings   │
                    │  Theme/SFX  │
                    │  RenderRes  │
                    └──────┬──────┘
                           │
┌──────────┐   raw dx/dy   ▼    hitscan     ┌─────────────┐
│ RawMouse ├────────► Camera/YawPitch ─────►│  Scenario   │
└──────────┘            │                   │ Six / Grid  │
                        │                   └──────┬──────┘
                        │  append                  │ events
                        ▼                          ▼
                 ┌─────────────────────────────────────┐
                 │           SessionTrace              │
                 └──────────────┬──────────────────────┘
                                │
            ┌───────────────────┼───────────────────┐
            ▼                   ▼                   ▼
     cache/sessions/      Bevy Replay         VideoSink
     *.shot + stats.db    (准星轨迹)          ffmpeg → openh264
                                │
                                ▼  （后续）
                         运动模型数据集导出
```

### 4.1 模块切分

共享库逻辑（`src/lib.rs` 风格模块，`main` 只启动壳）：

| 模块 | 职责 |
|---|---|
| `settings` / `paths` | 加载保存、校验 DPI / 分辨率 / 资源路径 |
| `theme` | 解析 `主题.json` |
| `input` | cm/360、yaw/pitch |
| `scenario` | 六目标 / 网格 |
| `combat` | hitscan、计分 |
| `session` | 轨迹、SQLite |
| `replay` | 读轨迹、插值 |
| `video` | `VideoSink` 探测与编码 |
| `shell` | gpui-kit 窗口 |
| `viewport` | Bevy Plugin 集合（仅 Playing/Replay） |

Bevy 侧 Plugin：`ThemePlugin`、`InputPlugin`、`ScenarioPlugin`、`CombatPlugin`、`HudPlugin`、`TracePlugin`、`ReplayPlugin`、`DisplayPlugin`（离屏纹理 + 拉伸）。

Playing 态系统顺序：

```
InputLook → FireHitscan → ScenarioRespawn → TraceAppend → Hud
```

全部 `.chain()`（look 也可在 `PreUpdate`，但必须在 fire 之前）。

### 4.2 目录（建议）

```
shot/
  Cargo.toml
  DESIGN.md
  res/
    主题.json
    准星.png
    命中.wav
    未命中.wav
  cache/                 # gitignore；运行时创建
    settings.json
    stats.db
    sessions/
    video/
  src/
    main.rs              # gpui-kit::application
    lib.rs
    settings.rs
    paths.rs
    theme.rs
    input/{mod,sensitivity,look}.rs
    combat/{hitscan,score}.rs
    scenario/{mod,six_targets,gridshot}.rs
    hud/{crosshair,overlay}.rs
    display.rs           # 离屏分辨率 + 拉伸全屏
    session/{trace,store,stats}.rs
    replay/{player,trail}.rs
    video/{mod,ffmpeg,openh264,detect}.rs
    shell/{mod,menu,results,settings_panel,chart}.rs
    viewport.rs          # Bevy App 组装与线程交接
```

`paths.rs`：以可执行文件目录为根（`std::env::current_exe()`），开发时若 `res/` 不在 exe 旁则回退到 `CARGO_MANIFEST_DIR`。`cache/video` 永远相对软件本身。

`.gitignore` 已包含 `/cache`。

---

## 5. 灵敏度、FOV、分辨率公式

### 5.1 cm/360 → 角度

```text
inches_per_360 = cm_per_360 / 2.54
counts_per_360 = dpi * inches_per_360
deg_per_count  = 360 / counts_per_360

yaw   += dx * deg_per_count
pitch += -dy * deg_per_count
pitch  = clamp(pitch, -89.9, 89.9)
```

默认：`19.05 cm/360`、`800 DPI` → `counts_per_360 = 6000` → 每计数 `0.06°`。

单测必须锁死：

- `6000` 计数 yaw 累加 = `360°`（模 360 后误差 `< 1e-4`）。
- 改 DPI 或 cm/360 时，物理一圈距离不变的充要关系成立。

### 5.2 FOV

Bevy `PerspectiveProjection::fov` 是 **垂直 FOV（弧度）**。

用户给水平 FOV `H`（度）。宽高比 **`a = render_width / render_height`**（不是窗口）：

```text
H = H * PI/180
V = 2 * atan(tan(H/2) / a)
camera.fov = V
```

`Horizontal4x3`：用户值先当 4:3 水平 FOV，再换到 `a` 下的垂直 FOV。设置页写清模式。

准星在离屏纹理中心，hitscan 用相机 forward。

### 5.3 拉伸

```text
window_w, window_h = 桌面分辨率
scale_x = window_w / render_width
scale_y = window_h / render_height
```

`scale_x` 与 `scale_y` 可以不相等。不要用 `min(scale_x, scale_y)`（那是黑边 letterbox）。

---

## 6. 战斗与场景细节

### 6.1 Hitscan

- 射线：`origin = camera.translation`，`dir = camera.forward()`。
- 六目标：射线-球体。
- 网格：射线-AABB。
- 一发只打最近靶。空射线 = miss。
- 不模拟子弹飞行。

点击：左键 `just_pressed`。不要把开火放到下一帧。

### 6.2 确定性

每局开始生成 `rng_seed: u64`。靶刷新只用 `ChaCha8Rng::seed_from_u64(seed)`。回放只走事件流；seed 用于校验和以后重新模拟。

### 6.3 主题映射

`res/主题.json` 已是 KovaaK 字段。v1 解释子集：

| JSON | 用法 |
|---|---|
| `wallTint` / `floorTint` / `ceilingTint` | `StandardMaterial.base_color` |
| `wallRoughness` / `Metallic` | PBR |
| `wallFullBright` | emissive 权重 |
| `enemyBodyColor` / `enemyHeadColor` | 靶颜色 |
| `skyColor` | `ClearColor` |
| `*Material` 名称 | v1 忽略或映射内置色板；无纹理包时不失败 |

准星：离屏纹理中心的 `Image`。找不到文件则画简单十字。

---

## 7. 轨迹、回放、视频

### 7.1 轨迹是权威数据

对局中 **禁止** 为了录像去拷每一帧。只做内存追加，预分配。

局结束后 `flush` 到 `cache/sessions/{id}.shot`，insert SQLite，若 `video_enabled` 则投递 `VideoSink`。

### 7.2 二进制 schema（postcard）

`schema_version: u16` 现在为 `1`。

```text
SessionFile
  header:
    schema_version
    session_id: Uuid
    scenario: SixTargets | GridShot
    started_at_unix_ms
    duration_ms
    rng_seed
    settings_snapshot {
      cm_per_360, dpi, fov_deg, fov_kind,
      render_width, render_height, display_mode,
      window_width, window_height, refresh_hz
    }
  mouse:  Vec<MouseSample>   # t_us, dx, dy, yaw_deg, pitch_deg
  shots:  Vec<ShotEvent>     # t_us, hit, target_id, yaw, pitch
  world:  Vec<WorldEvent>    # t_us, Spawn{...} | Despawn{id}
```

同时存 `dx/dy` 和 `yaw/pitch`。时间用开局为零点的微秒。

### 7.3 准星轨迹怎么画

回放时（在离屏纹理坐标里画，再随画面拉伸）：

1. 用样本 yaw/pitch 得相机 forward。
2. 与墙平面求交；再投到离屏纹理得 2D 点。
3. 最近 T 秒（默认 0.4s）折线，越旧越透明。
4. 射击点：命中圆点 / 未命中叉。

### 7.4 视频文件

- 路径：`{app}/cache/video/{session_id}.mp4`。
- 画面：离屏回放帧（含轨迹、准星、HUD），60 fps，`render_width × render_height`。
- 同 id 重导则覆盖。无文件对话框。
- 可设置「只保留最近 50 个视频」；轨迹默认长期留。

---

## 8. 为鼠标运动模型预留

1. **不要从视频学。**
2. 样本对齐 HID 事件，带 yaw/pitch 与目标相对角。
3. `WorldEvent` 可重建任意时刻靶位置。
4. `settings_snapshot` 含分辨率，不同 FOV/拉伸的局仍能换算到角度。
5. schema 只追加字段，bump `schema_version`。

后续导出（v1 不做）：

```text
对每个 shot：
  窗口 [t_shot - 400ms, t_shot] 的 mouse 序列
  该窗口内主目标的相对角序列
  label: 是否命中 / 最终误差角
```

---

## 9. SQLite 表

```sql
CREATE TABLE sessions (
  id            TEXT PRIMARY KEY,
  scenario      TEXT NOT NULL,
  started_at    INTEGER NOT NULL,
  duration_ms   INTEGER NOT NULL,
  score         INTEGER NOT NULL,
  hits          INTEGER NOT NULL,
  misses        INTEGER NOT NULL,
  cm_per_360    REAL NOT NULL,
  dpi           INTEGER NOT NULL,
  fov_deg       REAL NOT NULL,
  render_width  INTEGER NOT NULL,
  render_height INTEGER NOT NULL,
  trace_path    TEXT NOT NULL,
  video_path    TEXT,
  rng_seed      INTEGER NOT NULL
);
CREATE INDEX idx_sessions_scenario_time ON sessions(scenario, started_at);
```

---

## 10. 实现路线

### P0 — 空房间能转视角 + 拉伸全屏

- gpui-kit 壳能开，点开始后拉起 Bevy 窗口。
- 离屏 `render_*` + 无边框拉伸全屏。
- cm/360 + DPI、FOV 按渲染宽高比换算。
- 中心准星画在离屏纹理上。
- 单测：6000 count = 360°；1280×960 的 `a` 进入 FOV 公式。

### P1 — 六目标可练

- 6 靶、hitscan、音效、60s、主题着色。

### P2 — 网格 + 设置

- GridShot。
- gpui-kit：灵敏度、FOV、**分辨率预设/自定义**、资源路径、音量。

### P3 — 记分与趋势

- SQLite。
- 结算页 Chart。

### P4 — 轨迹回放

- `.shot` + Replay 态 + 轨迹线 + 倍速。

### P5 — 自动导出 MP4

- 探测系统 ffmpeg；没有再 openh264（可选 video-rs feature）。
- 写入 `cache/video/`，失败可重试。

P4 完成时产品闭环已经成立。

---

## 11. 风险

| 风险 | 处理 |
|---|---|
| gpui-kit 与 Bevy 抢事件循环 | 分窗口；Windows 上 Bevy 专用线程。若主线程限制，改为局前退出 GPUI、局后重启壳 |
| 拉伸用了窗口宽高比算 FOV | FOV 只认 `render_*`；加单测 |
| 准星画在窗口空间 | 必须进离屏纹理 |
| 对局跑壳层 UI | Playing 零 gpui-kit |
| 实时录像 | 禁止；只局后 `VideoSink` |
| 本机无 ffmpeg | openh264；结算提示当前后端 |
| video-rs 在 Windows 难链 libav | 默认不编译；可选 feature |
| 缺 DPI | 设置强制填；默认 800 |
| `res` 缺资源 | 十字/静音，不崩溃 |
| postcard 不兼容 | `schema_version` |

---

## 12. 关键实现备忘（Bevy 0.19）

- `Resource` 不要同时 derive `Component`。
- 生成用元组组件，不要用已删除的 `*Bundle`。
- 运动/计时用 `time.delta_secs()`；**视角不要用 delta 缩放鼠标计数**。
- `Commands` 生成的新靶同一系统里 Query 不到。
- 离屏 `Image` 要 `TextureUsages::RENDER_ATTACHMENT \| TEXTURE_BINDING \| COPY_SRC`（`COPY_SRC` 给局后回读编码）。
- 发布：`--release`。

---

## 13. 资源与 cache 约定

当前仓库 `res/` 仅有 `主题.json`。加载顺序：

1. `{exe_dir}/res/...`
2. `{CARGO_MANIFEST_DIR}/res/...`
3. 回退

运行时创建：

```
cache/settings.json
cache/stats.db
cache/sessions/{id}.shot
cache/video/{id}.mp4
```

用户永远不需要选这些路径。
