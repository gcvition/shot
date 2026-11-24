//! 自瞄诊断日志：`cache/logs/aimbot-<场景>-<会话8>.log`。
//!
//! 设计目标：用户跑一局后，排查者只看日志就能重建"瞄准当时在想
//! 什么"——锁定/丢锁事件、每帧的阶段/误差/输出量/朝向、开火与
//! 命中空枪的差分、结束汇总。行格式紧凑可 grep：
//!
//! ```text
//! START t=0.000 scen=wall sess=abc12345 key=Right fov=36.0° spd=1.00x/1.00y part=Head deg=0.060000
//! LOCK  t=0.412 id=3 dist=142.3 w=28.7
//! F     t=0.608 ph=Flick id=3 dx=142.3 dy=-20.1 w=28.7 out=18,-4 yaw=12.34 pit=-2.10
//! FIRE  t=1.020 id=3 err=-1.2,0.4 w=28.7 hold=33ms
//! HIT   t=1.041 hits=5 misses=2
//! UNLOCK t=1.050 id=3 why=dead_or_out_of_fov
//! END   t=20.000 hits=24 misses=1 frames=1240
//! ```
//!
//! headless 测试/无 `AppPathsRes` 环境自动不写（`ensure_open` 静默失败）。

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

/// 每写这么多行强制 flush 一次（~1s 的逐帧量，防中途崩/强杀丢日志）。
const FLUSH_EVERY: u32 = 30;

#[derive(Default)]
pub(crate) struct AimbotLog {
    w: Option<BufWriter<File>>,
    lines_since_flush: u32,
    /// 上一次见 ScoreState 的命中/空枪数（差分出 HIT/MISS 事件）。
    pub hits: i64,
    pub misses: i64,
    /// 持键期间累计帧数（END 汇总用）。
    pub frames: u64,
}

impl AimbotLog {
    /// 打开日志文件（已开则复用）。`logs_dir` 不存在就创建；
    /// 任何失败都静默降级为"不记日志"——诊断设施不能搞挂玩法。
    pub fn ensure_open(&mut self, logs_dir: &Path, scenario: &str, session: &str) {
        if self.w.is_some() {
            return;
        }
        let name = format!(
            "aimbot-{}-{}.log",
            sanitize(scenario),
            session.get(..8).unwrap_or(session)
        );
        if std::fs::create_dir_all(logs_dir).is_err() {
            return;
        }
        match File::create(logs_dir.join(name)) {
            Ok(f) => self.w = Some(BufWriter::new(f)),
            Err(_) => self.w = None,
        }
    }

    /// 写一行；周期性 flush。
    pub fn line(&mut self, s: &str) {
        let Some(w) = self.w.as_mut() else {
            return;
        };
        let _ = w.write_all(s.as_bytes());
        let _ = w.write_all(b"\n");
        self.lines_since_flush += 1;
        if self.lines_since_flush >= FLUSH_EVERY {
            let _ = w.flush();
            self.lines_since_flush = 0;
        }
    }

    /// 立即 flush（丢锁/结束/事件边界）。
    pub fn flush(&mut self) {
        if let Some(w) = self.w.as_mut() {
            let _ = w.flush();
            self.lines_since_flush = 0;
        }
    }

    pub fn active(&self) -> bool {
        self.w.is_some()
    }
}

/// 场景 id 进文件名：只留字母数字和 -_。
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}
