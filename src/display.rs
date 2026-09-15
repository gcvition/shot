//! Windows 显示器分辨率列表，给设置页的下拉框。
//!
//! 只收偶数宽高（渲染目标更省事）。列表按面积从大到小。
//! 非 Windows 会回落到 1080p / 720p 那一组常用值。

use crate::settings::validate_resolution;

/// 当前桌面分辨率。拿不到就 `None`，调用方再用列表里的第一项。
pub fn current_resolution() -> Option<(u32, u32)> {
    #[cfg(windows)]
    {
        win::current().and_then(even_valid)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// 去重、只留偶数宽高、从大到小。设置页直接拿来当 Select 选项。
pub fn list_resolutions() -> Vec<(u32, u32)> {
    let mut modes = Vec::new();
    #[cfg(windows)]
    {
        modes.extend(win::enumerate());
    }
    if let Some(current) = current_resolution() {
        modes.push(current);
    }
    if modes.is_empty() {
        modes.extend([
            (1920, 1080),
            (1600, 900),
            (1440, 1080),
            (1280, 960),
            (1280, 720),
            (1024, 768),
        ]);
    }
    unique_sorted(modes)
}

pub fn preferred_resolution() -> Option<(u32, u32)> {
    current_resolution().or_else(|| list_resolutions().into_iter().next())
}

pub fn unique_sorted(modes: impl IntoIterator<Item = (u32, u32)>) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = modes.into_iter().filter_map(even_valid).collect();
    out.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    out.dedup();
    out
}

fn even_valid((width, height): (u32, u32)) -> Option<(u32, u32)> {
    let width = width & !1;
    let height = height & !1;
    validate_resolution(width, height)
        .ok()
        .map(|_| (width, height))
}

#[cfg(windows)]
mod win {
    use windows_sys::Win32::Graphics::Gdi::{
        DEVMODEW, ENUM_CURRENT_SETTINGS, ENUM_DISPLAY_SETTINGS_MODE, EnumDisplaySettingsW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

    pub fn current() -> Option<(u32, u32)> {
        unsafe {
            let mut mode: DEVMODEW = std::mem::zeroed();
            mode.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
            if EnumDisplaySettingsW(std::ptr::null(), ENUM_CURRENT_SETTINGS, &mut mode) != 0
                && mode.dmPelsWidth > 0
                && mode.dmPelsHeight > 0
            {
                return Some((mode.dmPelsWidth, mode.dmPelsHeight));
            }
        }
        let width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
        let height = unsafe { GetSystemMetrics(SM_CYSCREEN) };
        if width > 0 && height > 0 {
            Some((width as u32, height as u32))
        } else {
            None
        }
    }

    pub fn enumerate() -> Vec<(u32, u32)> {
        let mut modes = Vec::new();
        for index in 0..1024u32 {
            let mut mode: DEVMODEW = unsafe { std::mem::zeroed() };
            mode.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
            let ok = unsafe {
                EnumDisplaySettingsW(
                    std::ptr::null(),
                    index as ENUM_DISPLAY_SETTINGS_MODE,
                    &mut mode,
                )
            };
            if ok == 0 {
                break;
            }
            if mode.dmPelsWidth > 0 && mode.dmPelsHeight > 0 {
                modes.push((mode.dmPelsWidth, mode.dmPelsHeight));
            }
        }
        modes
    }
}

#[cfg(test)]
mod tests {
    use super::unique_sorted;

    #[test]
    fn unique_sorted_should_keep_largest_first_and_drop_odd() {
        let modes = unique_sorted([
            (1280, 720),
            (1920, 1080),
            (1920, 1080),
            (1281, 720),
            (2560, 1440),
        ]);
        assert_eq!(modes, vec![(2560, 1440), (1920, 1080), (1280, 720)]);
    }
}
