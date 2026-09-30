//! 多模块管理抽象 —— Game Manager 的愿景：不只管理存档，还将管理修改器与 MOD。
//!
//! 预留方式：所有功能模块实现 [`GameModule`] 接口并向 [`modules`] 注册表登记。
//! 当前只有 saves（存档）模块真正实现；trainers / mods 为占位登记，
//! 后续实现时在此接入，UI 导航与命令层按 ModuleInfo 驱动，无需大改。

use serde::Serialize;

/// 一个功能模块对外暴露的元信息（UI 导航、能力探测都以此为准）
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleInfo {
    /// 稳定标识：saves / trainers / mods
    pub id: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    /// false = 占位（规划中，UI 显示为禁用）
    pub implemented: bool,
    /// 能力标签，供 UI 与命令层探测
    pub capabilities: &'static [&'static str],
}

pub trait GameModule {
    fn info(&self) -> ModuleInfo;
}

/// 存档模块（当前唯一实现；能力由 scanner/backup/restore 引擎承载）
pub struct SaveModule;

impl GameModule for SaveModule {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: "saves",
            display_name: "存档",
            description: "自动扫描、备份与恢复游戏存档，支持多账号与多平台",
            implemented: true,
            capabilities: &["scan", "backup", "restore", "auto-backup"],
        }
    }
}

/// 预留占位模块
pub struct ReservedModule {
    pub id: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
}

impl GameModule for ReservedModule {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: self.id,
            display_name: self.display_name,
            description: self.description,
            implemented: false,
            capabilities: &[],
        }
    }
}

/// 全部已注册模块（顺序即 UI 导航顺序）
pub fn modules() -> Vec<ModuleInfo> {
    vec![
        SaveModule.info(),
        ReservedModule {
            id: "trainers",
            display_name: "修改器",
            description: "游戏修改器的管理、更新与一站式启动（规划中）",
        }
        .info(),
        ReservedModule {
            id: "mods",
            display_name: "MOD",
            description: "MOD 的安装、版本管理与冲突排查（规划中）",
        }
        .info(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_registry_invariants() {
        let mods = modules();
        // 存档模块必须存在、已实现且排在首位
        assert_eq!(mods[0].id, "saves");
        assert!(mods[0].implemented);
        assert!(mods[0].capabilities.contains(&"backup"));
        // 预留模块未实现、id 唯一
        let ids: Vec<_> = mods.iter().map(|m| m.id).collect();
        assert_eq!(ids.len(), std::collections::HashSet::<_>::from_iter(ids.iter().copied()).len());
        assert!(mods.iter().all(|m| m.implemented || m.capabilities.is_empty()));
        assert!(mods.iter().any(|m| m.id == "trainers"));
        assert!(mods.iter().any(|m| m.id == "mods"));
    }
}
